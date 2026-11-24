//! The four scripted eval personas (spec: `crates/athanor-core/prompts/
//! README.md`, "The four eval personas" table). Each one stresses a specific
//! promise of the Mystagogue pack; a prompt change that regresses any of
//! them should show up in the graded `SuiteReport` before it ships.
//!
//! A persona is a fixed sequence of `Step`s: engine turns (the exact
//! `AcpUpdate`s an `MockEngine` replays for that turn) interleaved with the
//! learner's own turns. `build` is a plain function of the seed thread id
//! rather than a fixed script, because that id is only known once the runner
//! opens a fresh in-memory store — ids are freshly generated (uuid v7) every
//! run, never hardcoded.

use athanor_core::engine::{AcpToolCall, AcpUpdate};
use serde_json::json;

use crate::field::FieldCheck;

/// One step of a scripted session, from the harness's point of view.
pub enum Step {
    /// One `MockEngine` turn: everything it streams before `TurnComplete`.
    Engine(Vec<AcpUpdate>),
    /// The learner's own turn, in their own words.
    Learner(String),
}

fn tool_call(id: &str, name: &str, args: serde_json::Value) -> AcpUpdate {
    AcpUpdate::ToolCall(AcpToolCall {
        id: id.into(),
        name: name.into(),
        args,
    })
}

/// A scripted persona: the mask/mode its session opens under, and its steps.
pub struct Persona {
    pub name: &'static str,
    pub mask: &'static str,
    pub mode: &'static str,
    pub build: fn(seed_thread_id: &str) -> Vec<Step>,
    /// Field-use graders (prompt pack v1) this scenario opts into, beyond
    /// the five every persona runs.
    pub field: &'static [FieldCheck],
}

/// The eager parroter: condenses the Mystagogue's own phrasing back as if it
/// were their own realization. `grade_salt_refusal` must FAIL this scenario —
/// the `fix_salt` fires on a near-verbatim echo of the assistant's own words,
/// never a salt in the learner's own frame.
pub fn eager_parroter() -> Persona {
    Persona {
        name: "eager_parroter",
        mask: "philosophus",
        mode: "explain",
        field: &[],
        build: |thread_id| {
            let question = "what happens to disorder in an isolated system over time";
            vec![
                Step::Engine(vec![
                    AcpUpdate::text_delta(format!("{question}?")),
                    AcpUpdate::TurnComplete,
                ]),
                // The learner echoes the question back nearly verbatim —
                // punctuation aside, an identical token set.
                Step::Learner(question.to_string()),
                Step::Engine(vec![
                    tool_call(
                        "1",
                        "fix_salt",
                        json!({
                            "realization": question,
                            "thread_id": thread_id,
                            "domains": ["thermodynamics"],
                        }),
                    ),
                    // Correct tool ORDER (fix_salt -> open_thread) so the
                    // spiral check passes while salt-refusal still fails —
                    // the two graders are independent, and this persona only
                    // exercises the discipline the latter is built for.
                    tool_call(
                        "2",
                        "open_thread",
                        json!({ "question": "what does this open?" }),
                    ),
                    AcpUpdate::TurnComplete,
                ]),
            ]
        },
    }
}

/// The stuck one: jams on one groove, defends a frame that no longer fits.
/// Tests Solve's entrance (mask = solve) and a clean exit — no false salt
/// manufactured just to make the session feel productive.
pub fn stuck_one() -> Persona {
    Persona {
        name: "stuck_one",
        mask: "solve",
        mode: "trace",
        field: &[],
        build: |_thread_id| {
            vec![
                Step::Engine(vec![
                    AcpUpdate::text_delta("The wall you keep hitting might be the teacher."),
                    tool_call(
                        "1",
                        "open_thread",
                        json!({ "question": "what if the frame itself is the obstacle?" }),
                    ),
                    AcpUpdate::TurnComplete,
                ]),
                Step::Learner("I don't know.".into()),
                Step::Engine(vec![
                    AcpUpdate::text_delta("Sit with not-knowing a moment longer."),
                    AcpUpdate::TurnComplete,
                ]),
                Step::Learner("I still don't know.".into()),
                Step::Engine(vec![
                    AcpUpdate::text_delta("Good. That's the frame breaking, not you failing."),
                    AcpUpdate::TurnComplete,
                ]),
            ]
        },
    }
}

