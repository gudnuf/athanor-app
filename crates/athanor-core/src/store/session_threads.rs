//! Session <-> thread links (schema v3): which threads a session touched, and
//! in what role (`focal|worked|opened|fixed|born|evaporated`). The Conductor
//! and the Mystagogue's tools write them; close reads them to record tending
//! thread ids, set `last_worked`, and name the session's primary thread.

use rusqlite::params;

use crate::error::CoreError;

use super::Store;

impl Store {
    /// Records that `session_id` touched `thread_id` in `role`. Idempotent per
    /// `(session, thread, role)`: the first link's timestamp stands.
    pub fn link_session_thread(
        &self,
        session_id: &str,
        thread_id: &str,
        role: &str,
    ) -> Result<(), CoreError> {
        let now = self.now();
        self.conn().execute(
            "INSERT OR IGNORE INTO session_threads (session_id, thread_id, role, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![session_id, thread_id, role, now],
        )?;
        Ok(())
    }

    /// A session's links as `(thread_id, role)`, in a stable order — the
    /// export read.
    pub(crate) fn session_links(
        &self,
        session_id: &str,
    ) -> Result<Vec<(String, String)>, CoreError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT thread_id, role FROM session_threads WHERE session_id = ?1
             ORDER BY created_at ASC, role ASC, thread_id ASC",
        )?;
        let rows = stmt.query_map(params![session_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(CoreError::from)
    }

    /// The distinct threads a session touched, in first-link order, excluding
    /// `evaporated` links (an archived thread is not "worked").
    pub fn session_thread_ids(&self, session_id: &str) -> Result<Vec<String>, CoreError> {
        let mut ids: Vec<String> = Vec::new();
        for (thread_id, role) in self.session_links(session_id)? {
            if role != "evaporated" && !ids.contains(&thread_id) {
                ids.push(thread_id);
            }
        }
        Ok(ids)
    }

    /// The thread to record as the session's own: a `fixed` thread, else a
    /// `worked` one, else `focal`, else `opened`. `born` and `evaporated`
    /// never win. Earliest link breaks ties within a role.
    pub(crate) fn primary_session_thread(
        &self,
        session_id: &str,
    ) -> Result<Option<String>, CoreError> {
        let links = self.session_links(session_id)?;
        for role in ["fixed", "worked", "focal", "opened"] {
            if let Some((t, _)) = links.iter().find(|(_, r)| r == role) {
                return Ok(Some(t.clone()));
            }
        }
        Ok(None)
    }

    /// Sets `last_worked = at` on every thread the session worked. The role
    /// filter lives here: `born` (a child merely spawned) and `evaporated` (an
    /// archive) never count as work.
    pub(crate) fn mark_session_threads_worked(
        &self,
        session_id: &str,
        at: u64,
    ) -> Result<(), CoreError> {
        self.conn().execute(
            "UPDATE threads SET last_worked = ?1
             WHERE id IN (SELECT thread_id FROM session_threads
                          WHERE session_id = ?2
                            AND role IN ('focal','worked','fixed','opened'))",
            params![at, session_id],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    use super::*;

    fn store() -> Store {
        Store::open_in_memory("d").unwrap()
    }

    fn three(store: &Store) -> (String, String, String, String) {
        let sid = store
            .create_session(None, "philosophus", "explain")
            .unwrap()
            .id;
        let a = store.open_thread("a?", None, None).unwrap().id;
        let b = store.open_thread("b?", None, None).unwrap().id;
        let c = store.open_thread("c?", None, None).unwrap().id;
        (sid, a, b, c)
    }

    #[test]
    fn link_is_idempotent_per_role() {
        let s = store();
        let (sid, a, _, _) = three(&s);
        s.link_session_thread(&sid, &a, "worked").unwrap();
        s.link_session_thread(&sid, &a, "worked").unwrap();
        s.link_session_thread(&sid, &a, "fixed").unwrap();
        let n: i64 = s
            .conn()
            .query_row("SELECT count(*) FROM session_threads", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 2, "same role twice is one row; a second role is another");
    }

    #[test]
    fn session_thread_ids_dedupes_in_first_link_order_excluding_evaporated() {
        let t = Arc::new(AtomicU64::new(100));
        let t2 = t.clone();
        let s = store().with_clock(Arc::new(move || t2.load(Ordering::SeqCst)));
        let (sid, a, b, c) = three(&s);
        s.link_session_thread(&sid, &b, "opened").unwrap();
        t.store(101, Ordering::SeqCst);
        s.link_session_thread(&sid, &a, "worked").unwrap();
        t.store(102, Ordering::SeqCst);
        s.link_session_thread(&sid, &b, "fixed").unwrap();
        s.link_session_thread(&sid, &c, "evaporated").unwrap();
        assert_eq!(s.session_thread_ids(&sid).unwrap(), vec![b, a]);
    }

    #[test]
    fn primary_thread_prefers_fixed_then_worked_then_focal_then_opened() {
        let s = store();
        let (sid, a, b, c) = three(&s);
        assert_eq!(s.primary_session_thread(&sid).unwrap(), None);
        s.link_session_thread(&sid, &c, "born").unwrap();
        assert_eq!(
            s.primary_session_thread(&sid).unwrap(),
            None,
            "born never wins"
        );
        s.link_session_thread(&sid, &a, "opened").unwrap();
        assert_eq!(s.primary_session_thread(&sid).unwrap(), Some(a.clone()));
        s.link_session_thread(&sid, &b, "focal").unwrap();
        assert_eq!(s.primary_session_thread(&sid).unwrap(), Some(b.clone()));
        s.link_session_thread(&sid, &a, "worked").unwrap();
        assert_eq!(s.primary_session_thread(&sid).unwrap(), Some(a.clone()));
        s.link_session_thread(&sid, &b, "fixed").unwrap();
        assert_eq!(s.primary_session_thread(&sid).unwrap(), Some(b));
    }

    #[test]
    fn mark_session_threads_worked_touches_only_focal_worked_fixed_opened() {
        let s = store();
        let sid = s.create_session(None, "philosophus", "explain").unwrap().id;
        let mut ids = Vec::new();
        for (i, role) in ["focal", "worked", "fixed", "opened", "born", "evaporated"]
            .iter()
            .enumerate()
        {
            let t = s.open_thread(&format!("q{i}?"), None, None).unwrap().id;
            s.link_session_thread(&sid, &t, role).unwrap();
            ids.push(t);
        }
        s.mark_session_threads_worked(&sid, 777).unwrap();
        let lw: Vec<Option<u64>> = ids
            .iter()
            .map(|t| s.get_thread(t).unwrap().last_worked)
            .collect();
        assert_eq!(
            lw,
            vec![Some(777), Some(777), Some(777), Some(777), None, None]
        );
    }
}
