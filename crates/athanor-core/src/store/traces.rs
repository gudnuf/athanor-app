//! One-line session traces future sessions read (Mystagogue's "last time" memory).

use rusqlite::params;

use crate::error::CoreError;
use crate::ids::new_id;

use super::Store;

impl Store {
    pub fn add_trace(&self, session_id: &str, text: &str) -> Result<(), CoreError> {
        let id = new_id();
        let now = self.now();
        self.conn().execute(
            "INSERT INTO traces (id, session_id, text, created_at, device_id) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, session_id, text, now, self.device_id],
        )?;
        Ok(())
    }

    /// Most recently created trace's text, or None if no traces yet.
    pub fn last_trace(&self) -> Result<Option<String>, CoreError> {
        self.conn()
            .query_row(
                "SELECT text FROM traces ORDER BY created_at DESC, id DESC LIMIT 1",
                [],
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
    fn last_trace_is_none_when_empty() {
        let store = Store::open_in_memory("d").unwrap();
        assert_eq!(store.last_trace().unwrap(), None);
    }

    #[test]
    fn last_trace_returns_most_recent() {
        let store = Store::open_in_memory("d").unwrap();
        let session = store.create_session(None, "mystagogue", "trace").unwrap();
        store.add_trace(&session.id, "first").unwrap();
        store.add_trace(&session.id, "second").unwrap();
        assert_eq!(store.last_trace().unwrap(), Some("second".to_string()));
    }

    #[test]
    fn add_trace_rejects_unknown_session() {
        let store = Store::open_in_memory("d").unwrap();
        let err = store.add_trace("no-such-session", "text");
        assert!(
            err.is_err(),
            "FK on traces.session_id must reject dangling session"
        );
    }
}
