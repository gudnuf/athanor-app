//! Sessions: the dialogue container + its selected mask/mode. Row CRUD lands
//! here; `close_session`/`abandon_session` (the state-machine entry points,
//! Task 7) live in `session.rs` and call the `mark_session_*` row-writers
//! below.

use rusqlite::params;

use crate::domain::Session;
use crate::error::CoreError;
use crate::ids::new_id;

use super::Store;

const SESSION_COLS: &str = "id, thread_id, mask, mode, state, transcript, started_at, ended_at, created_at, updated_at, device_id, minutes, tended_day";

fn session_from_row(row: &rusqlite::Row) -> rusqlite::Result<Session> {
    Ok(Session {
        id: row.get(0)?,
        thread_id: row.get(1)?,
        mask: row.get(2)?,
        mode: row.get(3)?,
        state: row.get(4)?,
        transcript: row.get(5)?,
        started_at: row.get(6)?,
        ended_at: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
        device_id: row.get(10)?,
        minutes: row.get(11)?,
        tended_day: row.get(12)?,
    })
}

impl Store {
    pub fn create_session(
        &self,
        thread_id: Option<&str>,
        mask: &str,
        mode: &str,
    ) -> Result<Session, CoreError> {
        let id = new_id();
        let now = self.now();
        self.conn().execute(
            "INSERT INTO sessions (id, thread_id, mask, mode, state, transcript, started_at, created_at, updated_at, device_id)
             VALUES (?1, ?2, ?3, ?4, 'open', '', ?5, ?5, ?5, ?6)",
            params![id, thread_id, mask, mode, now, self.device_id],
        )?;
        self.get_session(&id)
    }

    /// Persists a session's current `(mask, mode)` — written when the model
    /// shifts the mask mid-session (`shift_mask`) or the learner pins one, so
    /// the row is a truthful record of the register the session is running
    /// under (lane 13). The live source of truth during a session is the shared
    /// `MaskState` cell; this keeps the durable row in step.
    pub fn set_session_mask_mode(
        &self,
        session_id: &str,
        mask: &str,
        mode: &str,
    ) -> Result<(), CoreError> {
        let now = self.now();
        let changed = self.conn().execute(
            "UPDATE sessions SET mask = ?1, mode = ?2, updated_at = ?3 WHERE id = ?4",
            params![mask, mode, now, session_id],
        )?;
        if changed == 0 {
            return Err(CoreError::NotFound(format!("session {session_id}")));
        }
        Ok(())
    }

    pub fn append_transcript(&self, session_id: &str, chunk: &str) -> Result<(), CoreError> {
        let now = self.now();
        let changed = self.conn().execute(
            "UPDATE sessions SET transcript = transcript || ?1, updated_at = ?2 WHERE id = ?3",
            params![chunk, now, session_id],
        )?;
        if changed == 0 {
            return Err(CoreError::NotFound(format!("session {session_id}")));
        }
        Ok(())
    }

    /// Sets state='closed', ended_at=now. Raw row-writer; `close_session`
    /// (session.rs) is the one that also records tending for the day.
    pub(crate) fn mark_session_closed(&self, id: &str) -> Result<Session, CoreError> {
        self.mark_session_ended(id, "closed")
    }

    /// Sets state='abandoned', ended_at=now. Raw row-writer; `abandon_session`
    /// (session.rs) is the one that also returns the thread to volatile.
    pub(crate) fn mark_session_abandoned(&self, id: &str) -> Result<Session, CoreError> {
        self.mark_session_ended(id, "abandoned")
    }

    fn mark_session_ended(&self, id: &str, state: &str) -> Result<Session, CoreError> {
        let now = self.now();
        let changed = self.conn().execute(
            "UPDATE sessions SET state = ?1, ended_at = ?2, updated_at = ?2 WHERE id = ?3",
            params![state, now, id],
        )?;
        if changed == 0 {
            return Err(CoreError::NotFound(format!("session {id}")));
        }
        self.get_session(id)
    }

