//! Session notes: the richer distillation written when a session condenses on
//! close (the "nothing is lost" residue). Append-only; read back per session
//! (the reading view) and, via the session's `thread_id`, per thread (a
//! thread's detail lists what each fire on it left behind).

use rusqlite::params;

use crate::domain::SessionNote;
use crate::error::CoreError;
use crate::ids::new_id;

use super::Store;

const NOTE_JOIN_COLS: &str = "n.session_id, s.thread_id, s.mask, n.note, n.created_at";

fn note_from_row(row: &rusqlite::Row) -> rusqlite::Result<SessionNote> {
    Ok(SessionNote {
        session_id: row.get(0)?,
        thread_id: row.get(1)?,
        mask: row.get(2)?,
        note: row.get(3)?,
        created_at: row.get(4)?,
    })
}

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

    /// The most recent session notes across ALL sessions, newest first — the
    /// rich continuity the next session's prompt reads back (see
    /// `prompt::profile_injection`). Joined with the session for its mask/thread.
    pub fn recent_session_notes(&self, limit: usize) -> Result<Vec<SessionNote>, CoreError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {NOTE_JOIN_COLS} FROM session_notes n
             JOIN sessions s ON s.id = n.session_id
             ORDER BY n.created_at DESC, n.id DESC LIMIT ?1"
        ))?;
        let rows = stmt.query_map(params![limit as i64], note_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(CoreError::from)
    }

    /// The most recent notes for ONE thread, newest first — surfaced first when
    /// a session opens on that thread, so it resumes where the last fire on it
    /// left off.
    pub fn thread_session_notes(
        &self,
        thread_id: &str,
        limit: usize,
    ) -> Result<Vec<SessionNote>, CoreError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {NOTE_JOIN_COLS} FROM session_notes n
             JOIN sessions s ON s.id = n.session_id
             WHERE n.thread_id = ?1
             ORDER BY n.created_at DESC, n.id DESC LIMIT ?2"
        ))?;
        let rows = stmt.query_map(params![thread_id, limit as i64], note_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(CoreError::from)
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
    fn recent_session_notes_newest_first_with_mask_and_thread() {
        let store = Store::open_in_memory("d")
            .unwrap()
            .with_clock(std::sync::Arc::new(|| 100));
        let thread = store.open_thread("q", None, None).unwrap();
        let older = store
            .create_session(Some(&thread.id), "philosophus", "explain")
            .unwrap();
        store
            .add_session_note(&older.id, Some(&thread.id), "older residue")
            .unwrap();
        let store = store.with_clock(std::sync::Arc::new(|| 200));
        let newer = store.create_session(None, "adamas", "challenge").unwrap();
        store
            .add_session_note(&newer.id, None, "newer residue")
            .unwrap();

        let notes = store.recent_session_notes(10).unwrap();
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0].note, "newer residue");
        assert_eq!(notes[0].mask, "adamas");
        assert_eq!(notes[1].note, "older residue");
        assert_eq!(notes[1].mask, "philosophus");
        assert_eq!(notes[1].thread_id.as_deref(), Some(thread.id.as_str()));
    }

    #[test]
    fn thread_session_notes_scopes_to_one_thread() {
        let store = Store::open_in_memory("d").unwrap();
        let a = store.open_thread("a", None, None).unwrap();
        let b = store.open_thread("b", None, None).unwrap();
        let sa = store
            .create_session(Some(&a.id), "solve", "design")
            .unwrap();
        store
            .add_session_note(&sa.id, Some(&a.id), "note on A")
            .unwrap();
        let sb = store
            .create_session(Some(&b.id), "solve", "design")
            .unwrap();
        store
            .add_session_note(&sb.id, Some(&b.id), "note on B")
            .unwrap();

        let notes = store.thread_session_notes(&a.id, 10).unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].note, "note on A");
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
