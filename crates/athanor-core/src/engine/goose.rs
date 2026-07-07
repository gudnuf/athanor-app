//! `GooseEngine`: the real, in-process embed of the model-driving engine,
//! built on the `goose` crate hard-pinned to tag `v1.41.0` (see
//! `forge/athanor-app/spike-goose-ios-report.md`, Gate 1 — GREEN).
//!
//! This is the ONE file in the codebase that names a `goose` /
//! `goose-providers` / `rmcp` type. Everything else talks to the seam types in
//! `acp.rs` (`AcpPrompt`/`AcpUpdate`/`AcpToolCall`/`AcpToolResult`) and the
//! `MystagogueEngine`/`ToolDispatch` traits in `mod.rs`. The whole module is
//! gated behind `feature = "goose"`; the hermetic tier never compiles it.
//!
//! ## How tool dispatch bridges to `ToolDispatch`
//!
//! goose's builtin-extension registration is a bare `fn` pointer (no captures),
//! so it can't carry a borrowed `&dyn ToolDispatch`. Instead we use goose's
//! **frontend-tool** mechanism: the caller's tools are registered as an
//! `ExtensionConfig::Frontend`, and when the model calls one, goose's `reply`
//! stream yields a `FrontendToolRequest` and then *blocks* waiting for the
//! frontend to answer via `agent.handle_tool_result(id, ...)`. We service that
//! inline in the reply loop by routing the call through the caller's
//! `ToolDispatch` and feeding the result straight back. No globals, no
//! `fn`-pointer bridge, no child process — exactly the in-process seam the
//! spike proved, mapped 1:1 onto our own `ToolDispatch` trait.

use super::{
    AcpPrompt, AcpRole, AcpToolCall, AcpUpdate, EngineError, MystagogueEngine, ToolDispatch,
};
use async_trait::async_trait;
use std::sync::Arc;

use futures::StreamExt;

use goose::agents::{Agent, AgentEvent, ExtensionConfig, SessionConfig};
use goose::config::GooseMode;
use goose::conversation::message::{Message, MessageContent};
use goose::providers::base::Provider;
use goose::session::session_manager::SessionType;

use goose_providers::anthropic::{
    AnthropicProviderBuilder, ANTHROPIC_API_VERSION, ANTHROPIC_DEFAULT_MODEL,
};
use goose_providers::api_client::{ApiClient, AuthMethod};
use goose_providers::model::ModelConfig;

use rmcp::model::{CallToolResult, Content, Tool};

const ANTHROPIC_API_HOST: &str = "https://api.anthropic.com";

/// The real embedded engine. Holds the Anthropic API key (passed in by the
/// caller — NEVER read from the environment or a `.env` inside this library)
/// and the model id to drive.
pub struct GooseEngine {
    anthropic_api_key: String,
    model: String,
}

impl GooseEngine {
    /// Construct a `GooseEngine`. `anthropic_api_key` is injected by the caller;
    /// `model` defaults to goose's Anthropic default (`claude-sonnet-4-5`) when
    /// `None`.
    pub fn new(anthropic_api_key: String, model: Option<String>) -> Self {
        Self {
            anthropic_api_key,
            model: model.unwrap_or_else(|| ANTHROPIC_DEFAULT_MODEL.to_string()),
        }
    }

    /// Build the Anthropic provider from the injected key. No environment reads.
    fn build_provider(&self) -> Result<Arc<dyn Provider>, EngineError> {
        let auth = AuthMethod::ApiKey {
            header_name: "x-api-key".to_string(),
            key: self.anthropic_api_key.clone(),
        };
        let api_client = ApiClient::new_with_tls(ANTHROPIC_API_HOST.to_string(), auth, None)
            .and_then(|c| c.with_header("anthropic-version", ANTHROPIC_API_VERSION))
            .map_err(|e| EngineError::Other(format!("anthropic api client: {e}")))?;
        let provider = AnthropicProviderBuilder::new(api_client).build();
        Ok(Arc::new(provider))
    }