    /// Whole minutes between the session's start and its last activity
    /// (`updated_at`, which `append_transcript` advances), rounded up and
    /// clamped to [1, 60]. Read BEFORE the row is closed: closing overwrites
    /// `updated_at`.
    pub(crate) fn measured_minutes(&self, session_id: &str) -> Result<u32, CoreError> {
        let s = self.get_session(session_id)?;
        let span = s.updated_at.saturating_sub(s.started_at);
        Ok(span.div_ceil(60).clamp(1, 60) as u32)
    }

    /// Writes what close learned onto the row: measured minutes, the tended
    /// UTC day, and (only if the row has none) the primary thread.
    pub(crate) fn finish_session_row(
        &self,
        id: &str,
        minutes: u32,
        tended_day: Option<&str>,
        primary_thread: Option<&str>,
    ) -> Result<(), CoreError> {
        let changed = self.conn().execute(
            "UPDATE sessions SET minutes = ?1, tended_day = ?2, thread_id = COALESCE(thread_id, ?3)
             WHERE id = ?4",
            params![minutes, tended_day, primary_thread, id],
        )?;
        if changed == 0 {
            return Err(CoreError::NotFound(format!("session {id}")));
        }
        Ok(())
    }

    /// When the most recent CLOSED session in which the learner spoke ended.
    /// "Spoke" = the durable transcript carries a learner block (the same marker
    /// the v3 backfill filters on); a session that never heard the learner is
    /// abandoned at close, so it is not "last time". `None` if there is none.
    pub(crate) fn last_learner_session_end(&self) -> Result<Option<u64>, CoreError> {
        let marker = crate::transcript::TranscriptRole::Learner.marker();
        let end: Option<u64> = self.conn().query_row(
            "SELECT MAX(ended_at) FROM sessions
             WHERE state = 'closed' AND ended_at IS NOT NULL AND instr(transcript, ?1) > 0",
            params![marker],
            |r| r.get(0),
        )?;
        Ok(end)
    }

    /// Per-mask last-used timestamp: `(mask, MAX(created_at))` over every
    /// session, one row per distinct mask. Feeds the home-screen mask-affinity
    /// heat (lane 14) — the mask the Mystagogue last wore runs warm, others
    /// cool, an unused mask stays cold iron.
    pub(crate) fn mask_usage(&self) -> Result<Vec<(String, u64)>, CoreError> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT mask, MAX(created_at) FROM sessions GROUP BY mask")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u64>(1)?)))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(CoreError::from)
    }

    /// A single session by id — the reading view reads the full role-tagged
    /// transcript off this row. Public (unlike `get_session`) because the FFI
    /// bridge, a separate crate, projects it into `SessionDetail`.
    pub fn session_by_id(&self, id: &str) -> Result<Session, CoreError> {
        self.get_session(id)
    }

    /// Closed sessions on a thread, newest first — a thread's detail lists every
    /// fire tended on it. Only `closed` sessions (a real, landed history);
    /// abandoned/open sessions have no settled record to show.
    pub fn sessions_for_thread(&self, thread_id: &str) -> Result<Vec<Session>, CoreError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {SESSION_COLS} FROM sessions
             WHERE thread_id = ?1 AND state = 'closed'
             ORDER BY created_at DESC, id DESC"
        ))?;
        let rows = stmt.query_map(params![thread_id], session_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(CoreError::from)
    }

    /// The most recent closed sessions regardless of thread — the "past fires"
    /// surface (including threadless ones: initiation, bare tend-the-fire opens).
    /// `offset` paginates (0 = newest page): the follow-on continuous-chat lane
    /// scrolls back through history a page at a time, so this is limit+offset-able
    /// against a stable newest-first order.
    pub fn recent_sessions(&self, limit: usize, offset: usize) -> Result<Vec<Session>, CoreError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {SESSION_COLS} FROM sessions
             WHERE state = 'closed'
             ORDER BY created_at DESC, id DESC
             LIMIT ?1 OFFSET ?2"
        ))?;
        let rows = stmt.query_map(params![limit as i64, offset as i64], session_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(CoreError::from)
    }

    /// The single most recent closed session, or `None` if none have landed —
    /// the cheap read the resume surface uses to reopen where the last fire
    /// ended.
    pub fn most_recent_session(&self) -> Result<Option<Session>, CoreError> {
        self.conn()
            .query_row(
                &format!(
                    "SELECT {SESSION_COLS} FROM sessions
                     WHERE state = 'closed'
                     ORDER BY created_at DESC, id DESC LIMIT 1"
                ),
                [],
                session_from_row,
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(CoreError::Sqlite(other)),
            })
    }

    pub(crate) fn get_session(&self, id: &str) -> Result<Session, CoreError> {
        self.conn()
            .query_row(
                &format!("SELECT {SESSION_COLS} FROM sessions WHERE id = ?1"),
                params![id],
                session_from_row,
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    CoreError::NotFound(format!("session {id}"))
                }
                other => CoreError::Sqlite(other),
            })
    }
}

