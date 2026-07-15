use rusqlite::Connection;

use crate::error::CoreError;

/// One entry per schema version, applied in order. NEVER edit an existing
/// entry after it has shipped — append a new one.
pub(crate) const MIGRATIONS: &[&str] = &[
    // v1: initial schema. All *_at columns are unix epoch-seconds; every row
    // carries created_at/updated_at/device_id; deletes are tombstones
    // (deleted_at) EXCEPT realizations and tending, which are append-only /
    // immutable (see Task 9's fix_salt, sole writer of realizations).
    r#"
    -- sulfur: domains + the desire-notes that seeded them
    CREATE TABLE domains (
      id TEXT PRIMARY KEY, name TEXT NOT NULL,
      created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, device_id TEXT NOT NULL, deleted_at INTEGER);
    CREATE TABLE pull_notes (
      id TEXT PRIMARY KEY, domain_id TEXT REFERENCES domains(id), text TEXT NOT NULL,
      created_at INTEGER NOT NULL, device_id TEXT NOT NULL);
    -- mercury: threads (open questions). state in {volatile,condensing,fixed,evaporated}
    CREATE TABLE threads (
      id TEXT PRIMARY KEY, prompt TEXT NOT NULL, domain_id TEXT REFERENCES domains(id),
      state TEXT NOT NULL, born INTEGER NOT NULL, last_worked INTEGER,
      parent_realization_id TEXT REFERENCES realizations(id),
      created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, device_id TEXT NOT NULL, deleted_at INTEGER);
    -- salt: realizations (immutable once approved). child_thread_id is the spiral link.
    CREATE TABLE realizations (
      id TEXT PRIMARY KEY, text TEXT NOT NULL, date INTEGER NOT NULL,
      thread_id TEXT NOT NULL REFERENCES threads(id), child_thread_id TEXT REFERENCES threads(id),
      created_at INTEGER NOT NULL, device_id TEXT NOT NULL);   -- no updated_at/deleted_at: immutable
    CREATE TABLE realization_domains (
      realization_id TEXT NOT NULL REFERENCES realizations(id), domain_id TEXT NOT NULL REFERENCES domains(id),
      PRIMARY KEY (realization_id, domain_id));
    -- fire: one row per day tended. append-only; wisdom = count(*)
    CREATE TABLE tending (
      day TEXT PRIMARY KEY,           -- 'YYYY-MM-DD' (UTC) — one row per day
      minutes INTEGER NOT NULL, thread_ids TEXT NOT NULL DEFAULT '[]',   -- JSON array
      created_at INTEGER NOT NULL, device_id TEXT NOT NULL);
    -- learner profile: one row per section (update_memory maintains these)
    CREATE TABLE profile (
      section TEXT PRIMARY KEY,       -- 'domains'|'pulls'|'frictions'|'working_history'|'how_i_learn'
      content TEXT NOT NULL DEFAULT '', updated_at INTEGER NOT NULL);
    -- one-line session traces future sessions read
    CREATE TABLE traces (
      id TEXT PRIMARY KEY, session_id TEXT NOT NULL REFERENCES sessions(id), text TEXT NOT NULL,
      created_at INTEGER NOT NULL, device_id TEXT NOT NULL);
    -- Tabula passage kindling (derived events; first-kindle wins)
    CREATE TABLE kindling (
      passage_key TEXT PRIMARY KEY,   -- 'SALT','NIGREDO','SOLVE','CITRINITAS','AZOTH',...
      first_kindled_at INTEGER NOT NULL, source_id TEXT);   -- id of the datum that kindled it
    -- Azoth's verb (mask deferred; schema ships now)
    CREATE TABLE correspondences (
      id TEXT PRIMARY KEY, domain_a TEXT NOT NULL, domain_b TEXT NOT NULL, note TEXT NOT NULL,
      created_at INTEGER NOT NULL, device_id TEXT NOT NULL);
    -- sessions: the dialogue container + its selected mask/mode
    CREATE TABLE sessions (
      id TEXT PRIMARY KEY, thread_id TEXT REFERENCES threads(id),
      mask TEXT NOT NULL, mode TEXT NOT NULL, state TEXT NOT NULL,   -- open|closed|abandoned
      transcript TEXT NOT NULL DEFAULT '',
      started_at INTEGER NOT NULL, ended_at INTEGER,
      created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, device_id TEXT NOT NULL);
    "#,
    // v2: session notes — the richer, multi-sentence distillation the
    // Mystagogue writes when a session CONDENSES on close (what moved, what
    // opened, what the learner circled). Distinct from `traces` (the one-line
    // "last time" memory the next session's prompt injects): notes are the
    // durable residue shown in session history + a thread's detail, carry the
    // session's focal thread for that thread-scoped read, and are append-only
    // like realizations/tending (a session's settled note is never rewritten).
    r#"
    CREATE TABLE session_notes (
      id TEXT PRIMARY KEY,
      session_id TEXT NOT NULL REFERENCES sessions(id),
      thread_id TEXT REFERENCES threads(id),
      note TEXT NOT NULL,
      created_at INTEGER NOT NULL, device_id TEXT NOT NULL);
    "#,
];
// NOTE: threads.parent_realization_id and realizations.child_thread_id are a
// deliberate FK cycle — SQLite does not enforce FK order within a CREATE, and
// foreign_keys=ON checks at row-write time, so insert a thread first (child),
// then the realization referencing it, then set the child's
// parent_realization_id — all inside one transaction (Task 9 fix_salt).
//
// NOTE: the schema defines no indices. This is an on-device single-user DB
// (no concurrent readers, dataset sized to one person's lifetime of
// threads/realizations), so `ripe_threads`'s scan of threads.state +
// last_worked (and similar small-table scans elsewhere) is an accepted
// tradeoff rather than an oversight — see ripe_threads in threads.rs.

