//! Salt: realizations. Immutable once written (no updated_at/deleted_at).
//!
//! `fix_salt` is the SOLE writer of this table and the enforcer of the
//! thread↔realization spiral invariant. In one `unchecked_transaction` it:
//!   (a) inserts the immutable `realizations` row (child_thread_id NULL),
//!   (b) synthesizes and inserts the child thread that references the new
//!       realization (`parent_realization_id = rid`),
//!   (c) back-links the realization to that child (`child_thread_id = ?`),
//!   (d) fixes the parent thread, and
//!   (e) kindles the SALT passage.
//! This order is FK-consistent: the realization's `thread_id` points at the
//! already-existing parent thread, its `child_thread_id` starts NULL, and the
//! child thread's `parent_realization_id` points at a row that now exists —
//! so every immediate FK check (foreign_keys=ON) passes. The deliberate
//! cross-link cycle (threads.parent_realization_id ↔ realizations.child_thread_id)
//! is closed by the final UPDATE, all before COMMIT.
//!
//! Immutability is enforced by ABSENCE: there is no update/delete path for
//! realizations. `try_mutate_realization` exists only to make that guarantee
//! explicit and testable — it always returns `Err(Immutable)`.

use rusqlite::params;

use crate::domain::{Realization, Thread, ThreadState};
use crate::error::CoreError;
use crate::ids::new_id;

use super::Store;

const REALIZATION_COLS: &str = "id, text, date, thread_id, child_thread_id, created_at, device_id";

/// Default question used to birth the child thread when the model fixes salt
/// without naming the next question. The spiral is structural, not optional:
/// every realization opens the next thread.
const DEFAULT_CHILD_QUESTION: &str = "what does this open?";

fn realization_from_row(row: &rusqlite::Row) -> rusqlite::Result<Realization> {
    Ok(Realization {
        id: row.get(0)?,
        text: row.get(1)?,
        date: row.get(2)?,
        thread_id: row.get(3)?,
        child_thread_id: row.get(4)?,
        created_at: row.get(5)?,
        device_id: row.get(6)?,
    })
}

impl Store {
    /// Fix salt: write an immutable realization AND birth its child thread in
    /// one transaction, enforcing the spiral invariant. `domain_names` are
    /// upserted (case-insensitive) and linked via `realization_domains`; the
    /// child thread inherits the parent thread's domain. If `child_question`
    /// is `None`, a default question is synthesized — the child thread is
    /// always created.
    ///
    /// This is the ONLY writer of `realizations`. Either the whole spiral
    /// lands or none of it does (a mid-transaction failure leaves neither the
    /// realization nor its child thread).
    pub fn fix_salt(
        &self,
        thread_id: &str,
        text: &str,
        domain_names: &[String],
        child_question: Option<&str>,
    ) -> Result<Realization, CoreError> {
        let tx = self.conn.unchecked_transaction()?;

        // Parent must exist; the child thread inherits its domain.
        let parent = self.get_thread(thread_id)?;

        let rid = new_id();
        let now = self.now();

        // (a) the immutable realization row — child_thread_id NULL for now.
        self.conn.execute(
            "INSERT INTO realizations (id, text, date, thread_id, child_thread_id, created_at, device_id)
             VALUES (?1, ?2, ?3, ?4, NULL, ?3, ?5)",
            params![rid, text, now, thread_id, self.device_id],
        )?;

        // Link domains (upsert names → ids so the model can speak in names).
        for name in domain_names {
            let domain = self.upsert_domain(name)?;
            self.conn.execute(
                "INSERT OR IGNORE INTO realization_domains (realization_id, domain_id) VALUES (?1, ?2)",
                params![rid, domain.id],
            )?;
        }

        // (b) birth the child thread, referencing the new realization.
        let question = child_question
            .map(str::trim)
            .filter(|q| !q.is_empty())
            .unwrap_or(DEFAULT_CHILD_QUESTION);
        let child = self.open_thread(question, parent.domain_id.as_deref(), Some(&rid))?;

        // (c) close the spiral: back-link the realization to its child.
        self.conn.execute(
            "UPDATE realizations SET child_thread_id = ?1 WHERE id = ?2",
            params![child.id, rid],
        )?;

        // (d) the parent thread is now Fixed.
        self.set_thread_state(thread_id, ThreadState::Fixed)?;

        // (e) kindle the SALT passage (first-wins; no-op if already kindled).
        self.kindle_passage("SALT", Some(&rid))?;

        tx.commit()?;
        self.get_realization(&rid)
    }

