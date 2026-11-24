//! Mercury: threads (open questions). state in {volatile,condensing,fixed,evaporated}.
//!
//! `set_thread_state` validates the requested move against
//! `ThreadState::can_transition_to` (see `session.rs`) before writing —
//! Task 7 layers this transition-legality check on top of what was
//! previously a raw, unconditional setter.

use rusqlite::params;

use crate::domain::{Thread, ThreadState};
use crate::error::CoreError;
use crate::ids::new_id;

use super::Store;

const THREAD_COLS: &str = "id, prompt, domain_id, state, born, last_worked, parent_realization_id, created_at, updated_at, device_id, deleted_at";

fn thread_from_row(row: &rusqlite::Row) -> rusqlite::Result<Thread> {
    let state: String = row.get(3)?;
    Ok(Thread {
        id: row.get(0)?,
        prompt: row.get(1)?,
        domain_id: row.get(2)?,
        state: state.parse().unwrap_or(ThreadState::Volatile),
        born: row.get(4)?,
        last_worked: row.get(5)?,
        parent_realization_id: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
        device_id: row.get(9)?,
        deleted_at: row.get(10)?,
    })
}

impl Store {
    pub fn open_thread(
        &self,
        prompt: &str,
        domain_id: Option<&str>,
        parent_realization_id: Option<&str>,
    ) -> Result<Thread, CoreError> {
        let id = new_id();
        let now = self.now();
        self.conn().execute(
            "INSERT INTO threads (id, prompt, domain_id, state, born, last_worked, parent_realization_id, created_at, updated_at, device_id)
             VALUES (?1, ?2, ?3, 'volatile', ?4, NULL, ?5, ?4, ?4, ?6)",
            params![id, prompt, domain_id, now, parent_realization_id, self.device_id],
        )?;
        self.get_thread(&id)
    }

    /// Sets the thread's state, first checking the move is legal per
    /// `ThreadState::can_transition_to` (the state DAG lives in `session.rs`).
    /// Returns `CoreError::BadState` on an illegal transition.
    pub fn set_thread_state(&self, id: &str, state: ThreadState) -> Result<Thread, CoreError> {
        let current = self.get_thread(id)?;
        if !current.state.can_transition_to(state) {
            return Err(CoreError::BadState(format!(
                "illegal thread transition: {} -> {}",
                current.state.as_str(),
                state.as_str()
            )));
        }
        let now = self.now();
        let changed = self.conn().execute(
            "UPDATE threads SET state = ?1, updated_at = ?2 WHERE id = ?3 AND deleted_at IS NULL",
            params![state.as_str(), now, id],
        )?;
        if changed == 0 {
            return Err(CoreError::NotFound(format!("thread {id}")));
        }
        self.get_thread(id)
    }

    pub fn evaporate_thread(&self, id: &str) -> Result<(), CoreError> {
        self.set_thread_state(id, ThreadState::Evaporated)?;
        Ok(())
    }

    /// Volatile/condensing threads, oldest `last_worked` first (nulls —
    /// never worked — sort first, since they're the ripest to pick up).
    /// No index on (state, last_worked): accepted tradeoff for an on-device
    /// single-user DB with a lifetime-of-one-person dataset size — see the
    /// note beside MIGRATIONS in migrations.rs.
    pub fn ripe_threads(&self, limit: usize) -> Result<Vec<Thread>, CoreError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {THREAD_COLS} FROM threads
             WHERE deleted_at IS NULL AND state IN ('volatile', 'condensing')
             ORDER BY last_worked IS NOT NULL, last_worked ASC, born ASC, id ASC
             LIMIT ?1"
        ))?;
        let rows = stmt.query_map(params![limit as i64], thread_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(CoreError::from)
    }

    /// Every volatile/condensing thread, for the Mercury screen — same
    /// evaporated/fixed exclusion and same ordering as `ripe_threads` (oldest
    /// `last_worked` first, never-worked ranks first) but unbounded, since
    /// Mercury shows the whole open-question pool, not just the next ones to
    /// pick up.
    pub fn open_threads(&self) -> Result<Vec<Thread>, CoreError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {THREAD_COLS} FROM threads
             WHERE deleted_at IS NULL AND state IN ('volatile', 'condensing')
             ORDER BY last_worked IS NOT NULL, last_worked ASC, born ASC, id ASC"
        ))?;
        let rows = stmt.query_map([], thread_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(CoreError::from)
    }

    /// Resolves a `#handle` (the last six hex characters of a thread id, as
    /// shown in `ripe_mercury`) to its live thread. The leading `#` is
    /// optional. Anything that is not exactly six lowercase hex digits is
    /// `Ok(None)` — never a wildcard match. Two live threads sharing a tail is
    /// `BadState("ambiguous handle")`; the caller falls back, never guesses.
    pub fn thread_by_handle(&self, handle: &str) -> Result<Option<Thread>, CoreError> {
        let h = handle.trim();
        let h = h.strip_prefix('#').unwrap_or(h);
        if h.len() != 6
            || !h
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Ok(None);
        }
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {THREAD_COLS} FROM threads WHERE substr(id, -6) = ?1 AND deleted_at IS NULL LIMIT 2"
        ))?;
        let rows: Vec<Thread> = stmt
            .query_map(params![h], thread_from_row)?
            .collect::<Result<_, _>>()?;
        match rows.len() {
            0 => Ok(None),
            1 => Ok(rows.into_iter().next()),
            _ => Err(CoreError::BadState("ambiguous handle".into())),
        }
    }

    pub fn get_thread(&self, id: &str) -> Result<Thread, CoreError> {
        self.conn()
            .query_row(
                &format!("SELECT {THREAD_COLS} FROM threads WHERE id = ?1"),
                params![id],
                thread_from_row,
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => CoreError::NotFound(format!("thread {id}")),
                other => CoreError::Sqlite(other),
            })
    }
}

