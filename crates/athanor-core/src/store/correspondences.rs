//! Azoth's verb: correspondences woven between domains (mask deferred; schema ships now).

use rusqlite::params;

use crate::domain::Correspondence;
use crate::error::CoreError;
use crate::ids::new_id;

use super::Store;

impl Store {
    pub fn weave_domains(&self, a: &str, b: &str, note: &str) -> Result<Correspondence, CoreError> {
        let id = new_id();
        let now = self.now();
        self.conn.execute(
            "INSERT INTO correspondences (id, domain_a, domain_b, note, created_at, device_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, a, b, note, now, self.device_id],
        )?;
        Ok(Correspondence {
            id,
            domain_a: a.to_string(),
            domain_b: b.to_string(),
            note: note.to_string(),
            created_at: now,
            device_id: self.device_id.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weave_domains_lands_a_correspondence() {
        let store = Store::open_in_memory("d").unwrap();
        let domain_a = store.upsert_domain("Magnetism").unwrap();
        let domain_b = store.upsert_domain("Rhetoric").unwrap();
        let corr = store
            .weave_domains(
                &domain_a.id,
                &domain_b.id,
                "both are about invisible attraction",
            )
            .unwrap();
        assert_eq!(corr.domain_a, domain_a.id);
        assert_eq!(corr.domain_b, domain_b.id);
        assert_eq!(corr.note, "both are about invisible attraction");
    }
}
