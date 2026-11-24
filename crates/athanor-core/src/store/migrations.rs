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
    // v3: session <-> thread links, measured minutes, tended day. The links are
    // what the cold room reads (which domains a session touched). The backfill
    // corrects placeholders only: it sets `tending.minutes` where it is 0 and
    // `tending.thread_ids` where it is '[]'; it never inserts or deletes a
    // tending day and never touches created_at (invariant #5: fire is
    // append-only). `minutes`/`tended_day` are filtered on the learner marker
    // (transcript.rs `◇ LEARNER`) so ghost sessions are not credited.
    r#"
    ALTER TABLE sessions ADD COLUMN minutes INTEGER;
    ALTER TABLE sessions ADD COLUMN tended_day TEXT;
    CREATE TABLE session_threads (
      session_id TEXT NOT NULL REFERENCES sessions(id),
      thread_id  TEXT NOT NULL REFERENCES threads(id),
      role TEXT NOT NULL,          -- focal|worked|opened|fixed|born|evaporated
      created_at INTEGER NOT NULL,
      PRIMARY KEY (session_id, thread_id, role));
    UPDATE sessions SET minutes = MAX(1, MIN(60, (ended_at - started_at + 59) / 60))
      WHERE state = 'closed' AND ended_at IS NOT NULL AND transcript LIKE '%◇ LEARNER%';
    UPDATE sessions SET tended_day = strftime('%Y-%m-%d', ended_at, 'unixepoch')
      WHERE state = 'closed' AND ended_at IS NOT NULL AND transcript LIKE '%◇ LEARNER%'
        AND strftime('%Y-%m-%d', ended_at, 'unixepoch') IN (SELECT day FROM tending);
    INSERT OR IGNORE INTO session_threads (session_id, thread_id, role, created_at)
      SELECT id, thread_id, 'focal', started_at FROM sessions WHERE thread_id IS NOT NULL;
    INSERT OR IGNORE INTO session_threads (session_id, thread_id, role, created_at)
      SELECT sid, child_thread_id, 'born', created_at FROM (
        SELECT r.child_thread_id, r.created_at,
          (SELECT s.id FROM sessions s WHERE s.state = 'closed'
             AND r.created_at BETWEEN s.started_at AND s.ended_at
             ORDER BY s.started_at DESC LIMIT 1) AS sid
        FROM realizations r WHERE r.child_thread_id IS NOT NULL) WHERE sid IS NOT NULL;
    INSERT OR IGNORE INTO session_threads (session_id, thread_id, role, created_at)
      SELECT sid, id, 'opened', born FROM (
        SELECT t.id, t.born,
          (SELECT s.id FROM sessions s WHERE s.state = 'closed'
             AND t.born BETWEEN s.started_at AND s.ended_at
             ORDER BY s.started_at DESC LIMIT 1) AS sid
        FROM threads t WHERE t.parent_realization_id IS NULL) WHERE sid IS NOT NULL;
    UPDATE tending SET minutes = (SELECT COALESCE(SUM(minutes), 0) FROM sessions WHERE tended_day = tending.day)
      WHERE minutes = 0;
    UPDATE tending SET thread_ids = (
        SELECT json_group_array(DISTINCT st.thread_id) FROM session_threads st
        JOIN sessions s ON s.id = st.session_id WHERE s.tended_day = tending.day)
      WHERE thread_ids = '[]'
        AND EXISTS (SELECT 1 FROM session_threads st JOIN sessions s ON s.id = st.session_id WHERE s.tended_day = tending.day);
    -- born children are excluded, matching the going-forward rule
    -- (mark_session_threads_worked skips born/evaporated links).
    UPDATE threads SET last_worked = (
        SELECT MAX(s.ended_at) FROM session_threads st JOIN sessions s ON s.id = st.session_id
        WHERE st.thread_id = threads.id AND s.state = 'closed' AND st.role <> 'born')
      WHERE last_worked IS NULL;
    UPDATE threads SET domain_id = (
        SELECT rd.domain_id FROM realization_domains rd
        WHERE rd.realization_id = threads.parent_realization_id ORDER BY rd.rowid LIMIT 1)
      WHERE domain_id IS NULL AND parent_realization_id IS NOT NULL;
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
pub(crate) fn migrate_with(conn: &Connection, migrations: &[&str]) -> Result<(), CoreError> {
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
                  'session_notes','session_threads')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 13, "all v1 + v2 + v3 tables created");
    }

    fn user_version(conn: &Connection) -> i64 {
        conn.pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn v3_adds_session_threads_and_session_columns() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        assert_eq!(user_version(&conn), 3);
        let cols = |t: &str| -> i64 {
            conn.query_row(
                &format!("SELECT count(*) FROM pragma_table_info('{t}')"),
                [],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(cols("session_threads"), 4);
        let has = |c: &str| -> i64 {
            conn.query_row(
                "SELECT count(*) FROM pragma_table_info('sessions') WHERE name = ?1",
                [c],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(has("minutes"), 1);
        assert_eq!(has("tended_day"), 1);
    }

    #[test]
    fn v3_learner_marker_matches_transcript_module() {
        let marker = crate::transcript::TranscriptRole::Learner.marker();
        assert!(
            MIGRATIONS[2].contains(&format!("'%{marker}%'")),
            "the migration's LIKE literal must equal transcript.rs's marker"
        );
    }

    #[test]
    fn v3_backfills_from_v2_data() {
        let conn = Connection::open_in_memory().unwrap();
        migrate_with(&conn, &MIGRATIONS[..2]).unwrap();
        let learner =
            crate::transcript::format_block(crate::transcript::TranscriptRole::Learner, "hi");
        // The seed has a parent<->child FK cycle; load it with FKs off, then
        // migrate with them ON (as the real Store does).
        conn.pragma_update(None, "foreign_keys", false).unwrap();
        let x = |sql: &str| conn.execute(sql, []).unwrap();
        x("INSERT INTO domains VALUES ('D1','one',1,1,'d',NULL), ('D2','two',1,1,'d',NULL)");
        x("INSERT INTO threads (id,prompt,domain_id,state,born,last_worked,parent_realization_id,created_at,updated_at,device_id)
           VALUES ('T0','parent?',NULL,'fixed',100,NULL,NULL,100,100,'d'),
                  ('C','child?',NULL,'volatile',1300,NULL,'R',1300,1300,'d'),
                  ('O','opened in session?',NULL,'volatile',1200,NULL,NULL,1200,1200,'d')");
        x("INSERT INTO realizations VALUES ('R','a realization',1300,'T0','C',1300,'d')");
        x("INSERT INTO realization_domains VALUES ('R','D2'), ('R','D1')");
        conn.execute(
            "INSERT INTO sessions (id,thread_id,mask,mode,state,transcript,started_at,ended_at,created_at,updated_at,device_id)
             VALUES ('A',NULL,'m','x','closed',?1,1000,1600,1000,1600,'d')",
            [&learner],
        )
        .unwrap();
        x("INSERT INTO sessions (id,thread_id,mask,mode,state,transcript,started_at,ended_at,created_at,updated_at,device_id)
           VALUES ('G',NULL,'m','x','closed','',5000,5010,5000,5010,'d'),
                  ('X',NULL,'m','x','abandoned','',1500,1700,1500,1700,'d')");
        x("INSERT INTO tending VALUES ('1970-01-01',0,'[]',1600,'d')");

        conn.pragma_update(None, "foreign_keys", true).unwrap();
        migrate(&conn).unwrap();

        let q = |sql: &str| -> Option<i64> { conn.query_row(sql, [], |r| r.get(0)).unwrap() };
        assert_eq!(q("SELECT minutes FROM sessions WHERE id='A'"), Some(10));
        let day: Option<String> = conn
            .query_row("SELECT tended_day FROM sessions WHERE id='A'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(day.as_deref(), Some("1970-01-01"));
        assert_eq!(
            q("SELECT minutes FROM sessions WHERE id='G'"),
            None,
            "ghost uncredited"
        );
        let gday: Option<String> = conn
            .query_row("SELECT tended_day FROM sessions WHERE id='G'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(gday, None);
        assert_eq!(
            q("SELECT count(*) FROM session_threads WHERE session_id='X'"),
            Some(0)
        );
        assert_eq!(
            q("SELECT minutes FROM tending WHERE day='1970-01-01'"),
            Some(10)
        );
        let ids: String = conn
            .query_row("SELECT thread_ids FROM tending", [], |r| r.get(0))
            .unwrap();
        let mut ids: Vec<String> = serde_json::from_str(&ids).unwrap();
        ids.sort();
        assert_eq!(ids, vec!["C".to_string(), "O".to_string()]);
        let mut stmt = conn
            .prepare("SELECT session_id, thread_id, role FROM session_threads ORDER BY thread_id")
            .unwrap();
        let links: Vec<(String, String, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(
            links,
            vec![
                ("A".into(), "C".into(), "born".into()),
                ("A".into(), "O".into(), "opened".into())
            ]
        );
        let dom: Option<String> = conn
            .query_row("SELECT domain_id FROM threads WHERE id='C'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(dom.as_deref(), Some("D2"), "first-inserted domain wins");
        let dom0: Option<String> = conn
            .query_row("SELECT domain_id FROM threads WHERE id='T0'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(dom0, None);
        assert_eq!(
            q("SELECT last_worked FROM threads WHERE id='O'"),
            Some(1600)
        );
        assert_eq!(
            q("SELECT last_worked FROM threads WHERE id='C'"),
            None,
            "born children are not 'worked' (same rule as going forward)"
        );
        assert_eq!(
            q("SELECT count(*) FROM tending"),
            Some(1),
            "no day added or removed"
        );
    }

    #[test]
    fn session_notes_table_exists_after_migrate() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version as usize, MIGRATIONS.len(), "migrated to latest");
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
