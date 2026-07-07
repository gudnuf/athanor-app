//! Fire: one row per UTC day tended. Append-only (upsert-add) by day;
//! wisdom = count(*) of distinct days tended, not total minutes.

use rusqlite::params;

use crate::domain::{FireState, Tending};
use crate::error::CoreError;
use crate::session::today_utc;

use super::Store;

/// Size of the `recent` window `fire_state` returns — enough tended days for
/// the Furnace's recency-aware copy ("the fire is warm" vs "the fire is
/// low", mockups-v2.html screen 2) to be derived client-side without a
/// second round-trip, without shipping the whole tending history every time
/// the home screen loads.
const RECENT_TENDING_WINDOW: usize = 7;

impl Store {
    /// Adds `minutes` to the day's tally and unions `thread_ids` into the
    /// day's set (deduped). Two calls the same day merge into one row —
    /// e.g. 7 then 5 minutes → one row, minutes=12 (see Task 7's worked
    /// example).
    pub fn record_tending(
        &self,
        day: &str,
        minutes: u32,
        thread_ids: &[String],
    ) -> Result<(), CoreError> {
        let now = self.now();
        let existing: Option<(u32, String)> = self
            .conn()
            .query_row(
                "SELECT minutes, thread_ids FROM tending WHERE day = ?1",
                params![day],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok();

        match existing {
            Some((prev_minutes, prev_ids_json)) => {
                let mut ids: Vec<String> = serde_json::from_str(&prev_ids_json)?;
                for id in thread_ids {
                    if !ids.contains(id) {
                        ids.push(id.clone());
                    }
                }
                let ids_json = serde_json::to_string(&ids)?;
                self.conn().execute(
                    "UPDATE tending SET minutes = ?1, thread_ids = ?2 WHERE day = ?3",
                    params![prev_minutes + minutes, ids_json, day],
                )?;
            }
            None => {
                let ids_json = serde_json::to_string(thread_ids)?;
                self.conn().execute(
                    "INSERT INTO tending (day, minutes, thread_ids, created_at, device_id) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![day, minutes, ids_json, now, self.device_id],
                )?;
            }
        }
        Ok(())
    }

    /// Count of distinct days tended — the wisdom accounting is "did I show
    /// up today," not "how many minutes total."
    pub fn wisdom_days(&self) -> Result<u64, CoreError> {
        let count: i64 = self
            .conn()
            .query_row("SELECT count(*) FROM tending", [], |r| r.get(0))?;
        Ok(count as u64)
    }

    /// Furnace heat state for the home screen: wisdom accounting plus a
    /// small recent-tending window. `tended_today` compares the latest
    /// tending row's day against `today_utc(self.now())`, so it honors the
    /// injected clock the same way `close_session`/`record_tending` do.
    pub fn fire_state(&self) -> Result<FireState, CoreError> {
        let wisdom_days = self.wisdom_days()?;
        let last_tended_day: Option<String> = self
            .conn
            .query_row(
                "SELECT day FROM tending ORDER BY day DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .ok();
        let today = today_utc(self.now());
        let tended_today = last_tended_day.as_deref() == Some(today.as_str());
        let recent = self.recent_tending(RECENT_TENDING_WINDOW)?;
        Ok(FireState {
            wisdom_days,
            last_tended_day,
            tended_today,
            recent,
        })
    }

    /// The most recent `limit` tended days, most-recent-first (`day DESC` —
    /// the `YYYY-MM-DD` format sorts lexicographically = chronologically).
    fn recent_tending(&self, limit: usize) -> Result<Vec<Tending>, CoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT day, minutes, thread_ids, created_at, device_id
             FROM tending ORDER BY day DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |r| {
            let ids_json: String = r.get(2)?;
            let thread_ids: Vec<String> = serde_json::from_str(&ids_json).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    2,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?;
            Ok(Tending {
                day: r.get(0)?,
                minutes: r.get(1)?,
                thread_ids,
                created_at: r.get(3)?,
                device_id: r.get(4)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(CoreError::from)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::store::Clock;

    /// A fixed clock anchored at a known epoch, so tests can derive the
    /// matching `YYYY-MM-DD` via the same `today_utc` the store uses,
    /// without depending on `session.rs`'s test-only day->epoch inverse.
    fn fixed_clock(epoch_secs: u64) -> Clock {
        Arc::new(move || epoch_secs)
    }

    #[test]
    fn record_tending_merges_same_day() {
        let store = Store::open_in_memory("d").unwrap();
        store
            .record_tending("2026-07-06", 7, &["t1".to_string()])
            .unwrap();
        store
            .record_tending("2026-07-06", 5, &["t2".to_string()])
            .unwrap();
        let minutes: u32 = store
            .conn()
            .query_row(
                "SELECT minutes FROM tending WHERE day = '2026-07-06'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(minutes, 12);
        assert_eq!(store.wisdom_days().unwrap(), 1);
    }

    #[test]
    fn record_tending_different_days_counts_separately() {
        let store = Store::open_in_memory("d").unwrap();
        store.record_tending("2026-07-06", 7, &[]).unwrap();
        store.record_tending("2026-07-07", 5, &[]).unwrap();
        assert_eq!(store.wisdom_days().unwrap(), 2);
    }

    #[test]
    fn record_tending_dedups_thread_ids() {
        let store = Store::open_in_memory("d").unwrap();
        store
            .record_tending("2026-07-06", 1, &["t1".to_string()])
            .unwrap();
        store
            .record_tending("2026-07-06", 1, &["t1".to_string(), "t2".to_string()])
            .unwrap();
        let ids_json: String = store
            .conn()
            .query_row(
                "SELECT thread_ids FROM tending WHERE day = '2026-07-06'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let ids: Vec<String> = serde_json::from_str(&ids_json).unwrap();
        assert_eq!(ids, vec!["t1".to_string(), "t2".to_string()]);
    }

    #[test]
    fn fire_state_on_a_cold_store_is_empty() {
        let store = Store::open_in_memory("d").unwrap();
        let fs = store.fire_state().unwrap();
        assert_eq!(fs.wisdom_days, 0);
        assert_eq!(fs.last_tended_day, None);
        assert!(!fs.tended_today);
        assert!(fs.recent.is_empty());
    }

    #[test]
    fn fire_state_tended_today_true_after_recording_today() {
        let store = Store::open_in_memory("d")
            .unwrap()
            .with_clock(fixed_clock(1_800_000_000));
        let today = today_utc(store.now());
        store
            .record_tending(&today, 12, &["t1".to_string()])
            .unwrap();

        let fs = store.fire_state().unwrap();
        assert_eq!(fs.wisdom_days, 1);
        assert_eq!(fs.last_tended_day, Some(today.clone()));
        assert!(fs.tended_today);
        assert_eq!(fs.recent.len(), 1);
        assert_eq!(fs.recent[0].day, today);
        assert_eq!(fs.recent[0].minutes, 12);
    }

    #[test]
    fn fire_state_tended_today_false_when_last_tending_was_yesterday() {
        let store = Store::open_in_memory("d")
            .unwrap()
            .with_clock(fixed_clock(1_800_000_000));
        let yesterday = today_utc(store.now() - 86_400);
        store.record_tending(&yesterday, 5, &[]).unwrap();

        let fs = store.fire_state().unwrap();
        assert!(
            !fs.tended_today,
            "the fire wasn't fed today, only yesterday"
        );
        assert_eq!(fs.last_tended_day, Some(yesterday));
    }

    #[test]
    fn fire_state_recent_window_caps_at_seven_most_recent_days_first() {
        let store = Store::open_in_memory("d").unwrap();
        for day in 1..=10 {
            store
                .record_tending(&format!("2026-07-{day:02}"), day, &[])
                .unwrap();
        }

        let fs = store.fire_state().unwrap();
        assert_eq!(fs.wisdom_days, 10, "wisdom counts every tended day");
        assert_eq!(fs.recent.len(), 7, "recent window is capped");
        let days: Vec<_> = fs.recent.iter().map(|t| t.day.clone()).collect();
        assert_eq!(
            days,
            vec![
                "2026-07-10",
                "2026-07-09",
                "2026-07-08",
                "2026-07-07",
                "2026-07-06",
                "2026-07-05",
                "2026-07-04",
            ],
            "most-recent day first"
        );
        assert_eq!(fs.last_tended_day, Some("2026-07-10".to_string()));
    }
}
