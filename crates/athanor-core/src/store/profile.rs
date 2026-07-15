//! Learner profile: one row per section (`update_memory` maintains these).

use rusqlite::params;

use crate::error::CoreError;

use super::Store;

impl Store {
    pub fn get_profile_section(&self, section: &str) -> Result<String, CoreError> {
        let content: Option<String> = self
            .conn()
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
        self.conn().execute(
            "INSERT INTO profile (section, content, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(section) DO UPDATE SET content = excluded.content, updated_at = excluded.updated_at",
            params![section, content, now],
        )?;
        Ok(())
    }

    /// Merges an addition INTO an existing profile section rather than
    /// clobbering it — the condensation pass refines what the furnace knows
    /// about the learner without erasing prior observations. Empty section →
    /// the addition becomes the content; an addition already present (verbatim
    /// substring) is a no-op; otherwise it's appended on a new line. A blank
    /// addition never touches the section.
    pub fn merge_profile_section(&self, section: &str, addition: &str) -> Result<(), CoreError> {
        let addition = addition.trim();
        if addition.is_empty() {
            return Ok(());
        }
        let existing = self.get_profile_section(section)?;
        if existing.trim().is_empty() {
            return self.set_profile_section(section, addition);
        }
        if existing.contains(addition) {
            return Ok(());
        }
        let merged = format!("{}\n{addition}", existing.trim_end());
        self.set_profile_section(section, &merged)
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

    #[test]
    fn merge_into_empty_sets_content() {
        let store = Store::open_in_memory("d").unwrap();
        store
            .merge_profile_section("how_i_learn", "demands proof")
            .unwrap();
        assert_eq!(
            store.get_profile_section("how_i_learn").unwrap(),
            "demands proof"
        );
    }

    #[test]
    fn merge_appends_without_clobbering_and_dedupes() {
        let store = Store::open_in_memory("d").unwrap();
        store
            .set_profile_section("how_i_learn", "demands proof")
            .unwrap();
        store
            .merge_profile_section("how_i_learn", "thinks in dialogue")
            .unwrap();
        assert_eq!(
            store.get_profile_section("how_i_learn").unwrap(),
            "demands proof\nthinks in dialogue",
            "the prior observation is kept, the new one appended"
        );
        // re-merging something already present is a no-op (no duplicate line)
        store
            .merge_profile_section("how_i_learn", "demands proof")
            .unwrap();
        assert_eq!(
            store.get_profile_section("how_i_learn").unwrap(),
            "demands proof\nthinks in dialogue"
        );
    }

    #[test]
    fn merge_blank_addition_is_a_noop() {
        let store = Store::open_in_memory("d").unwrap();
        store.set_profile_section("frictions", "x").unwrap();
        store.merge_profile_section("frictions", "   ").unwrap();
        assert_eq!(store.get_profile_section("frictions").unwrap(), "x");
    }
}
