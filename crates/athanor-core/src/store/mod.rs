//! Single-writer storage API. ALL mutations flow through `Store` methods so a
//! change-log/sync layer can be inserted later without touching callers. Rows
//! carry created_at/updated_at/device_id; deletes are tombstones, except
//! `realizations` and `tending` which are append-only/immutable (see Task 9's
//! `fix_salt`, the sole writer of `realizations`).

pub(crate) mod migrations;

mod correspondences;
mod domains;
mod kindling;
mod profile;
mod realizations;
mod session_notes;
mod sessions;
mod tending;
mod threads;
mod traces;

use std::path::Path;
use std::sync::Arc;

use parking_lot::{ReentrantMutex, ReentrantMutexGuard};
use rusqlite::Connection;

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
