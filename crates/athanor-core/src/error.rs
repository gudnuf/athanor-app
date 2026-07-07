use thiserror::Error;

/// Errors from the athanor-core domain layer.
#[derive(Debug, Error)]
pub enum CoreError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("immutable: {0}")]
    Immutable(String),
    #[error("bad state: {0}")]
    BadState(String),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}
