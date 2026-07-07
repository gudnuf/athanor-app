//! Tabula passage kindling (derived events; first-kindle wins, never re-fires).

use rusqlite::params;

use crate::error::CoreError;

use super::Store;

impl Store {
    /// Records the first time `passage_key` is kindled. Returns `true` if
    /// this call actually kindled it (first-wins), `false` if it was already
    /// kindled (no-op — kindling never re-fires).
    pub fn kindle_passage(
        &self,
        passage_key: &str,
        source_id: Option<&str>,
    ) -> Result<bool, CoreError> {
        let now = self.now();
        let changed = self.conn().execute(
            "INSERT OR IGNORE INTO kindling (passage_key, first_kindled_at, source_id) VALUES (?1, ?2, ?3)",
            params![passage_key, now, source_id],
        )?;
        Ok(changed == 1)
    }

    pub fn kindled(&self) -> Result<Vec<String>, CoreError> {
        let conn = self.conn();
        let mut stmt =
            conn.prepare("SELECT passage_key FROM kindling ORDER BY first_kindled_at ASC")?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(CoreError::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kindle_passage_first_wins() {
        let store = Store::open_in_memory("d").unwrap();
        assert!(store.kindle_passage("SALT", Some("r1")).unwrap());
        assert!(
            !store.kindle_passage("SALT", Some("r2")).unwrap(),
            "second kindle is a no-op"
        );
        assert_eq!(store.kindled().unwrap(), vec!["SALT".to_string()]);
    }

    #[test]
    fn kindled_lists_in_kindle_order() {
        let store = Store::open_in_memory("d").unwrap();
        store.kindle_passage("SALT", None).unwrap();
        store.kindle_passage("NIGREDO", None).unwrap();
        assert_eq!(
            store.kindled().unwrap(),
            vec!["SALT".to_string(), "NIGREDO".to_string()]
        );
    }
}
