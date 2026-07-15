//! Hermetic end-to-end test for the FFI bridge (Plan Phase 4, C1 + C2). Drives
//! a real `AthanorEngine` (test-injected `MockEngine` — no network, no key)
//! through:
//! - a scripted turn that fixes salt, asserting the ordered `SessionEvent`
//!   stream *including a bridge-synthesized `Condensation`*;
//! - each of the four read projections round-tripping the seeded state;
//! - an `abandon` returning its thread to volatile.

use std::sync::{Arc, Mutex};

use athanor_core::domain::ThreadState;
use athanor_core::engine::{
    AcpPrompt, AcpToolCall, AcpUpdate, EngineError, MockEngine, MystagogueEngine, ToolDispatch,
};
use athanor_core::Store;
use ffi::engine::AthanorEngine;
use ffi::events::{ReplyRegister, SessionEvent, SessionEventListener};
use serde_json::json;

/// Collects every `SessionEvent` the bridge emits so the test can assert order.
struct Collector(Mutex<Vec<SessionEvent>>);

impl SessionEventListener for Collector {
    fn on_event(&self, event: SessionEvent) {
        self.0.lock().unwrap().push(event);
    }
}

#[tokio::test]
async fn scripted_turn_fixes_salt_streams_condensation_then_reads_round_trip() {
    let store = Arc::new(Store::open_in_memory("dev").unwrap());
    let domain = store.upsert_domain("thermodynamics").unwrap();
    let parent = store
        .open_thread("does forgetting cost energy?", Some(&domain.id), None)
        .unwrap();

    // A scripted turn: reflect, fix salt against the parent thread, complete.
    let mock = MockEngine::new(vec![
        AcpUpdate::text_delta("Erasure is dissipation."),
        AcpUpdate::ToolCall(AcpToolCall {
            id: "1".into(),
            name: "fix_salt".into(),
            args: json!({
                "realization": "forgetting costs energy — erasure is dissipation",
                "thread_id": parent.id,
                "domains": ["thermodynamics"],
            }),
        }),
        AcpUpdate::TurnComplete,
    ]);

    let engine = AthanorEngine::with_engine(Arc::clone(&store), Arc::new(mock));

    let session = engine
        .begin_session(
            Some("philosophus".into()),
            Some("explain".into()),
            Some(parent.id.clone()),
        )
        .unwrap();

    let collector = Arc::new(Collector(Mutex::new(Vec::new())));
    session.set_listener(collector.clone());
    session
        .send_turn("Forgetting costs energy, doesn't it?".into())
        .await;

    // Ordered event stream: the opening register (lane 13), then delta, tool
    // call, synthesized condensation, then the turn completes — condensation
    // strictly BEFORE TurnComplete.
    let events = collector.0.lock().unwrap().clone();
    assert_eq!(
        events.len(),
        5,
        "mask, delta, toolcall, condensation, complete: {events:?}"
    );
    assert!(
        matches!(&events[0], SessionEvent::MaskShifted { mask, mode }
            if mask == "philosophus" && mode == "explain"),
        "first event surfaces the opening register: {:?}",
        events[0]
    );
    assert!(
        matches!(&events[1], SessionEvent::TextDelta { text, register }
            if text.contains("dissipation") && *register == ReplyRegister::Quick),
        "second event is a quick-register text delta: {:?}",
        events[1]
    );
    assert!(
        matches!(&events[2], SessionEvent::ToolCall { kind } if kind == "fix_salt"),
        "third event is the fix_salt tool call: {:?}",
        events[2]
    );
    let condensation_rid = match &events[3] {
        SessionEvent::Condensation {
            realization_id,
            child_thread_id,
            text,
        } => {
            assert!(
                !realization_id.is_empty(),
                "condensation carries a realization id"
            );
            assert!(
                child_thread_id.is_some(),
                "fix_salt births a spiral child thread"
            );
            // The moment now carries the REAL fixed salt's text (from the
            // fix_salt ToolResult's id), not a guess.
            assert!(
                text.contains("erasure is dissipation"),
                "condensation carries the fixed salt text: {text:?}"
            );
            realization_id.clone()
        }
        other => panic!("fourth event must be the condensation moment: {other:?}"),
    };
    assert!(
        matches!(events[4], SessionEvent::TurnComplete),
        "condensation precedes TurnComplete: {:?}",
        events[4]
    );

    // Land the session so tending is recorded (the only place wisdom advances).
    session.close(15).await.unwrap();

    // --- read projections round-trip the seeded state ---

    let furnace = engine.furnace_state().unwrap();
    assert_eq!(furnace.wisdom_days, 1, "the landed session advanced wisdom");
    assert!(furnace.tended_today, "the fire was fed today");
    assert_eq!(furnace.recent.len(), 1);
    assert_eq!(furnace.recent[0].minutes, 15);

    let grimoire = engine.grimoire().unwrap();
    assert_eq!(grimoire.len(), 1, "one grain of salt was fixed");
    assert_eq!(
        grimoire[0].id, condensation_rid,
        "condensation id matches the grimoire grain"
    );
    assert!(grimoire[0].text.contains("erasure is dissipation"));
    assert_eq!(grimoire[0].domains, vec!["thermodynamics".to_string()]);
    assert!(
        grimoire[0].child_thread_id.is_some(),
        "the grain carries its spiral link"
    );

    let mercury = engine.mercury().unwrap();
    // The parent thread condensed to Fixed (out of the open pool); its spiral
    // child is Volatile (open). So exactly one open thread, and it's the child.
    assert_eq!(
        mercury.len(),
        1,
        "the parent fixed; only its volatile child stays open"
    );
    assert_eq!(mercury[0].state, "volatile");
    assert_eq!(
        mercury[0].domain_name.as_deref(),
        Some("thermodynamics"),
        "mercury projects the domain's human NAME (resolved from id), not a raw id"
    );
    assert_eq!(
        mercury[0].parent_realization_id.as_deref(),
        Some(condensation_rid.as_str()),
        "the open thread is the spiral child of the fixed realization"
    );
    assert!(
        !mercury.iter().any(|t| t.id == parent.id),
        "the parent left the open pool"
    );

    // The Tabula now projects the seven canonical passages (rich shape), not
    // raw kindled keys. Fixing salt kindled SALT, which lights the Grimoire
    // passage (with its note); the scroll still renders all seven in order.
    let tabula = engine.tabula().unwrap();
    assert_eq!(tabula.len(), 7, "all seven passages render: {tabula:?}");
    assert_eq!(
        tabula.iter().map(|p| p.number.as_str()).collect::<Vec<_>>(),
        ["I", "II", "III", "IV", "V", "VI", "VII"],
        "scroll order I→VII"
    );
    let grimoire = tabula.iter().find(|p| p.key == "GRIMOIRE").unwrap();
    assert!(grimoire.kindled, "fixing salt lit the Grimoire: {tabula:?}");
    assert_eq!(
        grimoire.kindled_note.as_deref(),
        Some("the Grimoire began writing itself")
    );
    assert!(
        !grimoire.title.is_empty() && !grimoire.body.is_empty(),
        "passages carry their canonical content"
    );
    assert!(
        tabula
            .iter()
            .find(|p| p.key == "WORLD")
            .unwrap()
            .kindled_note
            .is_none(),
        "a dim passage carries no note"
    );
}