impl Store {
    /// Every thread in any state, tombstoned included, oldest first — the export read.
    pub(crate) fn all_threads(&self) -> Result<Vec<Thread>, CoreError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {THREAD_COLS} FROM threads ORDER BY created_at ASC, id ASC"
        ))?;
        let rows = stmt.query_map([], thread_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(CoreError::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_thread_starts_volatile_with_no_last_worked() {
        let store = Store::open_in_memory("d").unwrap();
        let t = store.open_thread("why is this?", None, None).unwrap();
        assert_eq!(t.state, ThreadState::Volatile);
        assert!(t.last_worked.is_none());
        assert!(t.parent_realization_id.is_none());
    }

    #[test]
    fn set_thread_state_updates_row() {
        let store = Store::open_in_memory("d").unwrap();
        let t = store.open_thread("q", None, None).unwrap();
        let updated = store
            .set_thread_state(&t.id, ThreadState::Condensing)
            .unwrap();
        assert_eq!(updated.state, ThreadState::Condensing);
    }

    #[test]
    fn evaporate_thread_sets_evaporated() {
        let store = Store::open_in_memory("d").unwrap();
        let t = store.open_thread("q", None, None).unwrap();
        store.evaporate_thread(&t.id).unwrap();
        let reloaded = store.get_thread(&t.id).unwrap();
        assert_eq!(reloaded.state, ThreadState::Evaporated);
    }

    #[test]
    fn ripe_threads_excludes_fixed_and_evaporated() {
        let store = Store::open_in_memory("d").unwrap();
        let volatile = store.open_thread("v", None, None).unwrap();
        let condensing = store.open_thread("c", None, None).unwrap();
        store
            .set_thread_state(&condensing.id, ThreadState::Condensing)
            .unwrap();
        let fixed = store.open_thread("f", None, None).unwrap();
        store
            .set_thread_state(&fixed.id, ThreadState::Condensing)
            .unwrap();
        store
            .set_thread_state(&fixed.id, ThreadState::Fixed)
            .unwrap();

        let ripe = store.ripe_threads(10).unwrap();
        let ids: Vec<_> = ripe.iter().map(|t| t.id.clone()).collect();
        assert!(ids.contains(&volatile.id));
        assert!(ids.contains(&condensing.id));
        assert!(!ids.contains(&fixed.id));
    }

    #[test]
    fn set_thread_state_rejects_illegal_transition() {
        let store = Store::open_in_memory("d").unwrap();
        let t = store.open_thread("q", None, None).unwrap();
        store
            .set_thread_state(&t.id, ThreadState::Condensing)
            .unwrap();
        store.set_thread_state(&t.id, ThreadState::Fixed).unwrap();
        // fixed -> volatile is not a legal move.
        let err = store
            .set_thread_state(&t.id, ThreadState::Volatile)
            .unwrap_err();
        assert!(matches!(err, CoreError::BadState(_)));
    }

    #[test]
    fn get_thread_missing_is_not_found() {
        let store = Store::open_in_memory("d").unwrap();
        let err = store.get_thread("no-such-id").unwrap_err();
        assert!(matches!(err, CoreError::NotFound(_)));
    }

    #[test]
    fn open_threads_excludes_evaporated_and_fixed() {
        let store = Store::open_in_memory("d").unwrap();
        let volatile = store.open_thread("v", None, None).unwrap();
        let condensing = store.open_thread("c", None, None).unwrap();
        store
            .set_thread_state(&condensing.id, ThreadState::Condensing)
            .unwrap();
        let fixed = store.open_thread("f", None, None).unwrap();
        store
            .set_thread_state(&fixed.id, ThreadState::Condensing)
            .unwrap();
        store
            .set_thread_state(&fixed.id, ThreadState::Fixed)
            .unwrap();
        let evaporated = store.open_thread("e", None, None).unwrap();
        store.evaporate_thread(&evaporated.id).unwrap();

        let open = store.open_threads().unwrap();
        let ids: Vec<_> = open.iter().map(|t| t.id.clone()).collect();
        assert!(ids.contains(&volatile.id));
        assert!(ids.contains(&condensing.id));
        assert!(!ids.contains(&fixed.id), "fixed threads are settled");
        assert!(
            !ids.contains(&evaporated.id),
            "evaporated threads were let go"
        );
    }

    #[test]
    fn open_threads_orders_never_worked_first_then_oldest_last_worked() {
        let store = Store::open_in_memory("d").unwrap();
        let never_worked = store.open_thread("never", None, None).unwrap();
        let worked_recently = store.open_thread("recent", None, None).unwrap();
        store
            .conn()
            .execute(
                "UPDATE threads SET last_worked = 200 WHERE id = ?1",
                rusqlite::params![worked_recently.id],
            )
            .unwrap();
        let worked_long_ago = store.open_thread("long-ago", None, None).unwrap();
        store
            .conn()
            .execute(
                "UPDATE threads SET last_worked = 100 WHERE id = ?1",
                rusqlite::params![worked_long_ago.id],
            )
            .unwrap();

        let open = store.open_threads().unwrap();
        let ids: Vec<_> = open.iter().map(|t| t.id.clone()).collect();
        assert_eq!(
            ids,
            vec![never_worked.id, worked_long_ago.id, worked_recently.id],
            "never-worked first, then oldest last_worked"
        );
    }

    #[test]
    fn open_threads_empty_store_is_empty() {
        let store = Store::open_in_memory("d").unwrap();
        assert!(store.open_threads().unwrap().is_empty());
    }

    #[test]
    fn all_threads_returns_every_state_including_evaporated() {
        let store = Store::open_in_memory("d").unwrap();
        let a = store.open_thread("a?", None, None).unwrap();
        let b = store.open_thread("b?", None, None).unwrap();
        store.evaporate_thread(&b.id).unwrap();
        let all = store.all_threads().unwrap();
        assert_eq!(all.len(), 2);
        assert!(all
            .iter()
            .any(|t| t.id == b.id && t.state == ThreadState::Evaporated));
        assert!(store.open_threads().unwrap().iter().all(|t| t.id == a.id)); // contrast
    }

    // ---- S4: handles + ripe order ----

    fn raw_thread(store: &Store, id: &str, born: u64, last_worked: Option<u64>) {
        store
            .conn()
            .execute(
                "INSERT INTO threads (id, prompt, state, born, last_worked, created_at, updated_at, device_id)
                 VALUES (?1, 'q?', 'volatile', ?2, ?3, ?2, ?2, 'd')",
                params![id, born, last_worked],
            )
            .unwrap();
    }

    #[test]
    fn thread_by_handle_resolves_suffix_with_or_without_hash() {
        let store = Store::open_in_memory("d").unwrap();
        let t = store.open_thread("a?", None, None).unwrap();
        let h = crate::ids::handle_of(&t.id);
        assert_eq!(store.thread_by_handle(&h).unwrap().unwrap().id, t.id);
        assert_eq!(store.thread_by_handle(&h[1..]).unwrap().unwrap().id, t.id);
    }

    #[test]
    fn thread_by_handle_rejects_wildcards_and_short_input() {
        let store = Store::open_in_memory("d").unwrap();
        store.open_thread("a?", None, None).unwrap();
        for bad in [
            "%", "_a", "", "abc", "#%%%%%%", "aaaaa_", "ABCDEF", "#abcdefg",
        ] {
            assert!(
                matches!(store.thread_by_handle(bad), Ok(None)),
                "{bad:?} must not match"
            );
        }
    }

    #[test]
    fn ambiguous_handle_errors() {
        let store = Store::open_in_memory("d").unwrap();
        raw_thread(&store, "aaaa-111111", 1, None);
        raw_thread(&store, "bbbb-111111", 2, None);
        assert!(matches!(
            store.thread_by_handle("#111111"),
            Err(CoreError::BadState(m)) if m.contains("ambiguous")
        ));
    }

    #[test]
    fn ripe_order_ties_break_by_born_then_id() {
        let store = Store::open_in_memory("d").unwrap();
        raw_thread(&store, "c", 5, Some(50));
        raw_thread(&store, "b", 2, None);
        raw_thread(&store, "a", 2, None);
        raw_thread(&store, "d", 1, Some(50));
        let ids = |v: Vec<Thread>| v.into_iter().map(|t| t.id).collect::<Vec<_>>();
        // never-worked first (born, then id), then worked oldest-first (born, id)
        assert_eq!(ids(store.ripe_threads(10).unwrap()), ["a", "b", "d", "c"]);
        assert_eq!(ids(store.open_threads().unwrap()), ["a", "b", "d", "c"]);
    }
}
