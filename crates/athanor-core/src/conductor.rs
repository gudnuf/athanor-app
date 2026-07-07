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

use crate::engine::{AcpPrompt, AcpRole, AcpTurn, AcpUpdate, EngineError, MystagogueEngine};
use crate::error::CoreError;
use crate::mask::{self, SharedMask};
use crate::mystagogue::Mystagogue;
use crate::prompt;
use crate::register::RegisterParser;
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
    /// The session's live `(mask, mode)` register — the shared cell the
    /// `shift_mask` tool moves and the FFI header/pin read (lane 13). Assembled
    /// under, fresh, on every turn, so a mid-session shift takes effect the next
    /// turn for free.
    mask_state: SharedMask,
    /// Whether this is the initiation session (assembles the cold-start prompt,
    /// ignores the mask/mode register). Fixed at construction.
    is_initiation: bool,
    thread_id: Option<String>,
    /// The full dialogue so far, oldest first, both sides — re-sent (via a
    /// freshly-assembled prompt) on every subsequent `run_turn` so a
    /// multi-turn session's prompt deterministically includes everything said
    /// so far, by BOTH the learner and the Mystagogue (SHOULD-FIX-4: without
    /// the engine's own prior replies in here, turn 3 of a live session can
    /// repeat or contradict turn 1's framing, because it never saw it).
    turns: Vec<AcpTurn>,
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
        let is_initiation = mask == INITIATION_MASK && mode == INITIATION_MODE;
        // The session's live register, shared with the Mystagogue's shift_mask
        // tool + the FFI header/pin. Seeded at the opening (mask, mode).
        let mask_state = mask::shared(&mask, &mode);
        // Hand the Mystagogue the session's focal thread (fix_salt fallback) and
        // the shared mask cell + id (so shift_mask can move + persist the
        // register mid-session).
        let mystagogue = Mystagogue::new(Arc::clone(&store))
            .with_focal_thread(thread_id.clone())
            .with_mask_state(Arc::clone(&mask_state), session_id.clone());
        Self {
            store,
            mystagogue,
            session_id,
            mask_state,
            is_initiation,
            thread_id,
            turns: Vec::new(),
            transcript: String::new(),
            tools_called: Vec::new(),
            landed: false,
        }
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// The session's shared `(mask, mode, pinned)` cell — handed to the FFI
    /// bridge so it can surface the honest header and pin the escape hatch.
    pub fn mask_state(&self) -> SharedMask {
        Arc::clone(&self.mask_state)
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
        self.is_initiation
    }

    /// Assembles the prompt fresh from current store state + the accumulated
    /// dialogue history (both sides — see `turns`). Called at the top of
    /// every `run_turn`/`open_turn` so later turns see whatever the session
    /// (or a prior turn's tool calls) has changed — including a mid-session
    /// `shift_mask`, since the `(mask, mode)` is read fresh from the shared cell
    /// here every turn. `assemble`/`assemble_initiation` are pure over store
    /// state, so this stays deterministic given a fixed store + turn history +
    /// register.
    fn assemble_prompt(&self) -> AcpPrompt {
        let plan = if self.is_initiation() {
            prompt::assemble_initiation(&self.store)
        } else {
            let (mask, mode) = mask::current(&self.mask_state);
            prompt::assemble(&mask, &mode, self.thread_id.as_deref(), &self.store)
        };
        AcpPrompt {
            system: plan.system_prompt,
            turns: self.turns.clone(),
            tools: Mystagogue::tool_specs(),
        }
    }

    /// Runs one turn: appends `incoming` (if any) to the accumulated turn
    /// history, re-assembles the prompt (so it includes that turn and
    /// everything before it, on both sides), then drives `engine.run_turn`
    /// against the Mystagogue's tool dispatch, streaming every `AcpUpdate` to
    /// `sink` as it arrives. Assistant text is appended to the store's
    /// transcript column, to the conductor's own accumulator, AND to `turns`
    /// (as a `Mystagogue` turn) so the *next* `run_turn` re-seeds it — this is
    /// the SHOULD-FIX-4 fix: the engine sees its own prior replies, not just
    /// the learner's turns. Tool names and the landed flag accumulate across
    /// calls (`tools_called`/`landed` reflect the whole session, not just
    /// this turn).
    async fn run_turn_inner(
        &mut self,
        engine: &dyn MystagogueEngine,
        incoming: Option<AcpTurn>,
        sink: &mut (dyn FnMut(AcpUpdate) + Send),
    ) -> Result<(), ConductorError> {
        if let Some(turn) = incoming {
            self.turns.push(turn);
        }
        let acp_prompt = self.assemble_prompt();

        let mut turn_text = String::new();
        let mut turn_landed = false;
        let tools_called = &mut self.tools_called;
        // The register parser strips the model's reading-voice markers and tags
        // each emitted delta with its register (identity.md §6). It runs HERE,
        // between the raw engine stream and the caller's sink, so markers never
        // leak — not to the caller, not to the accumulated transcript/history.
        let mut register = RegisterParser::default();

        engine
            .run_turn(acp_prompt, &self.mystagogue, &mut |update| match update {
                AcpUpdate::TextDelta { text, .. } => {
                    register.push(&text, &mut |chunk, reg| {
                        turn_text.push_str(&chunk);
                        sink(AcpUpdate::TextDelta {
                            text: chunk,
                            register: reg,
                        });
                    });
                }
                AcpUpdate::ToolCall(call) => {
                    tools_called.push(call.name.clone());
                    sink(AcpUpdate::ToolCall(call));
                }
                // A tool's return value — nothing to accumulate here; it flows
                // straight through to the bridge's sink.
                AcpUpdate::ToolResult(result) => sink(AcpUpdate::ToolResult(result)),
                AcpUpdate::TurnComplete => {
                    // Flush any held-back tail BEFORE TurnComplete so trailing
                    // reading-voice text lands in this turn, not after its end.
                    register.flush(&mut |chunk, reg| {
                        turn_text.push_str(&chunk);
                        sink(AcpUpdate::TextDelta {
                            text: chunk,
                            register: reg,
                        });
                    });
                    turn_landed = true;
                    sink(AcpUpdate::TurnComplete);
                }
            })
            .await?;

        // Safety net for a turn that never reached TurnComplete (e.g. a scripted
        // single-turn run): flush whatever the parser still holds. A no-op when
        // the TurnComplete arm above already drained it.
        register.flush(&mut |chunk, reg| {
            turn_text.push_str(&chunk);
            sink(AcpUpdate::TextDelta {
                text: chunk,
                register: reg,
            });
        });

        if !turn_text.is_empty() {
            self.store.append_transcript(&self.session_id, &turn_text)?;
            self.turns.push(AcpTurn {
                role: AcpRole::Mystagogue,
                text: turn_text.clone(),
            });
        }
        self.transcript.push_str(&turn_text);
        self.landed = turn_landed;

        Ok(())
    }

    /// Runs one turn from the learner's side: appends `learner_turn` (if any)
    /// to the accumulated turn history, then drives it through the engine —
    /// see `run_turn_inner` for the shared mechanics.
    ///
    /// Call this once for a single-turn session, or repeatedly — passing the
    /// next learner turn each time — for a multi-turn one. Passing `None` is
    /// only meaningful when `turns` is already non-empty (re-running against
    /// accumulated history with no new learner input); an ordinary cold
    /// open with nothing said yet should use `open_turn` instead, since the
    /// `GooseEngine` requires the prompt to end on a learner turn.
    pub async fn run_turn(
        &mut self,
        engine: &dyn MystagogueEngine,
        learner_turn: Option<&str>,
        sink: &mut (dyn FnMut(AcpUpdate) + Send),
    ) -> Result<(), ConductorError> {
        let incoming = learner_turn.map(|text| AcpTurn {
            role: AcpRole::Learner,
            text: text.to_string(),
        });
        self.run_turn_inner(engine, incoming, sink).await
    }

    /// Runs the ritual opening turn (BLOCKER-1 deep fix): the Mystagogue
    /// speaks first, with no real learner utterance yet to answer. Initiation
    /// is the one flow with no first-speaker channel otherwise — a real
    /// session would just sit silent until the learner broke the ice, and the
    /// `GooseEngine` errors on an empty turn history regardless (`agent.reply`
    /// only accepts a user-authored message).
    ///
    /// The fix is honest about what's actually happening: rather than inject
    /// a hardcoded string here, this seeds the ONE synthesized turn defined in
    /// the versioned prompt pack itself (`initiation.md`'s
    /// `<!-- ritual-opening-turn: ... -->` marker, `prompt::assets::
    /// initiation_opening_turn`) — visible to anyone reading the prompt pack,
    /// and exercisable by the eval personas the same way any other turn is.
    /// Call this once, before any `run_turn`, when opening a session with
    /// nothing said yet (in practice: `begin_initiation`).
    pub async fn open_turn(
        &mut self,
        engine: &dyn MystagogueEngine,
        sink: &mut (dyn FnMut(AcpUpdate) + Send),
    ) -> Result<(), ConductorError> {
        let opening = AcpTurn {
            role: AcpRole::Learner,
            text: prompt::assets::initiation_opening_turn().to_string(),
        };
        self.run_turn_inner(engine, Some(opening), sink).await
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
        // Completing the initiation lights the Furnace (Tabula passage I): the
        // learner has begun — the first ember that is their own. Derived from
        // the existing initiation-close event, no new Mystagogue tool or
        // user-facing mechanic (mirrors how `fix_salt` kindles SALT). Kindling
        // is first-wins, so re-initiation never re-fires it.
        if self.is_initiation() {
            self.store.kindle_passage("FURNACE", None)?;
        }
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
        let engine1 = MockEngine::new(vec![AcpUpdate::text_delta(
            "That thread about forgetting — say what just set.",
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
            AcpUpdate::text_delta(" Yes — erasure is dissipation."),
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

    /// A reply carrying a reading-voice passage: the conductor must strip the
    /// markers (they never reach the sink or the transcript) and tag the passage
    /// as `Register::Reading`, with the surrounding text staying `Quick`. Split
    /// across deltas to prove the parser holds partial markers across chunks.
    #[tokio::test]
    async fn reading_passage_is_register_tagged_and_markers_stripped() {
        use crate::engine::Register;

        let store = store_arc();
        let mut conductor =
            Conductor::begin(Arc::clone(&store), "philosophus", "explain", None).unwrap();

        // The open marker is split across two deltas on purpose.
        let engine = MockEngine::new(vec![
            AcpUpdate::text_delta("A quick nudge. <!--rea"),
            AcpUpdate::text_delta(
                "ding-->A measured lesson, laid down.<!--/reading--> Back to it.",
            ),
            AcpUpdate::TurnComplete,
        ]);

        let mut deltas: Vec<(String, Register)> = Vec::new();
        conductor
            .run_turn(&engine, Some("Teach me."), &mut |u| {
                if let AcpUpdate::TextDelta { text, register } = u {
                    deltas.push((text, register));
                }
            })
            .await
            .unwrap();

        assert_eq!(
            deltas,
            vec![
                ("A quick nudge. ".to_string(), Register::Quick),
                (
                    "A measured lesson, laid down.".to_string(),
                    Register::Reading
                ),
                (" Back to it.".to_string(), Register::Quick),
            ],
            "reading passage tagged, quick around it, markers stripped even when split"
        );

        // Neither the streamed text nor the stored transcript ever sees a marker.
        let joined: String = deltas.iter().map(|(t, _)| t.as_str()).collect();
        assert!(!joined.contains("<!--"), "no marker leaks to the sink");
        assert!(
            !conductor.transcript().contains("<!--"),
            "no marker leaks into the accumulated transcript"
        );
        assert!(conductor
            .transcript()
            .contains("A measured lesson, laid down."));
    }

    /// Lane 13: the Mystagogue shifts the mask mid-session via `shift_mask`; the
    /// shift lands on the shared cell during the turn and the NEXT assemble runs
    /// under the new register (the Conductor reads the cell fresh each turn).
    #[tokio::test]
    async fn shift_mask_mid_session_reassembles_under_the_new_register_next_turn() {
        let store = store_arc();
        let mut conductor =
            Conductor::begin(Arc::clone(&store), "philosophus", "explain", None).unwrap();
        assert_eq!(
            mask::current(&conductor.mask_state()),
            ("philosophus".into(), "explain".into())
        );

        // Turn 1: the model shifts to adamas/challenge as the moment calls for it.
        let engine = MockEngine::new(vec![
            AcpUpdate::text_delta("Let's press this harder."),
            AcpUpdate::ToolCall(AcpToolCall {
                id: "1".into(),
                name: "shift_mask".into(),
                args: json!({ "mask": "adamas", "mode": "challenge" }),
            }),
            AcpUpdate::TurnComplete,
        ]);
        conductor
            .run_turn(&engine, Some("Push me."), &mut |_| {})
            .await
            .unwrap();

        // The shared cell moved…
        assert_eq!(
            mask::current(&conductor.mask_state()),
            ("adamas".into(), "challenge".into()),
            "shift_mask moved the live register"
        );
        // …and the next assemble is byte-identical to a fresh adamas/challenge
        // assembly (proving the Conductor reads the register fresh each turn),
        // and different from the philosophus/explain it opened under.
        let next = conductor.assemble_prompt();
        let adamas = prompt::assemble("adamas", "challenge", None, &store);
        let philosophus = prompt::assemble("philosophus", "explain", None, &store);
        assert_eq!(next.system, adamas.system_prompt);
        assert_ne!(next.system, philosophus.system_prompt);
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
        let engine = MockEngine::new(vec![AcpUpdate::text_delta("only halfway...")]);
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

        let e1 = MockEngine::new(vec![AcpUpdate::text_delta("first reply")]);
        conductor
            .run_turn(&e1, Some("first learner turn"), &mut |_| {})
            .await
            .unwrap();
        assert_eq!(
            conductor.turns,
            vec![
                AcpTurn {
                    role: AcpRole::Learner,
                    text: "first learner turn".to_string()
                },
                AcpTurn {
                    role: AcpRole::Mystagogue,
                    text: "first reply".to_string()
                },
            ],
            "the Mystagogue's own reply must be accumulated alongside the learner's turn \
             (SHOULD-FIX-4) so the next run_turn re-seeds both"
        );

        // Re-assembling now (before the second run_turn call) must already
        // reflect the first accumulated exchange, both sides.
        let prompt_after_one = conductor.assemble_prompt();
        assert_eq!(prompt_after_one.turns, conductor.turns);

        let e2 = MockEngine::new(vec![AcpUpdate::text_delta("second reply")]);
        conductor
            .run_turn(&e2, Some("second learner turn"), &mut |_| {})
            .await
            .unwrap();

        let prompt_after_two = conductor.assemble_prompt();
        assert_eq!(
            prompt_after_two.turns,
            vec![
                AcpTurn {
                    role: AcpRole::Learner,
                    text: "first learner turn".to_string()
                },
                AcpTurn {
                    role: AcpRole::Mystagogue,
                    text: "first reply".to_string()
                },
                AcpTurn {
                    role: AcpRole::Learner,
                    text: "second learner turn".to_string()
                },
                AcpTurn {
                    role: AcpRole::Mystagogue,
                    text: "second reply".to_string()
                },
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
        assert!(
            prompt.turns.is_empty(),
            "no turn has run yet — open_turn is what seeds the ritual opening"
        );

        let engine = MockEngine::new(vec![
            AcpUpdate::text_delta("Welcome."),
            AcpUpdate::TurnComplete,
        ]);
        conductor.open_turn(&engine, &mut |_| {}).await.unwrap();
        assert!(conductor.landed());
        assert_eq!(
            conductor.turns,
            vec![
                AcpTurn {
                    role: AcpRole::Learner,
                    text: prompt::assets::initiation_opening_turn().to_string(),
                },
                AcpTurn {
                    role: AcpRole::Mystagogue,
                    text: "Welcome.".to_string(),
                },
            ],
            "open_turn should seed the versioned ritual-opening marker as the driving \
             turn, then accumulate the Mystagogue's reply alongside it"
        );
        let outcome = conductor.close(DEFAULT_SESSION_MINUTES).unwrap();
        assert!(outcome.transcript.contains("Welcome."));
    }

    #[tokio::test]
    async fn closing_the_initiation_lights_the_furnace_passage() {
        let store = store_arc();
        let mut conductor = Conductor::begin_initiation(Arc::clone(&store)).unwrap();
        let engine = MockEngine::new(vec![
            AcpUpdate::text_delta("Welcome."),
            AcpUpdate::TurnComplete,
        ]);
        conductor.open_turn(&engine, &mut |_| {}).await.unwrap();

        // Before close, the Furnace is cold.
        assert!(
            !store
                .tabula()
                .unwrap()
                .iter()
                .any(|p| p.key == "FURNACE" && p.kindled),
            "the Furnace passage is dim until initiation completes"
        );

        conductor.close(DEFAULT_SESSION_MINUTES).unwrap();

        assert!(
            store.kindled().unwrap().contains(&"FURNACE".to_string()),
            "completing initiation kindled FURNACE"
        );
        let furnace = store
            .tabula()
            .unwrap()
            .into_iter()
            .find(|p| p.key == "FURNACE")
            .unwrap();
        assert!(furnace.kindled, "the Furnace passage (I) is now lit");
        assert!(furnace.kindled_note.is_some());
    }

    #[tokio::test]
    async fn closing_an_ordinary_session_does_not_light_the_furnace() {
        let store = store_arc();
        let mut conductor =
            Conductor::begin(Arc::clone(&store), "philosophus", "explain", None).unwrap();
        let engine = MockEngine::new(vec![
            AcpUpdate::text_delta("Say more."),
            AcpUpdate::TurnComplete,
        ]);
        conductor
            .run_turn(&engine, Some("a thought"), &mut |_| {})
            .await
            .unwrap();
        conductor.close(DEFAULT_SESSION_MINUTES).unwrap();
        assert!(
            !store.kindled().unwrap().contains(&"FURNACE".to_string()),
            "only the initiation lights the Furnace, not an ordinary session"
        );
    }

    /// A second `run_turn` after `open_turn` must re-seed the ritual opening
    /// AND the Mystagogue's first reply — a real initiation is a multi-turn
    /// dialogue, and this is exactly the SHOULD-FIX-4 path applied to it.
    #[tokio::test]
    async fn open_turn_then_run_turn_accumulates_both_sides() {
        let store = store_arc();
        let mut conductor = Conductor::begin_initiation(Arc::clone(&store)).unwrap();

        let opening_engine = MockEngine::new(vec![
            AcpUpdate::text_delta("What's been pulling at you?"),
            AcpUpdate::TurnComplete,
        ]);
        conductor
            .open_turn(&opening_engine, &mut |_| {})
            .await
            .unwrap();

        let reply_engine = MockEngine::new(vec![
            AcpUpdate::text_delta("Say more about that."),
            AcpUpdate::TurnComplete,
        ]);
        conductor
            .run_turn(
                &reply_engine,
                Some("I keep circling back to why iron forgets."),
                &mut |_| {},
            )
            .await
            .unwrap();

        assert_eq!(
            conductor.turns,
            vec![
                AcpTurn {
                    role: AcpRole::Learner,
                    text: prompt::assets::initiation_opening_turn().to_string(),
                },
                AcpTurn {
                    role: AcpRole::Mystagogue,
                    text: "What's been pulling at you?".to_string(),
                },
                AcpTurn {
                    role: AcpRole::Learner,
                    text: "I keep circling back to why iron forgets.".to_string(),
                },
                AcpTurn {
                    role: AcpRole::Mystagogue,
                    text: "Say more about that.".to_string(),
                },
            ]
        );
    }

    /// Live coherence check (SHOULD-FIX-4's real proof): a real 2-turn
    /// session through the actual `GooseEngine`. Ignored by default; run
    /// explicitly with a key:
    /// `ANTHROPIC_API_KEY=… cargo test -p athanor-core --features goose -- --ignored`.
    /// Skips gracefully (passes) when the key is unset. This does not assert
    /// on the second reply's content (no LLM judge) — it prints the full
    /// transcript so a human can eyeball whether turn 2 is contextually
    /// continuous with turn 1, which `cargo test -- --ignored --nocapture`
    /// surfaces directly.
    #[cfg(feature = "goose")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore]
    async fn live_two_turn_session_stays_coherent_across_turns() {
        let Ok(key) = std::env::var("ANTHROPIC_API_KEY") else {
            eprintln!("ANTHROPIC_API_KEY unset — skipping live test");
            return;
        };

        let store = store_arc();
        let mut conductor =
            Conductor::begin(Arc::clone(&store), "philosophus", "explain", None).unwrap();
        let engine = crate::engine::GooseEngine::new(key, None);

        let mut turn1 = String::new();
        conductor
            .run_turn(
                &engine,
                Some(
                    "Remember this exact detail for later: my favorite made-up word for \
                     entropy is 'grumblewarp'. Just acknowledge it in one short sentence.",
                ),
                &mut |u| {
                    if let AcpUpdate::TextDelta { text: t, .. } = u {
                        turn1.push_str(&t);
                    }
                },
            )
            .await
            .expect("turn 1 should succeed");
        println!("--- turn 1 (Mystagogue) ---\n{turn1}");
        assert!(!turn1.is_empty(), "turn 1 must stream some reply text");

        let mut turn2 = String::new();
        conductor
            .run_turn(
                &engine,
                Some("What was that made-up word again? Just say the word."),
                &mut |u| {
                    if let AcpUpdate::TextDelta { text: t, .. } = u {
                        turn2.push_str(&t);
                    }
                },
            )
            .await
            .expect("turn 2 should succeed");
        println!("--- turn 2 (Mystagogue) ---\n{turn2}");

        // The real proof: turn 2 must reference what was established in turn
        // 1 — before this fix, the engine never saw its own turn-1 reply, so
        // the ONLY way it could get this right is if the coined word also
        // happens to still be in the learner-turns-only history (it is, in
        // the learner's own turn 1 — so this assertion is a floor, not a
        // ceiling; the real proof is in the printed transcript above:
        // turn 1 should show the Mystagogue actually engaging with/echoing
        // 'grumblewarp' in its own words, which requires assistant history).
        assert!(
            turn2.to_lowercase().contains("grumblewarp"),
            "turn 2 should recall the coined word from the exchange: {turn2:?}"
        );
    }
}