/// BLOCKER-1 deep fix: `begin_initiation` + `open()` runs the ritual opening
/// turn — the Mystagogue speaks first, streaming a reply before any
/// `send_turn` call, with no demo-string or tap-triggered kickoff involved.
#[tokio::test]
async fn begin_initiation_open_streams_the_mystagogues_first_reply_with_no_learner_input() {
    let store = Arc::new(Store::open_in_memory("dev").unwrap());

    let mock = MockEngine::new(vec![
        AcpUpdate::text_delta("Before anything else — what's been pulling at you?"),
        AcpUpdate::TurnComplete,
    ]);
    let engine = AthanorEngine::with_engine(Arc::clone(&store), Arc::new(mock));

    let session = engine.begin_initiation().unwrap();
    let collector = Arc::new(Collector(Mutex::new(Vec::new())));
    session.set_listener(collector.clone());

    // No send_turn call happens before this — open() is the ONLY thing that
    // drives the first turn.
    session.open().await;

    let events = collector.0.lock().unwrap().clone();
    assert_eq!(events.len(), 3, "mask, delta, complete: {events:?}");
    assert!(
        matches!(&events[0], SessionEvent::MaskShifted { mask, mode }
            if mask == "initiation" && mode == "initiation"),
        "the opening register is surfaced first: {:?}",
        events[0]
    );
    assert!(
        matches!(&events[1], SessionEvent::TextDelta { text, .. }
            if text.contains("pulling at you")),
        "the Mystagogue's opening line streams with no learner turn preceding it: {:?}",
        events[1]
    );
    assert!(matches!(events[2], SessionEvent::TurnComplete));

    session.close(15).await.unwrap();
}

