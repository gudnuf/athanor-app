//! `athanor-cli` — a thin desktop harness over `athanor-core` for fast prompt
//! iteration. It drives the *real* pipeline: `assemble` the system prompt →
//! run one turn through an engine (hermetic `MockEngine` by default, the real
//! embed behind `--features goose`) → dispatch the Mystagogue's tools against a
//! `Store` → land the session.
//!
//! **Shell-thin (the rmp invariant).** All logic lives in `athanor-core`; this
//! crate is orchestration + I/O only — the same contract SwiftUI has. Nothing
//! here reimplements a store operation, a tool, or a state transition; it wires
//! the core's pieces together and renders their output.

use std::sync::Arc;

use athanor_core::engine::{AcpPrompt, AcpUpdate, MockEngine, MystagogueEngine};
use athanor_core::prompt;
use athanor_core::{close_session, Mystagogue, Store};

pub mod script;

/// Minutes recorded against today's tending when a session lands. The engine
/// (core-identity §5) targets ~15-minute sessions; the dev harness records that
/// nominal figure rather than wall-clock, since a scripted turn is instant.
pub const DEFAULT_SESSION_MINUTES: u32 = 15;

/// What one driven session produced — enough for a test or a human to see what
/// happened without reaching into the store.
#[derive(Debug, Clone)]
pub struct SessionOutcome {
    pub session_id: String,
    /// The assistant's streamed text, concatenated.
    pub transcript: String,
    /// Names of the tools the engine invoked, in order.
    pub tools_called: Vec<String>,
    /// Whether the turn reached `TurnComplete` (and so the session was landed).
    pub landed: bool,
}

/// Drives one session turn end-to-end against `engine`, streaming assistant text
/// to `out` as it arrives.
///
/// Glue only: `prompt::assemble`, `Mystagogue`'s tool dispatch, and
/// `close_session`'s tending/wisdom accounting all live in `athanor-core`.
pub async fn run_session(
    store: Arc<Store>,
    engine: &dyn MystagogueEngine,
    mask: &str,
    mode: &str,
    thread_id: Option<&str>,
    out: &mut (dyn std::io::Write + Send),
) -> Result<SessionOutcome, Box<dyn std::error::Error>> {
    let plan = prompt::assemble(mask, mode, thread_id, &store);
    let session = store.create_session(thread_id, mask, mode)?;
    let mystagogue = Mystagogue::new(Arc::clone(&store));

    let acp_prompt = AcpPrompt {
        system: plan.system_prompt,
        user_turns: Vec::new(),
        tools: Mystagogue::tool_specs(),
    };

    let mut transcript = String::new();
    let mut tools_called: Vec<String> = Vec::new();
    let mut landed = false;

    engine
        .run_turn(acp_prompt, &mystagogue, &mut |update| match update {
            AcpUpdate::TextDelta(text) => transcript.push_str(&text),
            AcpUpdate::ToolCall(call) => tools_called.push(call.name),
            AcpUpdate::TurnComplete => landed = true,
        })
        .await?;

    out.write_all(transcript.as_bytes())?;
    out.flush()?;

    // On a completed turn, land the session — the only place wisdom advances.
    if landed {
        let threads: Vec<String> = thread_id.into_iter().map(str::to_string).collect();
        close_session(&store, &session.id, DEFAULT_SESSION_MINUTES, &threads)?;
    }

    Ok(SessionOutcome {
        session_id: session.id,
        transcript,
        tools_called,
        landed,
    })
}

/// Hermetic convenience: drive a session against a scripted [`MockEngine`].
/// This is what the smoke test and the default (non-`goose`) CLI path use.
pub async fn run_scripted_session(
    store: Arc<Store>,
    mask: &str,
    mode: &str,
    thread_id: Option<&str>,
    script: Vec<AcpUpdate>,
    out: &mut (dyn std::io::Write + Send),
) -> Result<SessionOutcome, Box<dyn std::error::Error>> {
    let engine = MockEngine::new(script);
    run_session(store, &engine, mask, mode, thread_id, out).await
}
