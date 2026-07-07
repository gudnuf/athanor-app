//! Hermetic end-to-end test for the FFI bridge (Plan Phase 4, C1 + C2). Drives
//! a real `AthanorEngine` (test-injected `MockEngine` — no network, no key)
//! through:
//! - a scripted turn that fixes salt, asserting the ordered `SessionEvent`
//!   stream *including a bridge-synthesized `Condensation`*;
//! - each of the four read projections round-tripping the seeded state;
//! - an `abandon` returning its thread to volatile.

use std::sync::{Arc, Mutex};

use athanor_core::domain::ThreadState;
use athanor_core::engine::{AcpToolCall, AcpUpdate, MockEngine};
use athanor_core::Store;
use ffi::engine::AthanorEngine;
use ffi::events::{SessionEvent, SessionEventListener};
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
        AcpUpdate::TextDelta("Erasure is dissipation.".into()),
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

    // Ordered event stream: delta, tool call, synthesized condensation, then
    // the turn completes — condensation strictly BEFORE TurnComplete.
    let events = collector.0.lock().unwrap().clone();
    assert_eq!(
        events.len(),
        4,
        "delta, toolcall, condensation, complete: {events:?}"
    );
    assert!(
        matches!(&events[0], SessionEvent::TextDelta { text, register }
            if text.contains("dissipation") && register == "quick"),
        "first event is a quick-register text delta: {:?}",
        events[0]
    );
    assert!(
        matches!(&events[1], SessionEvent::ToolCall { kind } if kind == "fix_salt"),
        "second event is the fix_salt tool call: {:?}",
        events[1]
    );
    let condensation_rid = match &events[2] {
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
        other => panic!("third event must be the condensation moment: {other:?}"),
    };
    assert!(
        matches!(events[3], SessionEvent::TurnComplete),
        "condensation precedes TurnComplete: {:?}",
        events[3]
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
        AcpUpdate::TextDelta("Before anything else — what's been pulling at you?".into()),
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
    assert_eq!(events.len(), 2, "delta, complete: {events:?}");
    assert!(
        matches!(&events[0], SessionEvent::TextDelta { text, .. }
            if text.contains("pulling at you")),
        "the Mystagogue's opening line streams with no learner turn preceding it: {:?}",
        events[0]
    );
    assert!(matches!(events[1], SessionEvent::TurnComplete));

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
