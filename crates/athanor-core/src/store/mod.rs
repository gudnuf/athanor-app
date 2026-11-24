//! Single-writer storage API. ALL mutations flow through `Store` methods so a
//! change-log/sync layer can be inserted later without touching callers. Rows
//! carry created_at/updated_at/device_id; deletes are tombstones, except
//! `realizations` and `tending` which are append-only/immutable (see Task 9's
//! `fix_salt`, the sole writer of `realizations`).

pub(crate) mod migrations;

mod correspondences;
mod domains;
mod export;
mod kindling;
mod profile;
mod realizations;
mod recall;
mod session_notes;
mod session_threads;
mod sessions;
mod tending;
mod threads;
mod traces;

use std::path::Path;
use std::sync::Arc;

use parking_lot::{ReentrantMutex, ReentrantMutexGuard};
use rusqlite::{Connection, OpenFlags};

use crate::error::CoreError;

pub type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;

// epoch-seconds
pub(crate) fn system_clock() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub struct Store {
    // The rusqlite `Connection` is `Send` but `!Sync`. Wrapping it in a
    // `ReentrantMutex` makes `Store` (and thus `Arc<Store>`) `Send + Sync`, so
    // the `Mystagogue` tool dispatcher can satisfy the engine seam's
    // `ToolDispatch: Send + Sync` contract without weakening that contract for
    // the real engine lane. Reentrant (not plain `Mutex`) because `fix_salt`
    // holds one transaction while calling other `Store` methods that re-lock —
    // a non-reentrant mutex would self-deadlock there. Single-writer on-device
    // app: lock contention is negligible. All connection access goes through
    // `conn()`; every rusqlite method used here takes `&self`, so the shared
    // `&Connection` a reentrant guard yields is sufficient.
    conn: ReentrantMutex<Connection>,
    pub(crate) device_id: String,
    clock: Clock,
    /// `Some(found)` for a read-only open (the file's `user_version`); `None`
    /// for a normal open. Lets export migrate a temp copy of an older file.
    pub(crate) read_only_version: Option<i64>,
}

impl Store {
    pub fn open(path: impl AsRef<Path>, device_id: impl Into<String>) -> Result<Self, CoreError> {
        Self::from_connection(Connection::open(path)?, device_id)
    }

    pub fn open_in_memory(device_id: impl Into<String>) -> Result<Self, CoreError> {
        Self::from_connection(Connection::open_in_memory()?, device_id)
    }

    fn from_connection(conn: Connection, device_id: impl Into<String>) -> Result<Self, CoreError> {
        conn.pragma_update(None, "foreign_keys", true)?;
        migrations::migrate(&conn)?;
        Ok(Store {
            conn: ReentrantMutex::new(conn),
            device_id: device_id.into(),
            clock: Arc::new(system_clock),
            read_only_version: None,
        })
    }

    /// Opens an EXISTING store file read-only for export. Never migrates, never
    /// creates. Errors: missing file → NotFound; `user_version` > MIGRATIONS.len()
    /// (a file from a NEWER build) → BadState. An OLDER file opens fine:
    /// `export_vault` renders it by migrating a temp copy, and
    /// `export_database_copy` keeps it byte-honest at its own version. Sets busy_timeout(2s) so a read during the live engine's
    /// short write transaction waits instead of failing.
    pub fn open_read_only(
        path: impl AsRef<Path>,
        device_id: impl Into<String>,
    ) -> Result<Self, CoreError> {
        let path = path.as_ref();
        if !path.exists() {
            return Err(CoreError::NotFound(format!(
                "store file {}",
                path.display()
            )));
        }
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.busy_timeout(std::time::Duration::from_secs(2))?;
        let found: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        let expected = migrations::MIGRATIONS.len();
        if found < 0 || found as usize > expected {
            return Err(CoreError::BadState(format!(
                "schema v{found} is newer than this build (v{expected}) — update the app to export it"
            )));
        }
        Ok(Store {
            conn: ReentrantMutex::new(conn),
            device_id: device_id.into(),
            clock: Arc::new(system_clock),
            read_only_version: Some(found),
        })
    }

    /// Replaces the clock (tests inject deterministic time).
    pub fn with_clock(mut self, clock: Clock) -> Self {
        self.clock = clock;
        self
    }

    /// Locks and yields the connection. Reentrant: safe to call again while a
    /// guard from an enclosing call is still held (e.g. `fix_salt`'s
    /// transaction calling other `Store` methods).
    pub(crate) fn conn(&self) -> ReentrantMutexGuard<'_, Connection> {
        self.conn.lock()
    }

    pub(crate) fn now(&self) -> u64 {
        (self.clock)()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_in_memory_migrates_to_latest() {
        let store = Store::open_in_memory("device-a").unwrap();
        let version: i64 = store
            .conn()
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version as usize, migrations::MIGRATIONS.len());
    }

    #[test]
    fn reopen_is_idempotent() {
        let dir = std::env::temp_dir().join(format!("athanor-core-test-{}", crate::ids::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("athanor.db");
        {
            Store::open(&path, "device-a").unwrap();
        }
        let store = Store::open(&path, "device-a").unwrap();
        let version: i64 = store
            .conn()
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version as usize, migrations::MIGRATIONS.len());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn open_read_only_accepts_an_older_schema() {
        let dir = std::env::temp_dir().join(format!("athanor-core-test-{}", crate::ids::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("old.db");
        {
            let c = Connection::open(&path).unwrap();
            migrations::migrate_with(&c, &migrations::MIGRATIONS[..1]).unwrap();
        }
        let ro = Store::open_read_only(&path, "d").unwrap();
        assert_eq!(ro.read_only_version, Some(1));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn open_read_only_refuses_a_newer_schema() {
        let dir = std::env::temp_dir().join(format!("athanor-core-test-{}", crate::ids::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("new.db");
        {
            let c = Connection::open(&path).unwrap();
            c.pragma_update(
                None,
                "user_version",
                (migrations::MIGRATIONS.len() + 1) as i64,
            )
            .unwrap();
        }
        assert!(matches!(
            Store::open_read_only(&path, "d"),
            Err(CoreError::BadState(_))
        ));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn injected_clock_drives_now() {
        let store = Store::open_in_memory("device-a")
            .unwrap()
            .with_clock(Arc::new(|| 12345));
        assert_eq!(store.now(), 12345);
    }

    #[test]
    fn foreign_keys_are_enforced() {
        let store = Store::open_in_memory("device-a").unwrap();
        // threads.domain_id is nullable — a thread with no domain is fine.
        let result = store.conn().execute(
            "INSERT INTO threads (id, prompt, state, born, created_at, updated_at, device_id)
             VALUES ('t1', 'why?', 'volatile', 1, 1, 1, 'd')",
            [],
        );
        assert!(result.is_ok(), "null domain_id is fine");
        // dangling domain_id must be rejected.
        let result = store.conn().execute(
            "INSERT INTO threads (id, prompt, domain_id, state, born, created_at, updated_at, device_id)
             VALUES ('t2', 'why?', 'no-such-domain', 'volatile', 1, 1, 1, 'd')",
            [],
        );
        assert!(result.is_err(), "dangling domain_id must be rejected");
    }
}
