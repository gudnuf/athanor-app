//! Salt: realizations. Immutable once written (no updated_at/deleted_at).
//!
//! `fix_salt` — the sole writer of this table, and the enforcer of the
//! thread↔realization spiral invariant (insert child thread, then the
//! realization referencing it, then back-link the parent's
//! `parent_realization_id`, all inside one `unchecked_transaction`) — lands
//! in Task 9. This module is intentionally empty until then: Task 6 only
//! ships the schema (see migrations.rs) and the CRUD contract for the other
//! seven tables.
