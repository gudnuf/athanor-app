//! `athanor-cli` — a thin desktop harness over `athanor-core` for fast prompt
//! iteration. It drives the *real* pipeline via `athanor_core::Conductor`:
//! open a session → run one (scripted, single-turn) turn through an engine
//! (hermetic `MockEngine` by default, the real embed behind `--features
//! goose`) → dispatch the Mystagogue's tools against a `Store` → land the
//! session.
//!
//! **Shell-thin (the rmp invariant).** All logic — prompt assembly, the tool
//! dispatch, the state transitions, transcript/trace persistence — lives in
//! `athanor-core`'s `Conductor`; this crate is orchestration + I/O only, the
//! same contract SwiftUI has. It does not reimplement a store operation, a
//! tool, or a state transition; it drives the Conductor and renders its
//! output.

use std::sync::Arc;

use athanor_core::engine::{AcpUpdate, MockEngine, MystagogueEngine};
use athanor_core::{Conductor, ConductorOutcome, Store};

pub mod script;

/// Minutes recorded against today's tending when a session lands. The engine
/// (core-identity §5) targets ~15-minute sessions; the dev harness records that
/// nominal figure rather than wall-clock, since a scripted turn is instant.
pub const DEFAULT_SESSION_MINUTES: u32 = 15;

/// What one driven session produced — enough for a test or a human to see what
/// happened without reaching into the store. A thin alias over the
/// Conductor's own outcome type, kept as a distinct name here since it's this
/// crate's public surface (existing callers/tests name `SessionOutcome`).
pub type SessionOutcome = ConductorOutcome;

/// Drives one session turn end-to-end against `engine` via a `Conductor`,
/// streaming assistant text to `out` as it arrives, then landing the session
/// on a completed turn.
///
/// Glue only: opening the session, assembling the prompt, dispatching the
/// Mystagogue's tools, and the tending/wisdom + trace accounting on close all
/// live in `athanor-core`'s `Conductor`.
pub async fn run_session(
    store: Arc<Store>,
    engine: &dyn MystagogueEngine,
    mask: &str,
    mode: &str,
    thread_id: Option<&str>,
    out: &mut (dyn std::io::Write + Send),
) -> Result<SessionOutcome, Box<dyn std::error::Error>> {
    let mut conductor = Conductor::begin(Arc::clone(&store), mask, mode, thread_id)?;

    conductor
        .run_turn(engine, None, &mut |update| {
            if let AcpUpdate::TextDelta(text) = &update {
                let _ = out.write_all(text.as_bytes());
            }
        })
        .await?;
    out.flush()?;

    // On a completed turn, land the session — the only place wisdom advances.
    let outcome = if conductor.landed() {
        conductor.close(DEFAULT_SESSION_MINUTES)?
    } else {
        // No TurnComplete: nothing landed, but the harness still needs a
        // report. Read the accumulator back out without abandoning the
        // session — a dev-harness single-turn run that didn't complete is
        // left `open` for a human to inspect, not silently abandoned.
        outcome_without_landing(&conductor)
    };

    Ok(outcome)
}

/// Snapshots a not-yet-landed conductor's accumulators into an outcome
/// without consuming it (so the underlying session stays `open`, matching the
/// pre-Conductor harness's behavior of only closing on `TurnComplete`).
fn outcome_without_landing(conductor: &Conductor) -> SessionOutcome {
    SessionOutcome {
        session_id: conductor.session_id().to_string(),
        transcript: conductor.transcript().to_string(),
        tools_called: conductor.tools_called().to_vec(),
        landed: conductor.landed(),
    }
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