    /// The engine core, factored out so tests can inject a canned, no-network
    /// provider (mirroring the spike's round-trip) instead of hitting Anthropic.
    async fn drive(
        &self,
        provider: Arc<dyn Provider>,
        model_config: ModelConfig,
        prompt: AcpPrompt,
        tools: &dyn ToolDispatch,
        sink: &mut (dyn FnMut(AcpUpdate) + Send),
    ) -> Result<(), EngineError> {
        let agent = Agent::new();

        // A fresh, hidden session per turn. The working dir only matters for
        // extensions that touch the filesystem; ours don't.
        let session = agent
            .config
            .session_manager
            .create_session(
                std::env::temp_dir(),
                "athanor-turn".to_string(),
                SessionType::Hidden,
                GooseMode::default(),
            )
            .await
            .map_err(|e| EngineError::Other(format!("create_session: {e}")))?;

        agent.override_system_prompt(prompt.system.clone()).await;

        agent
            .update_provider(provider, model_config, &session.id)
            .await
            .map_err(|e| EngineError::Other(format!("update_provider: {e}")))?;

        // Register the caller's tools as frontend tools. When the model calls
        // one, goose routes it back to us (see module docs) instead of an MCP
        // extension.
        if !prompt.tools.is_empty() {
            let tool_specs: Vec<Tool> = prompt
                .tools
                .iter()
                .map(|t| {
                    let schema = match &t.json_schema {
                        serde_json::Value::Object(m) => m.clone(),
                        _ => serde_json::Map::new(),
                    };
                    Tool::new(t.name.clone(), t.name.clone(), schema)
                })
                .collect();
            agent
                .add_extension(
                    ExtensionConfig::Frontend {
                        name: "mystagogue".to_string(),
                        description: "The Mystagogue's verbs.".to_string(),
                        tools: tool_specs,
                        instructions: None,
                        bundled: Some(true),
                        available_tools: vec![],
                    },
                    &session.id,
                )
                .await
                .map_err(|e| EngineError::Other(format!("add_extension: {e}")))?;
        }

        // Map the ACP prompt's full dialogue history onto the session:
        // everything before the last turn is prior history, seeded onto the
        // goose session with role preserved (SHOULD-FIX-4 — `AcpPrompt` now
        // carries the Mystagogue's own prior replies, not just the learner's
        // turns, so a live multi-turn session sees what it already said);
        // the last turn drives this call to `agent.reply`. `reply` only
        // accepts a user-authored message, so the prompt must end on a
        // `Learner` turn — the Conductor guarantees this (every ordinary
        // `run_turn` appends the learner's turn last; `open_turn` seeds a
        // synthesized learner-arrival marker for the case where there is no
        // real learner utterance yet, e.g. initiation's cold open).
        let (last_turn, prior_turns) = prompt
            .turns
            .split_last()
            .ok_or_else(|| EngineError::Other("run_turn: turns is empty".into()))?;
        if !matches!(last_turn.role, AcpRole::Learner) {
            return Err(EngineError::Other(
                "run_turn: the last turn must be from the learner".into(),
            ));
        }

        for turn in prior_turns {
            let msg = match turn.role {
                AcpRole::Learner => Message::user().with_text(turn.text.clone()),
                AcpRole::Mystagogue => Message::assistant().with_text(turn.text.clone()),
            };
            agent
                .config
                .session_manager
                .add_message(&session.id, &msg)
                .await
                .map_err(|e| EngineError::Other(format!("seed history: {e}")))?;
        }

        let session_config = SessionConfig {
            id: session.id.clone(),
            schedule_id: None,
            max_turns: Some(50),
            retry_config: None,
        };
        let user = Message::user().with_text(last_turn.text.clone());

        // `reply` borrows `&agent` for the stream's lifetime; `handle_tool_result`
        // also takes `&self`, so both coexist as shared borrows — we can answer a
        // frontend tool call inline without cloning the agent into an Arc.
        let mut stream = agent
            .reply(user, session_config, None)
            .await
            .map_err(|e| EngineError::Other(format!("reply: {e}")))?;

        while let Some(ev) = stream.next().await {
            match ev {
                Ok(AgentEvent::Message(message)) => {
                    for content in &message.content {
                        match content {
                            MessageContent::Text(t) if !t.text.is_empty() => {
                                sink(AcpUpdate::text_delta(t.text.clone()));
                            }
                            MessageContent::FrontendToolRequest(req) => match &req.tool_call {
                                Ok(params) => {
                                    let args = params
                                        .arguments
                                        .clone()
                                        .map(serde_json::Value::Object)
                                        .unwrap_or(serde_json::Value::Null);
                                    let call = AcpToolCall {
                                        id: req.id.clone(),
                                        name: params.name.to_string(),
                                        args,
                                    };
                                    sink(AcpUpdate::ToolCall(call.clone()));

                                    let result = tools.dispatch(call).await;
                                    // Stream the result to the bridge (carries
                                    // fix_salt's realization id) before handing
                                    // it back to the agent loop.
                                    sink(AcpUpdate::ToolResult(result.clone()));
                                    let payload = serde_json::to_string(&result.value)
                                        .unwrap_or_else(|_| "{}".to_string());
                                    agent
                                        .handle_tool_result(
                                            req.id.clone(),
                                            Ok(CallToolResult::success(vec![Content::text(
                                                payload,
                                            )])),
                                        )
                                        .await;
                                }
                                Err(e) => {
                                    // Model produced a malformed tool call; hand
                                    // the error back so the agent loop unblocks.
                                    agent
                                        .handle_tool_result(req.id.clone(), Err(e.clone()))
                                        .await;
                                }
                            },
                            _ => {}
                        }
                    }
                }
                Ok(_) => {}
                Err(e) => return Err(EngineError::Other(format!("reply stream: {e}"))),
            }
        }

        sink(AcpUpdate::TurnComplete);
        Ok(())
    }
}