pub(crate) fn migrate(conn: &Connection) -> Result<(), CoreError> {
    migrate_with(conn, MIGRATIONS)
}

/// Applies pending migrations from `migrations`. Each one is all-or-nothing:
/// the DDL and the `user_version` bump commit in a single transaction, so a
/// mid-batch failure rolls back cleanly instead of leaving partial tables
/// behind with a stale version.
fn migrate_with(conn: &Connection, migrations: &[&str]) -> Result<(), CoreError> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    for (i, sql) in migrations.iter().enumerate().skip(version as usize) {
        let result = conn.execute_batch(&format!(
            "BEGIN;\n{}\nPRAGMA user_version = {};\nCOMMIT;",
            sql,
            i + 1
        ));
        if let Err(e) = result {
            // A mid-batch failure leaves the explicit BEGIN open on the
            // connection; roll it back so the connection stays usable.
            if !conn.is_autocommit() {
                conn.execute_batch("ROLLBACK;")?;
            }
            return Err(e.into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_tables_exist_after_migrate() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN
                 ('domains','pull_notes','threads','realizations','realization_domains',
                  'tending','profile','traces','kindling','correspondences','sessions',
                  'session_notes')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 12, "all v1 + v2 tables created");
    }

    #[test]
    fn session_notes_table_exists_after_migrate() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version as usize, MIGRATIONS.len(), "migrated to v2");
        let cols: i64 = conn
            .query_row(
                "SELECT count(*) FROM pragma_table_info('session_notes')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cols, 6, "session_notes has its six columns");
    }

    #[test]
    fn failed_migration_rolls_back_cleanly() {
        let conn = Connection::open_in_memory().unwrap();
        let broken: &[&str] = &[MIGRATIONS[0], "CREATE TABLE broken (;"];
        assert!(migrate_with(&conn, broken).is_err());
        let v: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(v, 1, "v1 committed, broken v2 rolled back");
    }
}