    pub fn get_realization(&self, id: &str) -> Result<Realization, CoreError> {
        self.conn
            .query_row(
                &format!("SELECT {REALIZATION_COLS} FROM realizations WHERE id = ?1"),
                params![id],
                realization_from_row,
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    CoreError::NotFound(format!("realization {id}"))
                }
                other => CoreError::Sqlite(other),
            })
    }

    /// The thread this realization gave birth to (the spiral's next question).
    pub fn realization_child_thread(&self, id: &str) -> Result<Thread, CoreError> {
        let child_id: Option<String> = self
            .conn
            .query_row(
                "SELECT child_thread_id FROM realizations WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    CoreError::NotFound(format!("realization {id}"))
                }
                other => CoreError::Sqlite(other),
            })?;
        let child_id = child_id
            .ok_or_else(|| CoreError::NotFound(format!("child thread of realization {id}")))?;
        self.get_thread(&child_id)
    }

    /// Realizations are immutable — there is no update path. This helper
    /// exists to make that guarantee explicit and testable: it never mutates
    /// anything and always returns `Err(Immutable)`.
    pub fn try_mutate_realization(&self, id: &str, _new_text: &str) -> Result<(), CoreError> {
        Err(CoreError::Immutable(format!(
            "realization {id} is immutable; fix_salt is the sole writer"
        )))
    }

    /// Domain ids linked to a realization, in insertion order.
    pub fn realization_domains(&self, id: &str) -> Result<Vec<String>, CoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT domain_id FROM realization_domains WHERE realization_id = ?1 ORDER BY rowid",
        )?;
        let rows = stmt.query_map(params![id], |r| r.get(0))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(CoreError::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parent_thread(store: &Store, domain: Option<&str>) -> Thread {
        store.open_thread("what is entropy?", domain, None).unwrap()
    }

    #[test]
    fn fix_salt_writes_realization_and_births_child_in_one_txn() {
        let store = Store::open_in_memory("d").unwrap();
        let domain = store.upsert_domain("thermodynamics").unwrap();
        let parent = parent_thread(&store, Some(&domain.id));

        let realization = store
            .fix_salt(
                &parent.id,
                "entropy is lost ways-to-not-know",
                &["thermodynamics".to_string()],
                None,
            )
            .unwrap();

        // realization landed, immutable columns only.
        assert_eq!(realization.thread_id, parent.id);
        assert!(realization.child_thread_id.is_some());

        // child thread exists, is volatile, and back-links the realization.
        let child = store.realization_child_thread(&realization.id).unwrap();
        assert_eq!(child.state, ThreadState::Volatile);
        assert_eq!(
            child.parent_realization_id.as_deref(),
            Some(realization.id.as_str())
        );
        // child inherits the parent thread's domain.
        assert_eq!(child.domain_id.as_deref(), Some(domain.id.as_str()));

        // parent thread is now Fixed.
        let reloaded_parent = store.get_thread(&parent.id).unwrap();
        assert_eq!(reloaded_parent.state, ThreadState::Fixed);

        // domain link recorded.
        assert_eq!(
            store.realization_domains(&realization.id).unwrap(),
            vec![domain.id.clone()]
        );

        // SALT kindled.
        assert!(store.kindled().unwrap().contains(&"SALT".to_string()));
    }

    #[test]
    fn fix_salt_synthesizes_child_question_when_absent() {
        let store = Store::open_in_memory("d").unwrap();
        let parent = parent_thread(&store, None);
        let realization = store
            .fix_salt(&parent.id, "a realization", &[], None)
            .unwrap();
        let child = store.realization_child_thread(&realization.id).unwrap();
        assert_eq!(child.prompt, DEFAULT_CHILD_QUESTION);
    }

    #[test]
    fn fix_salt_honors_an_explicit_child_question() {
        let store = Store::open_in_memory("d").unwrap();
        let parent = parent_thread(&store, None);
        let realization = store
            .fix_salt(
                &parent.id,
                "a realization",
                &[],
                Some("what carries the heat?"),
            )
            .unwrap();
        let child = store.realization_child_thread(&realization.id).unwrap();
        assert_eq!(child.prompt, "what carries the heat?");
    }

    #[test]
    fn fix_salt_on_missing_parent_writes_nothing() {
        let store = Store::open_in_memory("d").unwrap();
        let err = store
            .fix_salt("no-such-thread", "orphan", &[], None)
            .unwrap_err();
        assert!(matches!(err, CoreError::NotFound(_)));
        // neither a realization nor a stray child thread was left behind.
        let realization_count: i64 = store
            .conn
            .query_row("SELECT count(*) FROM realizations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(realization_count, 0, "no realization row on rollback");
        let thread_count: i64 = store
            .conn
            .query_row("SELECT count(*) FROM threads", [], |r| r.get(0))
            .unwrap();
        assert_eq!(thread_count, 0, "no child thread on rollback");
    }

    #[test]
    fn realizations_are_immutable() {
        let store = Store::open_in_memory("d").unwrap();
        let parent = parent_thread(&store, None);
        let realization = store.fix_salt(&parent.id, "fixed", &[], None).unwrap();
        assert!(matches!(
            store.try_mutate_realization(&realization.id, "tampered"),
            Err(CoreError::Immutable(_))
        ));
    }
}
