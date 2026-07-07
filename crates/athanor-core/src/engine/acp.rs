//! ACP-shaped seam types.
//!
//! These are OUR structs, not the `agent-client-protocol` crate's. They
//! mirror ACP wire shapes closely enough that `engine/goose.rs` can convert
//! to/from the real goose/ACP types at the boundary, but no other module in
//! this codebase should ever name a goose or `agent-client-protocol` type
//! directly — that isolation is the entire point of this seam. When the
//! engine underneath churns (goose upgrade, or the goosed-on-tailnet RED
//! path), only `engine/goose.rs` (or its replacement) needs to change.

use serde_json::Value;

/// One turn's worth of input to the engine: system prompt, the user-visible
/// turns so far, and the tools available for this turn.
#[derive(Debug, Clone, PartialEq)]
pub struct AcpPrompt {
    pub system: String,
    pub user_turns: Vec<String>,
    pub tools: Vec<AcpToolSpec>,
}

/// A tool made available to the engine for this turn.
#[derive(Debug, Clone, PartialEq)]
pub struct AcpToolSpec {
    pub name: String,
    pub json_schema: Value,
}

/// A single streamed update from the engine while it drives a turn.
#[derive(Debug, Clone, PartialEq)]
pub enum AcpUpdate {
    TextDelta(String),
    ToolCall(AcpToolCall),
    TurnComplete,
}

/// A tool invocation requested by the engine, to be resolved via
/// `ToolDispatch`.
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