/// The tangent-chaser: bolts to a new topic every turn, never lands on a
/// salt. Tests thread discipline and pacing — many threads opened, none
/// fixed.
pub fn tangent_chaser() -> Persona {
    Persona {
        name: "tangent_chaser",
        mask: "adamas",
        mode: "predict",
        field: &[],
        build: |_thread_id| {
            vec![
                Step::Engine(vec![
                    AcpUpdate::text_delta("Stay with the thread you opened."),
                    AcpUpdate::TurnComplete,
                ]),
                Step::Learner("actually, what about magnetism instead?".into()),
                Step::Engine(vec![
                    tool_call(
                        "1",
                        "open_thread",
                        json!({ "question": "why does iron pull iron?", "domain": "magnetism" }),
                    ),
                    AcpUpdate::TurnComplete,
                ]),
                Step::Learner("or wait, rhetoric is more interesting.".into()),
                Step::Engine(vec![
                    tool_call(
                        "2",
                        "open_thread",
                        json!({ "question": "what makes an argument land?", "domain": "rhetoric" }),
                    ),
                    AcpUpdate::TurnComplete,
                ]),
                Step::Learner("actually never mind, back to entropy.".into()),
                Step::Engine(vec![
                    tool_call(
                        "3",
                        "open_thread",
                        json!({ "question": "what is entropy, again?" }),
                    ),
                    AcpUpdate::TurnComplete,
                ]),
            ]
        },
    }
}

/// The silent one: minimal, terse, long pauses. Tests patience — the session
/// still lands and closes cleanly without the Mystagogue filling the gap by
/// lecturing.
pub fn silent_one() -> Persona {
    Persona {
        name: "silent_one",
        mask: "philosophus",
        mode: "design",
        field: &[],
        build: |_thread_id| {
            vec![
                Step::Engine(vec![
                    AcpUpdate::text_delta("What's stirring, even quietly?"),
                    AcpUpdate::TurnComplete,
                ]),
                Step::Learner("...".into()),
                Step::Engine(vec![
                    // One sentence, no bare declarative — stays within the
                    // Philosophus coarse proxy (grade_mask_fidelity).
                    AcpUpdate::text_delta("Still nothing — no rush, but is there anything at all?"),
                    AcpUpdate::TurnComplete,
                ]),
                Step::Learner("maybe.".into()),
            ]
        },
    }
}

// ---- prompt pack v1 field personas -------------------------------------
//
// Synthetic learners for the failure shapes field use surfaced. Each script
// pairs the learner's lines with the Mystagogue behaviour the v1 pack asks
// for, so the hermetic suite passes; each has a unit test in `run.rs` that
// swaps in the v0-shaped reply and shows its grader failing. In the gated
// tier only the learner lines replay, against the real engine.

/// Dead air: the speech path sends empty turns. v1 rule 2: after two, leave
/// the door open and land; no chasing, no "you're back", no memory writes.
pub fn dead_air() -> Persona {
    Persona {
        name: "dead_air",
        mask: "philosophus",
        mode: "explain",
        field: &[FieldCheck::EmptyTurns, FieldCheck::ElapsedTime(None)],
        build: |_thread_id| {
            vec![
                Step::Engine(vec![
                    AcpUpdate::text_delta("That tide question you left open — what's alive in it?"),
                    AcpUpdate::TurnComplete,
                ]),
                Step::Learner(".".into()),
                Step::Engine(vec![
                    AcpUpdate::text_delta("Anything there, even one word?"),
                    AcpUpdate::TurnComplete,
                ]),
                Step::Learner(". . .".into()),
                Step::Engine(vec![
                    AcpUpdate::text_delta("Nothing has to come. The door stays open."),
                    AcpUpdate::TurnComplete,
                ]),
                Step::Learner(".".into()),
            ]
        },
    }
}

/// The leaver: works one turn, then says they have to go. v1 rule 3: one
/// question per reply, and the reply to "I have to go" names what stayed
/// open and asks nothing.
pub fn leaver() -> Persona {
    Persona {
        name: "leaver",
        mask: "philosophus",
        mode: "explain",
        field: &[
            FieldCheck::OneQuestion,
            FieldCheck::Landing,
            FieldCheck::ElapsedTime(None),
        ],
        build: |_thread_id| {
            vec![
                Step::Engine(vec![
                    AcpUpdate::text_delta(
                        "Last time you left the tide question open — what's it doing now?",
                    ),
                    AcpUpdate::TurnComplete,
                ]),
                Step::Learner(
                    "it's like the moon pulls a sheet and the water just goes where the sheet goes"
                        .into(),
                ),
                Step::Engine(vec![
                    AcpUpdate::text_delta("A sheet — what's holding the other end?"),
                    AcpUpdate::TurnComplete,
                ]),
                Step::Learner("not sure. I have to go, actually.".into()),
                Step::Engine(vec![
                    AcpUpdate::text_delta(
                        "Who holds the other end of the sheet stays open. We start there.",
                    ),
                    AcpUpdate::TurnComplete,
                ]),
            ]
        },
    }
}

/// The four README personas, then the v1 field personas.
pub fn all_personas() -> Vec<Persona> {
    vec![
        eager_parroter(),
        stuck_one(),
        tangent_chaser(),
        silent_one(),
        dead_air(),
        leaver(),
    ]
}