impl Store {
    /// Every session in any state, unbounded, oldest first — the export read.
    pub(crate) fn all_sessions(&self) -> Result<Vec<Session>, CoreError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {SESSION_COLS} FROM sessions ORDER BY started_at ASC, id ASC"
        ))?;
        let rows = stmt.query_map([], session_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(CoreError::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_session_starts_open_with_empty_transcript() {
        let store = Store::open_in_memory("d").unwrap();
        let session = store.create_session(None, "mystagogue", "trace").unwrap();
        assert_eq!(session.state, "open");
        assert_eq!(session.transcript, "");
        assert!(session.thread_id.is_none());
    }

    #[test]
    fn set_session_mask_mode_updates_the_row() {
        let store = Store::open_in_memory("d").unwrap();
        let session = store
            .create_session(None, "philosophus", "explain")
            .unwrap();
        store
            .set_session_mask_mode(&session.id, "adamas", "challenge")
            .unwrap();
        let reloaded = store.get_session(&session.id).unwrap();
        assert_eq!(reloaded.mask, "adamas");
        assert_eq!(reloaded.mode, "challenge");
    }

    #[test]
    fn append_transcript_accumulates_chunks() {
        let store = Store::open_in_memory("d").unwrap();
        let session = store.create_session(None, "mystagogue", "trace").unwrap();
        store.append_transcript(&session.id, "hello ").unwrap();
        store.append_transcript(&session.id, "world").unwrap();
        let reloaded = store.get_session(&session.id).unwrap();
        assert_eq!(reloaded.transcript, "hello world");
    }

    #[test]
    fn sessions_for_thread_returns_only_closed_newest_first() {
        let store = Store::open_in_memory("d")
            .unwrap()
            .with_clock(std::sync::Arc::new(|| 1000));
        let thread = store.open_thread("why?", None, None).unwrap();
        // an open session on the thread — excluded (no settled record)
        store
            .create_session(Some(&thread.id), "philosophus", "explain")
            .unwrap();
        // two closed sessions on the thread
        let older = store
            .create_session(Some(&thread.id), "philosophus", "explain")
            .unwrap();
        store.mark_session_closed(&older.id).unwrap();
        let store = store.with_clock(std::sync::Arc::new(|| 2000));
        let newer = store
            .create_session(Some(&thread.id), "adamas", "challenge")
            .unwrap();
        store.mark_session_closed(&newer.id).unwrap();
        // a closed session on a DIFFERENT thread — excluded
        let other_thread = store.open_thread("other", None, None).unwrap();
        let other = store
            .create_session(Some(&other_thread.id), "solve", "design")
            .unwrap();
        store.mark_session_closed(&other.id).unwrap();

        let ids: Vec<String> = store
            .sessions_for_thread(&thread.id)
            .unwrap()
            .into_iter()
            .map(|s| s.id)
            .collect();
        assert_eq!(ids, vec![newer.id, older.id], "closed only, newest first");
    }

    #[test]
    fn recent_sessions_spans_threads_and_threadless_newest_first() {
        let store = Store::open_in_memory("d")
            .unwrap()
            .with_clock(std::sync::Arc::new(|| 10));
        let threadless = store
            .create_session(None, "philosophus", "explain")
            .unwrap();
        store.mark_session_closed(&threadless.id).unwrap();
        let store = store.with_clock(std::sync::Arc::new(|| 20));
        let thread = store.open_thread("q", None, None).unwrap();
        let onthread = store
            .create_session(Some(&thread.id), "adamas", "challenge")
            .unwrap();
        store.mark_session_closed(&onthread.id).unwrap();
        // an open one is excluded
        store.create_session(None, "solve", "design").unwrap();

        let ids: Vec<String> = store
            .recent_sessions(10, 0)
            .unwrap()
            .into_iter()
            .map(|s| s.id)
            .collect();
        assert_eq!(ids, vec![onthread.id.clone(), threadless.id.clone()]);

        // pagination: offset 1 drops the newest, offset past the end is empty.
        let page2: Vec<String> = store
            .recent_sessions(10, 1)
            .unwrap()
            .into_iter()
            .map(|s| s.id)
            .collect();
        assert_eq!(page2, vec![threadless.id.clone()]);
        assert!(store.recent_sessions(10, 5).unwrap().is_empty());

        // most_recent_session is the newest closed one.
        assert_eq!(
            store.most_recent_session().unwrap().unwrap().id,
            onthread.id
        );
    }

    #[test]
    fn most_recent_session_is_none_with_no_closed_sessions() {
        let store = Store::open_in_memory("d").unwrap();
        store.create_session(None, "solve", "design").unwrap(); // open, not closed
        assert!(store.most_recent_session().unwrap().is_none());
    }

    #[test]
    fn all_sessions_includes_open_and_abandoned() {
        let store = Store::open_in_memory("d").unwrap();
        let open = store
            .create_session(None, "philosophus", "explain")
            .unwrap();
        let gone = store
            .create_session(None, "philosophus", "explain")
            .unwrap();
        store.mark_session_abandoned(&gone.id).unwrap();
        let all = store.all_sessions().unwrap();
        assert_eq!(all.len(), 2);
        assert!(all.iter().any(|s| s.id == open.id && s.state == "open"));
        assert!(all
            .iter()
            .any(|s| s.id == gone.id && s.state == "abandoned"));
        assert!(store.recent_sessions(10, 0).unwrap().is_empty(), "contrast");
    }

    #[test]
    fn measured_minutes_uses_last_activity_not_close_time() {
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::sync::Arc;
        let t = Arc::new(AtomicU64::new(1000));
        let t2 = t.clone();
        let store = Store::open_in_memory("d")
            .unwrap()
            .with_clock(Arc::new(move || t2.load(Ordering::SeqCst)));
        let s = store
            .create_session(None, "philosophus", "explain")
            .unwrap();
        assert_eq!(store.measured_minutes(&s.id).unwrap(), 1, "floor of 1");
        t.store(1430, Ordering::SeqCst);
        store.append_transcript(&s.id, "x").unwrap();
        t.store(9000, Ordering::SeqCst);
        assert_eq!(store.measured_minutes(&s.id).unwrap(), 8, "430s -> 8 min");
        t.store(1000 + 4 * 3600, Ordering::SeqCst);
        store.append_transcript(&s.id, "y").unwrap();
        assert_eq!(store.measured_minutes(&s.id).unwrap(), 60, "clamped to 60");
    }
}
