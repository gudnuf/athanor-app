//! Sulfur: domains of desire/interest, and the pull-notes that seed them.

use rusqlite::params;

use crate::domain::{Domain, PullNote};
use crate::error::CoreError;
use crate::ids::new_id;

use super::Store;

fn domain_from_row(row: &rusqlite::Row) -> rusqlite::Result<Domain> {
    Ok(Domain {
        id: row.get(0)?,
        name: row.get(1)?,
        created_at: row.get(2)?,
        updated_at: row.get(3)?,
        device_id: row.get(4)?,
        deleted_at: row.get(5)?,
    })
}

const DOMAIN_COLS: &str = "id, name, created_at, updated_at, device_id, deleted_at";

impl Store {
    /// Upserts a domain by name, case-insensitively. Returns the existing row
    /// if a domain with that name (any case) already exists, else creates one.
    pub fn upsert_domain(&self, name: &str) -> Result<Domain, CoreError> {
        if let Ok(existing) = self.conn().query_row(
            &format!(
                "SELECT {DOMAIN_COLS} FROM domains WHERE name = ?1 COLLATE NOCASE AND deleted_at IS NULL"
            ),
            params![name],
            domain_from_row,
        ) {
            return Ok(existing);
        }
        let id = new_id();
        let now = self.now();
        self.conn().execute(
            "INSERT INTO domains (id, name, created_at, updated_at, device_id) VALUES (?1, ?2, ?3, ?3, ?4)",
            params![id, name, now, self.device_id],
        )?;
        Ok(Domain {
            id,
            name: name.to_string(),
            created_at: now,
            updated_at: now,
            device_id: self.device_id.clone(),
            deleted_at: None,
        })
    }

    pub fn add_pull_note(
        &self,
        domain_id: Option<&str>,
        text: &str,
    ) -> Result<PullNote, CoreError> {
        let id = new_id();
        let now = self.now();
        self.conn().execute(
            "INSERT INTO pull_notes (id, domain_id, text, created_at, device_id) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, domain_id, text, now, self.device_id],
        )?;
        Ok(PullNote {
            id,
            domain_id: domain_id.map(str::to_string),
            text: text.to_string(),
            created_at: now,
            device_id: self.device_id.clone(),
        })
    }

    pub fn list_domains(&self) -> Result<Vec<Domain>, CoreError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {DOMAIN_COLS} FROM domains WHERE deleted_at IS NULL ORDER BY name COLLATE NOCASE"
        ))?;
        let rows = stmt.query_map([], domain_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(CoreError::from)
    }
}

impl Store {
    /// Every domain, tombstoned included, oldest first — the export read.
    pub(crate) fn all_domains(&self) -> Result<Vec<Domain>, CoreError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {DOMAIN_COLS} FROM domains ORDER BY created_at ASC, id ASC"
        ))?;
        let rows = stmt.query_map([], domain_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(CoreError::from)
    }

    /// Every pull note, oldest first — the export read (no other reader exists).
    pub(crate) fn all_pull_notes(&self) -> Result<Vec<PullNote>, CoreError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, domain_id, text, created_at, device_id FROM pull_notes
             ORDER BY created_at ASC, id ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(PullNote {
                id: row.get(0)?,
                domain_id: row.get(1)?,
                text: row.get(2)?,
                created_at: row.get(3)?,
                device_id: row.get(4)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(CoreError::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_domain_creates_then_reuses_case_insensitively() {
        let store = Store::open_in_memory("d").unwrap();
        let a = store.upsert_domain("Magnetism").unwrap();
        let b = store.upsert_domain("MAGNETISM").unwrap();
        assert_eq!(a.id, b.id, "same domain regardless of case");
        assert_eq!(store.list_domains().unwrap().len(), 1);
    }

    #[test]
    fn add_pull_note_lands_and_allows_null_domain() {
        let store = Store::open_in_memory("d").unwrap();
        let domain = store.upsert_domain("Rhetoric").unwrap();
        let note = store
            .add_pull_note(Some(&domain.id), "why does this pull me?")
            .unwrap();
        assert_eq!(note.domain_id.as_deref(), Some(domain.id.as_str()));
        let untethered = store.add_pull_note(None, "unnamed pull").unwrap();
        assert!(untethered.domain_id.is_none());
    }

    #[test]
    fn list_domains_orders_by_name() {
        let store = Store::open_in_memory("d").unwrap();
        store.upsert_domain("Yoga").unwrap();
        store.upsert_domain("Academy").unwrap();
        let names: Vec<_> = store
            .list_domains()
            .unwrap()
            .into_iter()
            .map(|d| d.name)
            .collect();
        assert_eq!(names, vec!["Academy", "Yoga"]);
    }

    #[test]
    fn all_domains_includes_tombstoned_and_all_pull_notes_includes_untethered() {
        let store = Store::open_in_memory("d").unwrap();
        let a = store.upsert_domain("Yoga").unwrap();
        let b = store.upsert_domain("Academy").unwrap();
        store
            .conn()
            .execute(
                "UPDATE domains SET deleted_at = 5 WHERE id = ?1",
                params![b.id],
            )
            .unwrap();
        assert_eq!(store.list_domains().unwrap().len(), 1, "contrast");
        let all = store.all_domains().unwrap();
        assert_eq!(all.len(), 2);
        assert!(all.iter().any(|d| d.id == b.id && d.deleted_at == Some(5)));
        store.add_pull_note(Some(&a.id), "tethered").unwrap();
        store.add_pull_note(None, "untethered").unwrap();
        let notes = store.all_pull_notes().unwrap();
        assert_eq!(notes.len(), 2);
        assert!(notes
            .iter()
            .any(|n| n.domain_id.is_none() && n.text == "untethered"));
    }
}
