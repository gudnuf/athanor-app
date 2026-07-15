//! Session notes: the richer distillation written when a session condenses on
//! close (the "nothing is lost" residue). Append-only; read back per session
//! (the reading view) and, via the session's `thread_id`, per thread (a
//! thread's detail lists what each fire on it left behind).

use rusqlite::params;

use crate::error::CoreError;
use crate::ids::new_id;

use super::Store;

impl Store {
    /// Records a session's condensation note. `thread_id` is the session's
    /// focal thread (if any) so the note is reachable from that thread's
    /// detail without re-reading the session row.
    pub fn add_session_note(
        &self,
        session_id: &str,
        thread_id: Option<&str>,
        note: &str,
    ) -> Result<(), CoreError> {
        let id = new_id();
        let now = self.now();
        self.conn().execute(
            "INSERT INTO session_notes (id, session_id, thread_id, note, created_at, device_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, session_id, thread_id, note, now, self.device_id],
        )?;
        Ok(())
    }

    /// The most recent note for a session, or `None` if it condensed nothing
    /// (best-effort condensation that fell through, or a pre-feature session).
    pub fn session_note(&self, session_id: &str) -> Result<Option<String>, CoreError> {
        self.conn()
            .query_row(
                "SELECT note FROM session_notes WHERE session_id = ?1
                 ORDER BY created_at DESC, id DESC LIMIT 1",
                params![session_id],
                |r| r.get(0),
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(CoreError::Sqlite(other)),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_note_is_none_when_unwritten() {
        let store = Store::open_in_memory("d").unwrap();
        let s = store
            .create_session(None, "philosophus", "explain")
            .unwrap();
        assert_eq!(store.session_note(&s.id).unwrap(), None);
    }

    #[test]
    fn add_then_read_returns_latest_note() {
        let store = Store::open_in_memory("d").unwrap();
        let s = store
            .create_session(None, "philosophus", "explain")
            .unwrap();
        store.add_session_note(&s.id, None, "first pass").unwrap();
        store.add_session_note(&s.id, None, "refined pass").unwrap();
        assert_eq!(
            store.session_note(&s.id).unwrap(),
            Some("refined pass".to_string())
        );
    }

    #[test]
    fn add_session_note_carries_thread_id_and_rejects_dangling_session() {
        let store = Store::open_in_memory("d").unwrap();
        let thread = store.open_thread("why?", None, None).unwrap();
        let s = store
            .create_session(Some(&thread.id), "philosophus", "explain")
            .unwrap();
        store
            .add_session_note(&s.id, Some(&thread.id), "circled the same knot")
            .unwrap();
        assert_eq!(
            store.session_note(&s.id).unwrap(),
            Some("circled the same knot".to_string())
        );
        // FK on session_notes.session_id rejects a dangling session.
        assert!(store
            .add_session_note("no-such-session", None, "x")
            .is_err());
    }
}