/// Lane 13 escape hatch, end-to-end: the learner pins a mask, the header truth
/// updates, and a subsequent model `shift_mask` no-ops (the pin wins).
#[tokio::test]
async fn pinning_a_mask_holds_it_against_a_later_shift_mask() {
    let store = Arc::new(Store::open_in_memory("dev").unwrap());
    store
        .open_thread("why does iron remember?", None, None)
        .unwrap();

    // A turn where the model tries to shift to solve.
    let mock = MockEngine::new(vec![
        AcpUpdate::text_delta("Staying with it."),
        AcpUpdate::ToolCall(AcpToolCall {
            id: "1".into(),
            name: "shift_mask".into(),
            args: json!({ "mask": "solve" }),
        }),
        AcpUpdate::TurnComplete,
    ]);
    let engine = AthanorEngine::with_engine(Arc::clone(&store), Arc::new(mock));
    let session = engine.begin_session(None, None, None).unwrap();
    assert_eq!(
        session.current_mask(),
        "philosophus",
        "opens on the default"
    );

    let collector = Arc::new(Collector(Mutex::new(Vec::new())));
    session.set_listener(collector.clone());

    // The learner pins adamas via the header escape hatch.
    session.pin_mask("adamas".into());
    assert_eq!(session.current_mask(), "adamas");
    assert!(
        collector
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|e| matches!(e, SessionEvent::MaskShifted { mask, .. } if mask == "adamas")),
        "pinning surfaces the choice to the header at once"
    );

    // The model tries to shift to solve — the pin holds.
    session.send_turn("push".into()).await;
    assert_eq!(
        session.current_mask(),
        "adamas",
        "the pin wins: shift_mask no-ops while pinned"
    );

    session.close(15).await.unwrap();
}

#[tokio::test]
async fn abandon_returns_the_thread_to_volatile() {
    let store = Arc::new(Store::open_in_memory("dev").unwrap());
    let thread = store
        .open_thread("why does the fire cool?", None, None)
        .unwrap();
    store
        .set_thread_state(&thread.id, ThreadState::Condensing)
        .unwrap();

    // No turn is driven, so the engine script is irrelevant.
    let engine = AthanorEngine::with_engine(Arc::clone(&store), Arc::new(MockEngine::new(vec![])));

    let session = engine
        .begin_session(None, None, Some(thread.id.clone()))
        .unwrap();
    session.abandon().await.unwrap();

    let reloaded = store.get_thread(&thread.id).unwrap();
    assert_eq!(
        reloaded.state,
        ThreadState::Volatile,
        "abandon returns the working thread to the volatile pool"
    );
    assert_eq!(store.last_trace().unwrap(), None, "abandon writes no trace");
}

/// An engine that plays a DIFFERENT scripted reply per `run_turn` call (unlike
/// `MockEngine`, which drains its whole script on the first turn) — so the
/// dialogue turn and the close-time condensation turn can each be scripted.
struct PerCallEngine(Mutex<std::collections::VecDeque<Vec<AcpUpdate>>>);

impl PerCallEngine {
    fn new(scripts: Vec<Vec<AcpUpdate>>) -> Self {
        Self(Mutex::new(scripts.into_iter().collect()))
    }
}

#[async_trait::async_trait]
impl MystagogueEngine for PerCallEngine {
    async fn run_turn(
        &self,
        _prompt: AcpPrompt,
        tools: &dyn ToolDispatch,
        sink: &mut (dyn FnMut(AcpUpdate) + Send),
    ) -> Result<(), EngineError> {
        let script = self.0.lock().unwrap().pop_front().unwrap_or_default();
        for update in script {
            if let AcpUpdate::ToolCall(call) = &update {
                let call = call.clone();
                sink(update);
                let result = tools.dispatch(call).await;
                sink(AcpUpdate::ToolResult(result));
            } else {
                sink(update);
            }
        }
        Ok(())
    }
}

