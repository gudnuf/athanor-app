//! ACP-shaped seam types — LOCAL MIRROR of Task 8's `engine/acp.rs`.
//!
//! ============================ INTEGRATION NOTE ============================
//! These structs and the `ToolDispatch` trait are OWNED by Task 8 (the engine
//! seam, on `origin/task8-engine`: `engine/acp.rs` + `engine/mod.rs`). Task 9
//! was built off `origin/main`, which has only the Task 6 store — so this file
//! carries a byte-compatible mirror of that contract so the Mystagogue can
//! compile and be tested in isolation.
//!
//! When the engine lane is integrated, DELETE this file and repoint the two
//! `use super::acp::…` imports in `mystagogue/mod.rs` at `crate::engine::…`.
//! The shapes here are identical to Task 8's, so no other change is needed.
//! =========================================================================

use serde_json::Value;

/// A tool made available to the engine for a turn (name + JSON schema).
#[derive(Debug, Clone, PartialEq)]
pub struct AcpToolSpec {
    pub name: String,
    pub json_schema: Value,
}

/// A tool invocation requested by the engine, resolved via `ToolDispatch`.
#[derive(Debug, Clone, PartialEq)]
pub struct AcpToolCall {
    pub id: String,
    pub name: String,
    pub args: Value,
}

/// The result of dispatching a tool call, handed back to the engine.
#[derive(Debug, Clone, PartialEq)]
pub struct AcpToolResult {
    pub id: String,
    pub value: Value,
}

/// Resolves a tool call requested mid-turn by the engine. `Mystagogue`
/// implements this over the `Store`.
///
/// ============================ INTEGRATION NOTE ============================
/// Task 8's `engine::ToolDispatch` is declared `Send + Sync` with a `Send`
/// dispatch future. `Mystagogue` cannot satisfy that: it holds an
/// `Arc<Store>`, and `Store` wraps a single rusqlite `Connection`, which is
/// `!Sync` — so `Arc<Store>` is neither `Send` nor `Sync`. The plan's own
/// `Mystagogue::store(&self) -> &Store` accessor presupposes exactly this
/// plain-`Arc<Store>` shape (a `Mutex<Store>` would make that accessor
/// impossible). An on-device engine turn is single-threaded, so this mirror
/// drops the `Send + Sync` bounds and uses `?Send`. At integration, either
/// relax `engine::ToolDispatch` to `?Send` (recommended — the turn is
/// single-threaded) or wrap the `Store` in a `Mutex` and change the accessor.
/// =========================================================================
#[async_trait::async_trait(?Send)]
pub trait ToolDispatch {
    async fn dispatch(&self, call: AcpToolCall) -> AcpToolResult;
}