#[async_trait]
impl MystagogueEngine for GooseEngine {
    async fn run_turn(
        &self,
        prompt: AcpPrompt,
        tools: &dyn ToolDispatch,
        sink: &mut (dyn FnMut(AcpUpdate) + Send),
    ) -> Result<(), EngineError> {
        let provider = self.build_provider()?;
        let model_config = ModelConfig::new(self.model.clone());
        self.drive(provider, model_config, prompt, tools, sink)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{AcpToolResult, AcpToolSpec, AcpTurn};
    use std::sync::atomic::{AtomicBool, Ordering};

    use async_trait::async_trait as async_trait_macro;

    use goose::providers::base::{
        stream_from_single_message, MessageStream, Provider, ProviderUsage, Usage,
    };
    use goose_providers::errors::ProviderError;
    use rmcp::model::Role;

    // A ToolDispatch that records whether it was invoked and with what name,
    // and returns a canned value — our stand-in for Task 9's Mystagogue.
    #[derive(Default)]
    struct RecordingDispatch {
        called: AtomicBool,
        last_name: std::sync::Mutex<Option<String>>,
    }

    #[async_trait]
    impl ToolDispatch for RecordingDispatch {
        async fn dispatch(&self, call: AcpToolCall) -> AcpToolResult {
            self.called.store(true, Ordering::SeqCst);
            *self.last_name.lock().unwrap() = Some(call.name.clone());
            AcpToolResult {
                id: call.id,
                value: serde_json::json!({ "ok": true, "note": "salt fixed" }),
            }
        }
    }

    /// Canned in-process provider — no network, no API key. First turn: request
    /// the frontend tool the agent offered (name ends in `fix_salt`). After the
    /// tool response returns: emit terminal text so the loop ends. This is the
    /// spike's `CannedProvider`, retargeted at the frontend-tool path.
    struct CannedProvider;

    #[async_trait_macro]
    impl Provider for CannedProvider {
        fn get_name(&self) -> &str {
            "canned"
        }

        async fn stream(
            &self,
            model_config: &ModelConfig,
            _system: &str,
            messages: &[Message],
            tools: &[rmcp::model::Tool],
        ) -> Result<MessageStream, ProviderError> {
            let usage = ProviderUsage::new(model_config.model_name.clone(), Usage::default());

            let already_responded = messages.iter().any(|m| {
                m.content
                    .iter()
                    .any(|c| matches!(c, MessageContent::ToolResponse(_)))
            });

            let fix_salt_tool = tools
                .iter()
                .map(|t| t.name.to_string())
                .find(|n| n.ends_with("fix_salt"));

            let msg = match fix_salt_tool {
                Some(tool_name) if !already_responded => {
                    let mut params = rmcp::model::CallToolRequestParams::new(tool_name);
                    params.arguments = serde_json::json!({
                        "realization": "heat is transformation, not destruction"
                    })
                    .as_object()
                    .cloned();
                    Message::assistant().with_tool_request("call_1", Ok(params))
                }
                _ => Message::assistant().with_text("The salt is fixed. Well drawn."),
            };

            Ok(stream_from_single_message(msg, usage))
        }
    }

    fn salt_prompt() -> AcpPrompt {
        AcpPrompt {
            system: "You are the Mystagogue.".into(),
            turns: vec![AcpTurn {
                role: AcpRole::Learner,
                text: "I think I finally get it about heat.".into(),
            }],
            tools: vec![AcpToolSpec {
                name: "fix_salt".into(),
                json_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "realization": { "type": "string" }
                    },
                    "required": ["realization"]
                }),
            }],
        }
    }

    /// Ports the spike's round-trip into the real engine: canned provider →
    /// frontend tool request → our `ToolDispatch` executes → result flows back →
    /// terminal text → `TurnComplete`. No network.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn goose_engine_frontend_tool_round_trip() {
        let engine = GooseEngine::new("unused-in-canned-path".into(), Some("canned-model".into()));
        let provider: Arc<dyn Provider> = Arc::new(CannedProvider);
        let dispatch = RecordingDispatch::default();

        let mut updates: Vec<AcpUpdate> = Vec::new();
        engine
            .drive(
                provider,
                ModelConfig::new("canned-model"),
                salt_prompt(),
                &dispatch,
                &mut |u| updates.push(u),
            )
            .await
            .expect("canned round-trip should succeed");

        assert!(
            dispatch.called.load(Ordering::SeqCst),
            "ToolDispatch must have executed"
        );
        assert_eq!(
            dispatch.last_name.lock().unwrap().as_deref(),
            Some("fix_salt"),
            "dispatched tool name should be the frontend tool"
        );
        assert!(
            updates
                .iter()
                .any(|u| matches!(u, AcpUpdate::ToolCall(c) if c.name == "fix_salt")),
            "engine must surface the tool call: {updates:?}"
        );
        assert!(
            updates
                .iter()
                .any(|u| matches!(u, AcpUpdate::TextDelta { .. })),
            "engine must stream terminal text: {updates:?}"
        );
        assert!(
            matches!(updates.last(), Some(AcpUpdate::TurnComplete)),
            "engine must end with TurnComplete: {updates:?}"
        );
    }

    /// A provider that records the `messages` array from EVERY call it
    /// receives and replies with fixed terminal text — for asserting what
    /// history the engine seeded onto the goose session. Records every call,
    /// not just the first, because `agent.reply` also issues an internal
    /// title-generation call against the same provider; the test picks out
    /// the actual chat-turn call by content rather than assuming call order.
    #[derive(Default)]
    struct RecordingProvider {
        calls: std::sync::Mutex<Vec<Vec<Message>>>,
    }

    #[async_trait_macro]
    impl Provider for RecordingProvider {
        fn get_name(&self) -> &str {
            "recording"
        }

        async fn stream(
            &self,
            model_config: &ModelConfig,
            _system: &str,
            messages: &[Message],
            _tools: &[rmcp::model::Tool],
        ) -> Result<MessageStream, ProviderError> {
            self.calls.lock().unwrap().push(messages.to_vec());
            let usage = ProviderUsage::new(model_config.model_name.clone(), Usage::default());
            let msg = Message::assistant().with_text("acknowledged.");
            Ok(stream_from_single_message(msg, usage))
        }
    }

    /// SHOULD-FIX-4: a prompt whose history includes the Mystagogue's own
    /// prior reply must seed that reply back onto the goose session as an
    /// **assistant**-authored message (not folded into the user turns), so a
    /// live multi-turn session sees what it already said.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn goose_engine_seeds_prior_turns_with_roles_preserved() {
        let engine = GooseEngine::new("unused-in-canned-path".into(), Some("canned-model".into()));
        let provider = Arc::new(RecordingProvider::default());
        let dispatch = RecordingDispatch::default();

        let prompt = AcpPrompt {
            system: "sys".into(),
            turns: vec![
                AcpTurn {
                    role: AcpRole::Learner,
                    text: "first learner turn".into(),
                },
                AcpTurn {
                    role: AcpRole::Mystagogue,
                    text: "first mystagogue reply".into(),
                },
                AcpTurn {
                    role: AcpRole::Learner,
                    text: "second learner turn".into(),
                },
            ],
            tools: vec![],
        };

        engine
            .drive(
                provider.clone(),
                ModelConfig::new("canned-model"),
                prompt,
                &dispatch,
                &mut |_| {},
            )
            .await
            .expect("history-seeded round-trip should succeed");

        let calls = provider.calls.lock().unwrap().clone();
        assert!(!calls.is_empty(), "provider must have been invoked");
        // The actual chat-turn call is whichever one carries the driving
        // message ("second learner turn") — `agent.reply` also fires an
        // internal title-generation call against the same provider, which
        // this must not be confused with.
        let chat_call = calls
            .iter()
            .find(|msgs| {
                msgs.iter().any(|m| {
                    m.content.iter().any(
                        |c| matches!(c, MessageContent::Text(t) if t.text == "second learner turn"),
                    )
                })
            })
            .expect("one call should carry the driving user turn");

        // goose's own agent loop appends extra content blocks to a message
        // (e.g. a `<turn-context>` system-injected block alongside the real
        // text) — collect every text block per message rather than assuming
        // there's exactly one, and check membership rather than exact
        // message-level equality.
        let role_for_text = |wanted: &str| -> Option<Role> {
            chat_call.iter().find_map(|m| {
                let has_it = m
                    .content
                    .iter()
                    .any(|c| matches!(c, MessageContent::Text(t) if t.text == wanted));
                has_it.then(|| m.role.clone())
            })
        };

        assert_eq!(
            role_for_text("first learner turn"),
            Some(Role::User),
            "learner turn should seed as a user message"
        );
        assert_eq!(
            role_for_text("first mystagogue reply"),
            Some(Role::Assistant),
            "the Mystagogue's own prior reply should seed as an ASSISTANT message, not a \
             user message — that's the whole point of the fix"
        );
        assert_eq!(
            role_for_text("second learner turn"),
            Some(Role::User),
            "the final learner turn should drive the reply as a user message"
        );
    }

    /// A prompt that ends on the Mystagogue's own turn (no trailing learner
    /// turn) cannot drive `agent.reply` (which only accepts a user-authored
    /// message) — the engine must reject it rather than silently mis-attribute
    /// the turn.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn goose_engine_errors_when_prompt_does_not_end_on_a_learner_turn() {
        let engine = GooseEngine::new("unused-in-canned-path".into(), Some("canned-model".into()));
        let provider: Arc<dyn Provider> = Arc::new(RecordingProvider::default());
        let dispatch = RecordingDispatch::default();

        let prompt = AcpPrompt {
            system: "sys".into(),
            turns: vec![AcpTurn {
                role: AcpRole::Mystagogue,
                text: "an opening the learner never got to answer".into(),
            }],
            tools: vec![],
        };

        let result = engine
            .drive(
                provider,
                ModelConfig::new("canned-model"),
                prompt,
                &dispatch,
                &mut |_| {},
            )
            .await;

        assert!(
            result.is_err(),
            "a prompt ending on a Mystagogue turn must be rejected, not silently misrouted"
        );
    }

    /// Live Anthropic round-trip. Ignored by default; run explicitly with a key:
    /// `ANTHROPIC_API_KEY=… cargo test -p athanor-core --features goose -- --ignored`.
    /// Skips gracefully (passes) when the key is unset.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore]
    async fn goose_engine_live_anthropic_round_trip() {
        let Ok(key) = std::env::var("ANTHROPIC_API_KEY") else {
            eprintln!("ANTHROPIC_API_KEY unset — skipping live test");
            return;
        };

        let engine = GooseEngine::new(key, None);
        let dispatch = RecordingDispatch::default();
        let prompt = AcpPrompt {
            system: "You are a terse assistant. Answer in one short sentence.".into(),
            turns: vec![AcpTurn {
                role: AcpRole::Learner,
                text: "Say the single word: ready.".into(),
            }],
            tools: vec![],
        };

        let mut updates: Vec<AcpUpdate> = Vec::new();
        engine
            .run_turn(prompt, &dispatch, &mut |u| updates.push(u))
            .await
            .expect("live round-trip should succeed");

        assert!(
            updates
                .iter()
                .any(|u| matches!(u, AcpUpdate::TextDelta { .. })),
            "live turn must stream text: {updates:?}"
        );
        assert!(
            matches!(updates.last(), Some(AcpUpdate::TurnComplete)),
            "live turn must end with TurnComplete: {updates:?}"
        );
    }
}
