//! `GooseEngine`: the real, in-process (or `goosed`-over-tailnet, if Spike
//! 1a came back RED) embed of the model-driving engine.
//!
//! This file is a **compile-gated stub**. It exists so the `MystagogueEngine`
//! seam has a real second implementation to type-check against, but it does
//! not — and must not, until the spike is confirmed GREEN — make any real
//! goose call. The exact embed API (which goose types, which entry point,
//! streaming shape) is still being spiked; see
//! `forge/athanor-app/spike-goose-ios-report.md` in the athanor meta repo
//! for the pinned tag and the confirmed (or refuted) in-process embed path.
//! (That report may not exist yet at the time this stub is written — that's
//! expected; it's produced by the Task 4 spike, not by this task.)
//!
//! TODO(spike-goose-ios-report): once GREEN, replace this stub with the real
//! conversion between `AcpPrompt`/`AcpUpdate`/`AcpToolCall`/`AcpToolResult`
//! (this crate's own types, defined in `acp.rs`) and whatever the pinned
//! `agent-client-protocol` / `goose-sdk-types` crates expose. No other module
//! in this codebase should ever import those crates directly — only this
//! file (or its `goosed_client.rs` replacement on the RED path) may.

use super::{AcpPrompt, AcpUpdate, EngineError, MystagogueEngine, ToolDispatch};
use async_trait::async_trait;

/// Holds the in-process agent from the pinned goose tag, once wired up.
/// Presently empty — this is a stub.
pub struct GooseEngine {
    _private: (),
}

impl GooseEngine {
    /// Constructing a real `GooseEngine` is not yet possible: the embed API
    /// is still being spiked. See the module doc for the pointer to the
    /// spike report.
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for GooseEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MystagogueEngine for GooseEngine {
    async fn run_turn(
        &self,
        _prompt: AcpPrompt,
        _tools: &dyn ToolDispatch,
        _sink: &mut (dyn FnMut(AcpUpdate) + Send),
    ) -> Result<(), EngineError> {
        Err(EngineError::Other(
            "GooseEngine is a stub — real embed not wired up yet; see \
             forge/athanor-app/spike-goose-ios-report.md"
                .into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mirrors the Task-4 spike's real-API round-trip. Gated behind
    /// `feature = "goose"` (never compiled in the hermetic tier) AND
    /// `#[ignore]` (never run by default even when the feature is on) —
    /// run explicitly once the spike report confirms the embed API:
    /// `cargo test -p athanor-core --features goose -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn goose_engine_real_api_round_trip() {
        let engine = GooseEngine::new();
        let prompt = AcpPrompt {
            system: "you are the mystagogue".into(),
            user_turns: vec!["what is entropy?".into()],
            tools: vec![],
        };
        struct NoTools;
        #[async_trait]
        impl ToolDispatch for NoTools {
            async fn dispatch(
                &self,
                call: super::super::AcpToolCall,
            ) -> super::super::AcpToolResult {
                super::super::AcpToolResult {
                    id: call.id,
                    value: serde_json::json!({}),
                }
            }
        }
        let mut updates = Vec::new();
        // TODO: once the spike report lands, this should succeed; today the
        // stub always errors, so this assertion documents the gate rather
        // than exercising a real embed.
        let result = engine
            .run_turn(prompt, &NoTools, &mut |u| updates.push(u))
            .await;
        assert!(result.is_err(), "stub must not silently succeed");
    }
}
