//! The session conductor: owns one session's arc from open to landed (or
//! abandoned) so `athanor-cli`, the FFI bridge, and tests all drive the exact
//! same path instead of each re-implementing "assemble → run a turn → land."
//!
//! Arc:
//! ```text
//! Conductor::begin / begin_initiation   → create_session row, pick the plan
//!   .run_turn(engine, learner_turn, sink)  → assemble (fresh each call, so
//!                                            newly-accumulated turns and any
//!                                            store writes the turn just made
//!                                            are reflected) → engine.run_turn
//!                                            streaming AcpUpdate to sink →
//!                                            append_transcript
//!   ... repeated for each learner turn (multi-turn) ...
//!   .close(minutes)   → close_session (tending/wisdom) + add_trace
//!   .abandon()        → abandon_session (thread returns to volatile)
//! ```
//!
//! Generic over `dyn MystagogueEngine` (the engine seam, `engine/mod.rs`):
//! `MockEngine` in tests, `GooseEngine` behind `feature = "goose"` in
//! production. Nothing here names a goose type.

use std::sync::Arc;

use crate::engine::{AcpPrompt, AcpUpdate, EngineError, MystagogueEngine};
use crate::error::CoreError;
use crate::mystagogue::Mystagogue;
use crate::prompt;
use crate::session::{abandon_session, close_session};
use crate::store::Store;

/// Default minutes recorded against today's tending when a session lands
/// without an explicit duration (core-identity §5: "assume ~15").
pub const DEFAULT_SESSION_MINUTES: u32 = 15;

/// The mask/mode pair the initiation flow assembles under (`prompt::
/// assemble_initiation` doesn't take a mask/mode — this is just the label the
/// conductor stores on the session row).
const INITIATION_MASK: &str = "initiation";
const INITIATION_MODE: &str = "initiation";

/// Longest a synthesized one-line trace is allowed to run, in `char`s, before
/// it's truncated with an ellipsis.
const TRACE_MAX_CHARS: usize = 220;

#[derive(Debug, thiserror::Error)]
pub enum ConductorError {
    #[error(transparent)]
    Core(#[from] CoreError),
    #[error(transparent)]
    Engine(#[from] EngineError),
}

/// What driving a session through the conductor produced — enough for a
/// caller (CLI, FFI, test) to report on without reaching back into the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConductorOutcome {
    pub session_id: String,
    /// The assistant's streamed text across every turn, concatenated.
    pub transcript: String,
    /// Names of the tools the engine invoked, across every turn, in order.
    pub tools_called: Vec<String>,
    /// Whether the most recent turn reached `TurnComplete`.
    pub landed: bool,
}

/// Owns one session's arc. See the module docs for the lifecycle.
pub struct Conductor {
    store: Arc<Store>,
    mystagogue: Mystagogue,
    session_id: String,
    mask: String,
    mode: String,
    thread_id: Option<String>,
    /// Learner turns accumulated so far, oldest first — re-sent (via a
    /// freshly-assembled prompt) on every subsequent `run_turn` so a
    /// multi-turn session's prompt deterministically includes everything said
    /// so far.
    user_turns: Vec<String>,
    transcript: String,
    tools_called: Vec<String>,
    landed: bool,
}

impl Conductor {
    /// Opens a session against `(mask, mode, thread_id)` — the ordinary path
    /// (`prompt::assemble`).
    pub fn begin(
        store: Arc<Store>,
        mask: &str,
        mode: &str,
        thread_id: Option<&str>,
    ) -> Result<Self, CoreError> {
        let session = store.create_session(thread_id, mask, mode)?;
        Ok(Self::from_session(
            store,
            session.id,
            mask.to_string(),
            mode.to_string(),
            thread_id.map(str::to_string),
        ))
    }

    /// Opens the first-launch initiation session (`prompt::
    /// assemble_initiation`) — no mask/mode/thread selected yet.
    pub fn begin_initiation(store: Arc<Store>) -> Result<Self, CoreError> {
        let session = store.create_session(None, INITIATION_MASK, INITIATION_MODE)?;
        Ok(Self::from_session(
            store,
            session.id,
            INITIATION_MASK.to_string(),
            INITIATION_MODE.to_string(),
            None,
        ))
    }

