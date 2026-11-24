//! Session <-> thread links (schema v3). The writers land in a later task; this
//! file starts with the read the export needs.

use rusqlite::params;

use crate::error::CoreError;

use super::Store;

impl Store {
    /// A session's links as `(thread_id, role)`, in a stable order — the
    /// export read.
    pub(crate) fn session_links(
        &self,
        session_id: &str,
    ) -> Result<Vec<(String, String)>, CoreError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT thread_id, role FROM session_threads WHERE session_id = ?1
             ORDER BY created_at ASC, role ASC, thread_id ASC",
        )?;
        let rows = stmt.query_map(params![session_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(CoreError::from)
    }
}
