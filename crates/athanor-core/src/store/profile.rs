//! Learner profile: one row per section (`update_memory` maintains these).

use rusqlite::params;

use crate::error::CoreError;

use super::Store;

impl Store {
    pub fn get_profile_section(&self, section: &str) -> Result<String, CoreError> {
        let content: Option<String> = self
            .conn
            .query_row(
                "SELECT content FROM profile WHERE section = ?1",
                params![section],
                |r| r.get(0),
            )
            .ok();
        Ok(content.unwrap_or_default())
    }

    pub fn set_profile_section(&self, section: &str, content: &str) -> Result<(), CoreError> {
        let now = self.now();
        self.conn.execute(
            "INSERT INTO profile (section, content, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(section) DO UPDATE SET content = excluded.content, updated_at = excluded.updated_at",
            params![section, content, now],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_section_reads_empty_string() {
        let store = Store::open_in_memory("d").unwrap();
        assert_eq!(store.get_profile_section("domains").unwrap(), "");
    }

    #[test]
    fn set_then_get_round_trips() {
        let store = Store::open_in_memory("d").unwrap();
        store
            .set_profile_section("domains", "magnetism, yoga")
            .unwrap();
        assert_eq!(
            store.get_profile_section("domains").unwrap(),
            "magnetism, yoga"
        );
    }

    #[test]
    fn set_twice_overwrites() {
        let store = Store::open_in_memory("d").unwrap();
        store.set_profile_section("how_i_learn", "v1").unwrap();
        store.set_profile_section("how_i_learn", "v2").unwrap();
        assert_eq!(store.get_profile_section("how_i_learn").unwrap(), "v2");
    }
}