    fn from_session(
        store: Arc<Store>,
        session_id: String,
        mask: String,
        mode: String,
        thread_id: Option<String>,
    ) -> Self {
        let mystagogue = Mystagogue::new(Arc::clone(&store));
        Self {
            store,
            mystagogue,
            session_id,
            mask,
            mode,
            thread_id,
            user_turns: Vec::new(),
            transcript: String::new(),
            tools_called: Vec::new(),
            landed: false,
        }
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn thread_id(&self) -> Option<&str> {
        self.thread_id.as_deref()
    }

    /// The assistant text streamed across every `run_turn` so far.
    pub fn transcript(&self) -> &str {
        &self.transcript
    }

    /// Tool names invoked across every `run_turn` so far, in order.
    pub fn tools_called(&self) -> &[String] {
        &self.tools_called
    }

    /// Whether the most recent turn reached `TurnComplete`.
    pub fn landed(&self) -> bool {
        self.landed
    }

    fn is_initiation(&self) -> bool {
        self.mask == INITIATION_MASK && self.mode == INITIATION_MODE
    }

    /// Assembles the prompt fresh from current store state + accumulated
    /// learner turns. Called at the top of every `run_turn` so later turns
    /// see whatever the session (or a prior turn's tool calls) has changed —
    /// `assemble`/`assemble_initiation` are pure over store state, so this
    /// stays deterministic given a fixed store + turn history.
    fn assemble_prompt(&self) -> AcpPrompt {
        let plan = if self.is_initiation() {
            prompt::assemble_initiation(&self.store)
        } else {
            prompt::assemble(
                &self.mask,
                &self.mode,
                self.thread_id.as_deref(),
                &self.store,
            )
        };
        AcpPrompt {
            system: plan.system_prompt,
            user_turns: self.user_turns.clone(),
            tools: Mystagogue::tool_specs(),
        }
    }

    /// Runs one turn: optionally appends `learner_turn` to the accumulated
    /// turn history, re-assembles the prompt (so it includes that turn and
    /// everything before it), then drives `engine.run_turn` against the
    /// Mystagogue's tool dispatch, streaming every `AcpUpdate` to `sink` as it
    /// arrives. Assistant text is appended to the store's transcript column
    /// and to the conductor's own accumulator; tool names and the landed flag
    /// accumulate across calls (`tools_called`/`landed` reflect the whole
    /// session, not just this turn).
    ///
    /// Call this once for a single-turn session, or repeatedly — passing the
    /// next learner turn each time — for a multi-turn one.
    pub async fn run_turn(
        &mut self,
        engine: &dyn MystagogueEngine,
        learner_turn: Option<&str>,
        sink: &mut (dyn FnMut(AcpUpdate) + Send),
    ) -> Result<(), ConductorError> {
        if let Some(turn) = learner_turn {
            self.user_turns.push(turn.to_string());
        }
        let acp_prompt = self.assemble_prompt();

        let mut turn_text = String::new();
        let mut turn_landed = false;
        let tools_called = &mut self.tools_called;

        engine
            .run_turn(acp_prompt, &self.mystagogue, &mut |update| {
                match &update {
                    AcpUpdate::TextDelta(text) => turn_text.push_str(text),
                    AcpUpdate::ToolCall(call) => tools_called.push(call.name.clone()),
                    AcpUpdate::TurnComplete => turn_landed = true,
                }
                sink(update);
            })
            .await?;

        if !turn_text.is_empty() {
            self.store.append_transcript(&self.session_id, &turn_text)?;
        }
        self.transcript.push_str(&turn_text);
        self.landed = turn_landed;

        Ok(())
    }

    /// Synthesizes the one-line trace future sessions read (`Store::
    /// last_trace`): the accumulated transcript, whitespace-collapsed onto
    /// one line and capped at `TRACE_MAX_CHARS`. A session that never
    /// streamed any text (e.g. abandoned before a word landed) gets a
    /// placeholder rather than an empty row.
    fn synthesize_trace(&self) -> String {
        let collapsed = self
            .transcript
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if collapsed.is_empty() {
            return "(no assistant text this session)".to_string();
        }
        if collapsed.chars().count() > TRACE_MAX_CHARS {
            let mut truncated: String = collapsed.chars().take(TRACE_MAX_CHARS).collect();
            truncated.push('…');
            truncated
        } else {
            collapsed
        }
    }

    /// Lands the session: `close_session` (marks it closed, records tending
    /// for today — the only place wisdom advances) then writes the one-line
    /// trace. Consumes the conductor; call `run_turn` for every turn first.
    pub fn close(self, minutes: u32) -> Result<ConductorOutcome, CoreError> {
        let trace = self.synthesize_trace();
        let thread_ids: Vec<String> = self.thread_id.clone().into_iter().collect();
        close_session(&self.store, &self.session_id, minutes, &thread_ids)?;
        self.store.add_trace(&self.session_id, &trace)?;
        Ok(self.into_outcome())
    }

    /// Abandons the session (interrupted mid-way): `abandon_session` marks it
    /// abandoned and returns its thread (if any) to volatile so it re-enters
    /// the working pool. No trace is written for an abandoned session — there
    /// is nothing settled to remember it by.
    pub fn abandon(self) -> Result<ConductorOutcome, CoreError> {
        abandon_session(&self.store, &self.session_id)?;
        Ok(self.into_outcome())
    }

    fn into_outcome(self) -> ConductorOutcome {
        ConductorOutcome {
            session_id: self.session_id,
            transcript: self.transcript,
            tools_called: self.tools_called,
            landed: self.landed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ThreadState;
    use crate::engine::{AcpToolCall, MockEngine};
    use serde_json::json;

    fn store_arc() -> Arc<Store> {
        Arc::new(Store::open_in_memory("dev").unwrap())
    }

    /// The plan's acceptance scenario: a scripted 2-turn session reaching
    /// `fix_salt` lands an immutable realization + spiral child thread
    /// (volatile, back-linked), kindles SALT, and writes a trace at close.
    #[tokio::test]
    async fn two_turn_scripted_session_fixes_salt_and_writes_trace() {
        let store = store_arc();
        let domain = store.upsert_domain("thermodynamics").unwrap();
        let parent = store
            .open_thread("does forgetting cost energy?", Some(&domain.id), None)
            .unwrap();

        let mut conductor = Conductor::begin(
            Arc::clone(&store),
            "philosophus",
            "explain",
            Some(&parent.id),
        )
        .unwrap();

        // Turn 1: the Mystagogue reflects; no tool call yet, turn doesn't land.
        let engine1 = MockEngine::new(vec![AcpUpdate::TextDelta(
            "That thread about forgetting — say what just set.".into(),
        )]);
        let mut seen1 = Vec::new();
        conductor
            .run_turn(
                &engine1,
                Some("Forgetting costs energy, doesn't it?"),
                &mut |u| seen1.push(u),
            )
            .await
            .unwrap();
        assert!(!conductor.landed(), "no TurnComplete yet");
        assert!(conductor.tools_called().is_empty());

        // Turn 2: the learner condenses; the engine fixes salt and completes.
        let engine2 = MockEngine::new(vec![
            AcpUpdate::TextDelta(" Yes — erasure is dissipation.".into()),
            AcpUpdate::ToolCall(AcpToolCall {
                id: "1".into(),
                name: "fix_salt".into(),
                args: json!({
                    "realization": "forgetting costs energy — erasure is dissipation",
                    "thread_id": parent.id,
                    "domains": ["thermodynamics"]
                }),
            }),
            AcpUpdate::TurnComplete,
        ]);
        let mut seen2 = Vec::new();
        conductor
            .run_turn(&engine2, Some("Say more."), &mut |u| seen2.push(u))
            .await
            .unwrap();

        assert!(conductor.landed(), "TurnComplete should land the session");
        assert_eq!(conductor.tools_called(), &["fix_salt".to_string()]);
        // transcript accumulates across BOTH turns.
        assert!(conductor.transcript().contains("say what just set"));
        assert!(conductor.transcript().contains("erasure is dissipation"));

        let session_id = conductor.session_id().to_string();
        let outcome = conductor.close(DEFAULT_SESSION_MINUTES).unwrap();
        assert_eq!(outcome.session_id, session_id);
        assert!(outcome.landed);
        assert_eq!(outcome.tools_called, vec!["fix_salt".to_string()]);

        // The store's transcript column matches what streamed.
        let stored_session = store.get_session(&session_id).unwrap();
        assert_eq!(stored_session.transcript, outcome.transcript);
        assert_eq!(stored_session.state, "closed");

        // wisdom advanced — the session landed.
        assert_eq!(store.wisdom_days().unwrap(), 1);

        // spiral: the fixed parent birthed exactly one volatile, back-linked
        // child thread.
        let ripe = store.ripe_threads(16).unwrap();
        let children: Vec<_> = ripe
            .iter()
            .filter(|t| t.parent_realization_id.is_some())
            .collect();
        assert_eq!(children.len(), 1, "one spiral child thread exists");
        assert_eq!(children[0].state, ThreadState::Volatile);
        let rid = children[0].parent_realization_id.clone().unwrap();
        let realization = store.get_realization(&rid).unwrap();
        assert!(realization.text.contains("erasure is dissipation"));
        assert!(store.try_mutate_realization(&rid, "tampered").is_err());

        // SALT kindled.
        assert!(store.kindled().unwrap().contains(&"SALT".to_string()));

        // parent thread condensed all the way to Fixed, out of the ripe pool.
        assert!(!ripe.iter().any(|t| t.id == parent.id));

        // trace written at close, non-empty, single line.
        let trace = store.last_trace().unwrap().unwrap();
        assert!(!trace.is_empty());
        assert!(!trace.contains('\n'));
        assert!(trace.contains("erasure is dissipation"));
    }

    #[tokio::test]
    async fn abandon_returns_thread_to_volatile_and_writes_no_trace() {
        let store = store_arc();
        let thread = store.open_thread("why?", None, None).unwrap();
        store
            .set_thread_state(&thread.id, ThreadState::Condensing)
            .unwrap();

        let mut conductor = Conductor::begin(
            Arc::clone(&store),
            "philosophus",
            "explain",
            Some(&thread.id),
        )
        .unwrap();
        let engine = MockEngine::new(vec![AcpUpdate::TextDelta("only halfway...".into())]);
        conductor
            .run_turn(&engine, Some("go on"), &mut |_| {})
            .await
            .unwrap();
        assert!(!conductor.landed());

        conductor.abandon().unwrap();

        let reloaded = store.get_thread(&thread.id).unwrap();
        assert_eq!(reloaded.state, ThreadState::Volatile);
        assert_eq!(store.last_trace().unwrap(), None, "abandon writes no trace");
    }

    #[tokio::test]
    async fn multi_turn_prompt_reassembly_includes_accumulated_turns_deterministically() {
        let store = store_arc();
        let mut conductor =
            Conductor::begin(Arc::clone(&store), "adamas", "challenge", None).unwrap();

        let e1 = MockEngine::new(vec![AcpUpdate::TextDelta("first reply".into())]);
        conductor
            .run_turn(&e1, Some("first learner turn"), &mut |_| {})
            .await
            .unwrap();
        assert_eq!(conductor.user_turns, vec!["first learner turn".to_string()]);

        // Re-assembling now (before the second run_turn call) must already
        // reflect the first accumulated turn.
        let prompt_after_one = conductor.assemble_prompt();
        assert_eq!(
            prompt_after_one.user_turns,
            vec!["first learner turn".to_string()]
        );

        let e2 = MockEngine::new(vec![AcpUpdate::TextDelta("second reply".into())]);
        conductor
            .run_turn(&e2, Some("second learner turn"), &mut |_| {})
            .await
            .unwrap();

        let prompt_after_two = conductor.assemble_prompt();
        assert_eq!(
            prompt_after_two.user_turns,
            vec![
                "first learner turn".to_string(),
                "second learner turn".to_string()
            ]
        );
        // deterministic: re-assembling again without a new turn is identical.
        assert_eq!(prompt_after_two, conductor.assemble_prompt());
    }

    #[tokio::test]
    async fn begin_initiation_assembles_the_cold_start_prompt() {
        let store = store_arc();
        let mut conductor = Conductor::begin_initiation(Arc::clone(&store)).unwrap();
        let prompt = conductor.assemble_prompt();
        assert!(prompt.system.contains("Initiation — the First Session"));

        let engine = MockEngine::new(vec![
            AcpUpdate::TextDelta("Welcome.".into()),
            AcpUpdate::TurnComplete,
        ]);
        conductor
            .run_turn(&engine, None, &mut |_| {})
            .await
            .unwrap();
        assert!(conductor.landed());
        let outcome = conductor.close(DEFAULT_SESSION_MINUTES).unwrap();
        assert!(outcome.transcript.contains("Welcome."));
    }
}
