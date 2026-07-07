//! `MockEngine`: replays a scripted sequence of `AcpUpdate`s. Hermetic —
//! never touches a model or the network — so it backs the fast test tier and
//! `athanor-cli`'s default (non-`goose`) mode.

use super::{AcpPrompt, AcpToolCall, AcpUpdate, EngineError, MystagogueEngine, ToolDispatch};
use async_trait::async_trait;
use std::collections::VecDeque;
use std::sync::Mutex;

pub struct MockEngine {
    script: Mutex<VecDeque<AcpUpdate>>,
}

impl MockEngine {
    pub fn new(script: Vec<AcpUpdate>) -> Self {
        Self {
            script: Mutex::new(script.into_iter().collect()),
        }
    }
}

#[async_trait]
impl MystagogueEngine for MockEngine {
    async fn run_turn(
        &self,
        _prompt: AcpPrompt,
        tools: &dyn ToolDispatch,
        sink: &mut (dyn FnMut(AcpUpdate) + Send),
    ) -> Result<(), EngineError> {
        let queue: Vec<AcpUpdate> = {
            let mut guard = self.script.lock().unwrap();
            guard.drain(..).collect()
        };
        for update in queue {
            if let AcpUpdate::ToolCall(call) = &update {
                let call: AcpToolCall = call.clone();
                sink(update);
                // Stream the dispatched result too, so the bridge sees the
                // tool's real return value (mirrors GooseEngine).
                let result = tools.dispatch(call).await;
                sink(AcpUpdate::ToolResult(result));
            } else {
                sink(update);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{AcpRole, AcpToolResult, AcpToolSpec, AcpTurn};
    use std::sync::Mutex as StdMutex;

    #[derive(Default)]
    struct RecordingDispatch {
        calls: StdMutex<Vec<String>>,
    }

    impl RecordingDispatch {
        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl ToolDispatch for RecordingDispatch {
        async fn dispatch(&self, call: AcpToolCall) -> AcpToolResult {
            self.calls.lock().unwrap().push(call.name.clone());
            AcpToolResult {
                id: call.id,
                value: serde_json::json!({}),
            }
        }
    }

    fn demo_prompt() -> AcpPrompt {
        AcpPrompt {
            system: "sys".into(),
            turns: vec![AcpTurn {
                role: AcpRole::Learner,
                text: "hi".into(),
            }],
            tools: vec![AcpToolSpec {
                name: "open_thread".into(),
                json_schema: serde_json::json!({}),
            }],
        }
    }

    #[tokio::test]
    async fn mock_engine_streams_then_dispatches_a_tool_then_completes() {
        let engine = MockEngine::new(vec![
            AcpUpdate::TextDelta("Consider ".into()),
            AcpUpdate::ToolCall(AcpToolCall {
                id: "1".into(),
                name: "open_thread".into(),
                args: serde_json::json!({"question": "why?"}),
            }),
            AcpUpdate::TurnComplete,
        ]);
        let tools = RecordingDispatch::default();
        let mut got = Vec::new();
        engine
            .run_turn(demo_prompt(), &tools, &mut |u| got.push(format!("{u:?}")))
            .await
            .unwrap();
        assert!(got.iter().any(|g| g.contains("TextDelta")));
        assert_eq!(tools.calls(), vec!["open_thread"]);
        assert!(got.iter().any(|g| g.contains("TurnComplete")));
    }

    #[tokio::test]
    async fn mock_engine_with_no_tool_calls_only_streams_and_completes() {
        let engine = MockEngine::new(vec![
            AcpUpdate::TextDelta("just text".into()),
            AcpUpdate::TurnComplete,
        ]);
        let tools = RecordingDispatch::default();
        let mut got = Vec::new();
        engine
            .run_turn(demo_prompt(), &tools, &mut |u| got.push(format!("{u:?}")))
            .await
            .unwrap();
        assert!(tools.calls().is_empty());
        assert_eq!(got.len(), 2);
    }
}