/// The "nothing is lost" close path, end to end through the bridge: a real
/// exchange persists BOTH roles to the transcript, the close-time condensation
/// writes a durable note, and the new read projections surface all of it —
/// the thread's session history, the full role-tagged transcript, and the
/// "past fires" recency list.
#[tokio::test]
async fn closing_condenses_a_note_and_the_session_reads_round_trip_both_roles() {
    let store = Arc::new(Store::open_in_memory("dev").unwrap());
    let thread = store
        .open_thread("does forgetting cost energy?", None, None)
        .unwrap();

    let engine = AthanorEngine::with_engine(
        Arc::clone(&store),
        Arc::new(PerCallEngine::new(vec![
            // turn 1 — the Mystagogue's reply
            vec![
                AcpUpdate::text_delta("Say what just set."),
                AcpUpdate::TurnComplete,
            ],
            // close — the condensation distillation
            vec![AcpUpdate::text_delta(
                "NOTE: The learner circled forgetting and named erasure as dissipation.\n\
                 PROFILE how_i_learn: reaches conviction by restating in their own words",
            )],
        ])),
    );

    let session = engine
        .begin_session(Some("philosophus".into()), None, Some(thread.id.clone()))
        .unwrap();
    let session_id = session.session_id();
    session
        .send_turn("Forgetting costs energy, doesn't it?".into())
        .await;
    session.close(15).await.unwrap();

    // The condensation note landed and the profile was merged.
    assert_eq!(
        store.session_note(&session_id).unwrap().as_deref(),
        Some("The learner circled forgetting and named erasure as dissipation.")
    );
    assert!(store
        .get_profile_section("how_i_learn")
        .unwrap()
        .contains("restating in their own words"));

    // sessions_for_thread: one closed session, its excerpt is the note.
    let history = engine.sessions_for_thread(thread.id.clone()).unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].id, session_id);
    assert_eq!(history[0].mask, "philosophus");
    assert!(history[0].excerpt.contains("erasure as dissipation"));

    // session_detail: the FULL role-tagged transcript, both sides, oldest first.
    let detail = engine.session_detail(session_id.clone()).unwrap();
    assert_eq!(detail.turns.len(), 2, "learner + mystagogue: {detail:?}");
    assert_eq!(detail.turns[0].role, "learner");
    assert_eq!(detail.turns[0].text, "Forgetting costs energy, doesn't it?");
    assert_eq!(detail.turns[1].role, "mystagogue");
    assert_eq!(detail.turns[1].text, "Say what just set.");
    assert!(detail.note.unwrap().contains("erasure as dissipation"));

    // recent_sessions: the "past fires" surface reaches it too.
    let recent = engine.recent_sessions(10).unwrap();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].id, session_id);
}

/// An engine that panics mid-turn — the exact hazard on a real device, where a
/// Rust panic (e.g. goose's `create_dir_all().expect()` on an un-writable path)
/// would unwind across the uniffi boundary and abort the host app.
struct PanickingEngine;

#[async_trait::async_trait]
impl MystagogueEngine for PanickingEngine {
    async fn run_turn(
        &self,
        _prompt: AcpPrompt,
        _tools: &dyn ToolDispatch,
        _sink: &mut (dyn FnMut(AcpUpdate) + Send),
    ) -> Result<(), EngineError> {
        panic!("simulated engine fault");
    }
}

/// FFI-seam panic containment: a turn whose engine PANICS must surface a calm,
/// in-band `SessionEvent::Error` — NOT abort the process. If containment
/// regressed, the panic would unwind through `send_turn` and this test binary
/// would crash instead of asserting.
#[tokio::test]
async fn a_panicking_engine_turn_is_contained_as_an_error_event_not_a_process_abort() {
    // Both `AthanorEngine::new` and the test `with_engine` install the panic
    // hook, so the contained fault carries the real message. Route through a
    // real in-memory store so `send_turn`'s whole path runs.
    let store = Arc::new(Store::open_in_memory("dev").unwrap());
    store
        .open_thread("what breaks at the seam?", None, None)
        .unwrap();
    let engine = AthanorEngine::with_engine(Arc::clone(&store), Arc::new(PanickingEngine));
    let session = engine.begin_session(None, None, None).unwrap();

    let collector = Arc::new(Collector(Mutex::new(Vec::new())));
    session.set_listener(collector.clone());

    // This call would abort the process if the panic escaped containment.
    session.send_turn("go".into()).await;

    let events = collector.0.lock().unwrap().clone();
    let err = events
        .iter()
        .find_map(|e| match e {
            SessionEvent::Error { message } => Some(message.clone()),
            _ => None,
        })
        .expect("the contained panic must surface as an Error event");
    assert!(
        err.contains("contained") && err.contains("simulated engine fault"),
        "the error carries the contained-fault detail: {err:?}"
    );
    // The session survives cleanly — it can still be landed.
    session.close(1).await.unwrap();
}
