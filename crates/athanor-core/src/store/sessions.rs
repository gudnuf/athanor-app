//! Sessions: the dialogue container + its selected mask/mode. Row CRUD lands
//! here; the open/closed/abandoned state-machine methods land in Task 7.

use rusqlite::params;

use crate::domain::Session;
use crate::error::CoreError;
use crate::ids::new_id;

use super::Store;

const SESSION_COLS: &str = "id, thread_id, mask, mode, state, transcript, started_at, ended_at, created_at, updated_at, device_id";

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
        self.conn.execute(
            "INSERT INTO sessions (id, thread_id, mask, mode, state, transcript, started_at, created_at, updated_at, device_id)
             VALUES (?1, ?2, ?3, ?4, 'open', '', ?5, ?5, ?5, ?6)",
            params![id, thread_id, mask, mode, now, self.device_id],
        )?;
        self.get_session(&id)
    }

    pub fn append_transcript(&self, session_id: &str, chunk: &str) -> Result<(), CoreError> {
        let now = self.now();
        let changed = self.conn.execute(
            "UPDATE sessions SET transcript = transcript || ?1, updated_at = ?2 WHERE id = ?3",
            params![chunk, now, session_id],
        )?;
        if changed == 0 {
            return Err(CoreError::NotFound(format!("session {session_id}")));
        }
        Ok(())
    }

    pub(crate) fn get_session(&self, id: &str) -> Result<Session, CoreError> {
        self.conn
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
    fn append_transcript_accumulates_chunks() {
        let store = Store::open_in_memory("d").unwrap();
        let session = store.create_session(None, "mystagogue", "trace").unwrap();
        store.append_transcript(&session.id, "hello ").unwrap();
        store.append_transcript(&session.id, "world").unwrap();
        let reloaded = store.get_session(&session.id).unwrap();
        assert_eq!(reloaded.transcript, "hello world");
    }
}
