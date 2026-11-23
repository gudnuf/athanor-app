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

/// The Anthropic model the Mystagogue drives in shipped builds. Core-owned
/// (invariant #1): the shell passes `None` and core decides. Changing this is
/// a deliberate, reviewed edit — see docs/plans/2026-11-22-relight-v1.0.7.md
/// Task 2 for the goose v1.41.0 request-shape notes (no canonical entry →
/// thinking omitted → Opus 5.5 runs adaptive at default effort).
pub const MYSTAGOGUE_MODEL: &str = "claude-opus-5-5";

/// Resolves a caller's optional model override against the pin.
pub fn mystagogue_model(requested: Option<String>) -> String {
    requested
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| MYSTAGOGUE_MODEL.to_string())
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mystagogue_model_defaults_to_the_pinned_id() {
        assert_eq!(MYSTAGOGUE_MODEL, "claude-opus-5-5");
        assert_eq!(mystagogue_model(None), "claude-opus-5-5");
        assert_eq!(
            mystagogue_model(Some("claude-haiku-5-5".into())),
            "claude-haiku-5-5"
        );
        // a blank override is treated as "no override", never sent as model=""
        assert_eq!(mystagogue_model(Some("  ".into())), "claude-opus-5-5");
    }
}
