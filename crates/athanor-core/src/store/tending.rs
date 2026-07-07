//! Fire: one row per UTC day tended. Append-only (upsert-add) by day;
//! wisdom = count(*) of distinct days tended, not total minutes.

use rusqlite::params;

use crate::error::CoreError;

use super::Store;

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
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
