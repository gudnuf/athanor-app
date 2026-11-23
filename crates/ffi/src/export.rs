//! Store-only vault export (relight v1.0.7): works with no key and no engine —
//! the rescue path must never depend on LLM credentials.
use athanor_core::Store;

use crate::engine::EngineError;

/// The whole vault (vault v1 markdown) of the store file at `db_path`, opened read-only.
#[uniffi::export]
pub fn export_vault_at(db_path: String) -> Result<String, EngineError> {
    Store::open_read_only(&db_path, "export")
        .and_then(|s| s.export_vault())
        .map_err(|e| EngineError::Read(e.to_string()))
}

/// A consistent copy of the store file at `db_path`, written to `dest_path` (must not exist).
#[uniffi::export]
pub fn export_database_copy_at(db_path: String, dest_path: String) -> Result<(), EngineError> {
    Store::open_read_only(&db_path, "export")
        .and_then(|s| s.export_database_copy(&dest_path))
        .map_err(|e| EngineError::Read(e.to_string()))
}
