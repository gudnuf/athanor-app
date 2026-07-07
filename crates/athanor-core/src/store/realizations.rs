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

use crate::domain::{GrimoireEntry, Realization, Thread, ThreadState};
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
        // Hold the reentrant guard for the whole transaction. The nested `Store`
        // calls below (get_thread, upsert_domain, open_thread, set_thread_state,
        // kindle_passage) and the `self.conn()` statements re-lock reentrantly on
        // this same thread and connection, so they participate in this one
        // transaction. Binding the guard (rather than a temporary from
        // `self.conn().unchecked_transaction()`) keeps `tx`'s borrow of the
        // connection alive until `commit`; a plain non-reentrant `Mutex` would
        // deadlock on the first nested lock.
        let conn = self.conn();
        let tx = conn.unchecked_transaction()?;

        // Parent must exist; the child thread inherits its domain.
        let parent = self.get_thread(thread_id)?;

        // No salt on a dead or already-settled thread: a Fixed thread has
        // already yielded its realization, and an Evaporated one was let go.
        // Guard early so a doomed transaction never synthesizes a child.
        if matches!(parent.state, ThreadState::Fixed | ThreadState::Evaporated) {
            return Err(CoreError::BadState(format!(
                "cannot fix salt on a {} thread {thread_id}",
                parent.state.as_str()
            )));
        }

        let rid = new_id();
        let now = self.now();

        // (a) the immutable realization row — child_thread_id NULL for now.
        self.conn().execute(
            "INSERT INTO realizations (id, text, date, thread_id, child_thread_id, created_at, device_id)
             VALUES (?1, ?2, ?3, ?4, NULL, ?3, ?5)",
            params![rid, text, now, thread_id, self.device_id],
        )?;

        // Link domains (upsert names → ids so the model can speak in names).
        for name in domain_names {
            let domain = self.upsert_domain(name)?;
            self.conn().execute(
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
        self.conn().execute(
            "UPDATE realizations SET child_thread_id = ?1 WHERE id = ?2",
            params![child.id, rid],
        )?;

        // (d) condense-then-fix the parent. The thread-state DAG (session.rs)
        // forbids a direct Volatile -> Fixed hop, so a still-volatile thread
        // takes the legal two-step path Volatile -> Condensing -> Fixed; a
        // thread already Condensing goes straight to Fixed. Both hops commit
        // inside THIS one transaction — the condensation moment happened in the
        // conversation that led here, and fix_salt is that moment made durable.
        if parent.state == ThreadState::Volatile {
            self.set_thread_state(thread_id, ThreadState::Condensing)?;
        }
        self.set_thread_state(thread_id, ThreadState::Fixed)?;

        // (e) kindle the SALT passage (first-wins; no-op if already kindled).
        self.kindle_passage("SALT", Some(&rid))?;

        tx.commit()?;
        self.get_realization(&rid)
    }

    pub fn get_realization(&self, id: &str) -> Result<Realization, CoreError> {
        self.conn()
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
            .conn()
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
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT domain_id FROM realization_domains WHERE realization_id = ?1 ORDER BY rowid",
        )?;
        let rows = stmt.query_map(params![id], |r| r.get(0))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(CoreError::from)
    }

    /// Domain *names* linked to a realization, in insertion order (join over
    /// `realization_domains` -> `domains`). Read-only helper for
    /// `list_realizations`.
    fn realization_domain_names(&self, id: &str) -> Result<Vec<String>, CoreError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT d.name FROM realization_domains rd
             JOIN domains d ON d.id = rd.domain_id
             WHERE rd.realization_id = ?1 ORDER BY rd.rowid",
        )?;
        let rows = stmt.query_map(params![id], |r| r.get(0))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(CoreError::from)
    }

    /// The Grimoire: every realization, chronological (`date` ASC, then
    /// `created_at` ASC to break same-day ties), each carrying its linked
    /// domain names and its (already-present) `child_thread_id` spiral link.
    /// Read-only projection — realizations are append-only/immutable, so this
    /// never races a writer.
    pub fn list_realizations(&self) -> Result<Vec<GrimoireEntry>, CoreError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {REALIZATION_COLS} FROM realizations ORDER BY date ASC, created_at ASC"
        ))?;
        let rows = stmt.query_map([], realization_from_row)?;
        let realizations = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(CoreError::from)?;
        realizations
            .into_iter()
            .map(|realization| {
                let domains = self.realization_domain_names(&realization.id)?;
                Ok(GrimoireEntry {
                    realization,
                    domains,
                })
            })
            .collect::<Result<Vec<_>, CoreError>>()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    use super::*;
    use crate::store::Clock;

    fn parent_thread(store: &Store, domain: Option<&str>) -> Thread {
        store.open_thread("what is entropy?", domain, None).unwrap()
    }

    /// A clock that advances by one second on every call, so successive
    /// `fix_salt`s land at strictly increasing `date`s without needing a
    /// fresh store per call.
    fn ticking_clock(start: u64) -> Clock {
        let counter = AtomicU64::new(start);
        Arc::new(move || counter.fetch_add(1, Ordering::SeqCst))
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

        // parent thread is now Fixed — a Volatile parent is condensed then
        // fixed inside fix_salt's single transaction (the DAG forbids a direct
        // Volatile -> Fixed hop), so the observable end state is Fixed.
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
            .conn()
            .query_row("SELECT count(*) FROM realizations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(realization_count, 0, "no realization row on rollback");
        let thread_count: i64 = store
            .conn()
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

    #[test]
    fn fix_salt_on_condensing_parent_goes_straight_to_fixed() {
        let store = Store::open_in_memory("d").unwrap();
        let parent = parent_thread(&store, None);
        // Already mid-condensation: fix_salt makes the single legal hop to Fixed.
        store
            .set_thread_state(&parent.id, ThreadState::Condensing)
            .unwrap();
        let realization = store.fix_salt(&parent.id, "settled", &[], None).unwrap();
        assert!(realization.child_thread_id.is_some());
        assert_eq!(
            store.get_thread(&parent.id).unwrap().state,
            ThreadState::Fixed
        );
    }

    #[test]
    fn fix_salt_rejects_a_fixed_parent() {
        let store = Store::open_in_memory("d").unwrap();
        let parent = parent_thread(&store, None);
        store.fix_salt(&parent.id, "first", &[], None).unwrap();
        // The parent is now Fixed; a second fix_salt has no salt to draw.
        let err = store.fix_salt(&parent.id, "again", &[], None).unwrap_err();
        assert!(matches!(err, CoreError::BadState(_)));
        // exactly one realization, and no orphaned child from the rejected call.
        let realization_count: i64 = store
            .conn()
            .query_row("SELECT count(*) FROM realizations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(realization_count, 1, "the rejected call wrote nothing");
    }

    #[test]
    fn fix_salt_rejects_an_evaporated_parent() {
        let store = Store::open_in_memory("d").unwrap();
        let parent = parent_thread(&store, None);
        store.evaporate_thread(&parent.id).unwrap();
        let err = store
            .fix_salt(&parent.id, "from the void", &[], None)
            .unwrap_err();
        assert!(matches!(err, CoreError::BadState(_)));
        let realization_count: i64 = store
            .conn()
            .query_row("SELECT count(*) FROM realizations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            realization_count, 0,
            "no realization on an evaporated thread"
        );
    }

    #[test]
    fn list_realizations_is_chronological_with_domains_and_child_link() {
        let store = Store::open_in_memory("d")
            .unwrap()
            .with_clock(ticking_clock(1_000));
        let t1 = parent_thread(&store, None);
        let r1 = store
            .fix_salt(&t1.id, "first", &["Alpha".to_string()], None)
            .unwrap();
        let t2 = parent_thread(&store, None);
        let r2 = store
            .fix_salt(&t2.id, "second", &["Beta".to_string()], None)
            .unwrap();

        let entries = store.list_realizations().unwrap();
        assert_eq!(entries.len(), 2, "both realizations landed");
        assert_eq!(entries[0].realization.id, r1.id, "earlier date sorts first");
        assert_eq!(entries[1].realization.id, r2.id);
        assert_eq!(entries[0].domains, vec!["Alpha".to_string()]);
        assert_eq!(entries[1].domains, vec!["Beta".to_string()]);
        assert!(
            entries[0].realization.child_thread_id.is_some(),
            "the spiral link is exposed on the read"
        );
    }

    #[test]
    fn list_realizations_multiple_domains_preserve_insertion_order() {
        let store = Store::open_in_memory("d").unwrap();
        let parent = parent_thread(&store, None);
        let realization = store
            .fix_salt(
                &parent.id,
                "cross-domain",
                &["Zeta".to_string(), "Alpha".to_string()],
                None,
            )
            .unwrap();
        let entries = store.list_realizations().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].realization.id, realization.id);
        assert_eq!(
            entries[0].domains,
            vec!["Zeta".to_string(), "Alpha".to_string()],
            "insertion order, not alphabetical"
        );
    }

    #[test]
    fn list_realizations_empty_store_is_empty() {
        let store = Store::open_in_memory("d").unwrap();
        assert!(store.list_realizations().unwrap().is_empty());
    }
}
