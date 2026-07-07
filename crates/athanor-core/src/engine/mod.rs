//! The engine seam: the ONE boundary between `athanor-core` and whatever
//! drives the model (embedded goose, `goosed` over the tailnet, or a
//! hermetic `MockEngine` in tests). Every caller in this crate talks to
//! `dyn MystagogueEngine` + the ACP-shaped types in `acp.rs`; nothing else
//! names a goose type.

mod acp;
mod mock;

#[cfg(feature = "goose")]
mod goose;

pub use acp::{
    AcpPrompt, AcpRole, AcpToolCall, AcpToolResult, AcpToolSpec, AcpTurn, AcpUpdate, Register,
};
pub use mock::MockEngine;

#[cfg(feature = "goose")]
pub use goose::GooseEngine;

use async_trait::async_trait;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("engine error: {0}")]
    Other(String),
}

/// Drives one prompt through whatever model backs the engine, streaming
/// `AcpUpdate`s to `sink` and resolving tool calls via `tools`.
#[async_trait]
pub trait MystagogueEngine: Send + Sync {
    async fn run_turn(
        &self,
        prompt: AcpPrompt,
        tools: &dyn ToolDispatch,
        sink: &mut (dyn FnMut(AcpUpdate) + Send),
    ) -> Result<(), EngineError>;
}

/// Resolves a tool call requested mid-turn by the engine. Task 9's
/// `Mystagogue` implements this over the `Store`.
#[async_trait]
pub trait ToolDispatch: Send + Sync {
    async fn dispatch(&self, call: AcpToolCall) -> AcpToolResult;
}
