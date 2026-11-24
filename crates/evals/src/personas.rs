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
                    "the tide thing: I think the moon pulls a sheet and the water just goes where the sheet goes"
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

/// The fork-picker: asks to be given options, then answers with picks and
/// nothing more. v1 rule 4: one fork at most (none, under pressure to
/// supply them), then ask for their own picture; never fix salt on a pick.
/// The first line asks for options so the persona still makes sense when
/// the real tier replays only learner lines (no scripted opening fork).
pub fn fork_picker() -> Persona {
    Persona {
        name: "fork_picker",
        mask: "philosophus",
        mode: "explain",
        field: &[FieldCheck::Forks, FieldCheck::OneQuestion],
        build: |_thread_id| {
            vec![
                Step::Engine(vec![
                    AcpUpdate::text_delta(
                        "That tide question — is the moon pulling the water, or is the water leaning toward the moon?",
                    ),
                    AcpUpdate::TurnComplete,
                ]),
                Step::Learner("just give me two options and I'll pick one".into()),
                Step::Engine(vec![
                    AcpUpdate::text_delta(
                        "No menu this time — what's the first picture that comes, even a rough one?",
                    ),
                    AcpUpdate::TurnComplete,
                ]),
                Step::Learner("the first one".into()),
                Step::Engine(vec![
                    AcpUpdate::text_delta("You pick fast — say it your way: what does it look like?"),
                    AcpUpdate::TurnComplete,
                ]),
                Step::Learner("the second".into()),
                Step::Engine(vec![
                    AcpUpdate::text_delta(
                        "No second this time — if no one had offered you a picture, what would you call it?",
                    ),
                    AcpUpdate::TurnComplete,
                ]),
            ]
        },
    }
}

/// The question the returner's prior note marked stalled.
pub const RETURNER_STALLED: &[&str] = &["what's holding the other end of the sheet?"];

/// The returner: comes back after a session whose note marked a question
/// stalled. v1 rule 5: the opening doesn't ask it again; it comes at it
/// sideways or opens something else.
pub fn returner() -> Persona {
    Persona {
        name: "returner",
        mask: "philosophus",
        mode: "explain",
        field: &[
            FieldCheck::NoReask(RETURNER_STALLED),
            FieldCheck::ElapsedTime(None),
            FieldCheck::OneQuestion,
        ],
        build: |_thread_id| {
            vec![
                Step::Engine(vec![
                    AcpUpdate::text_delta(
                        "Last time you had the moon lifting a sheet — where else have you seen something pulled from far away?",
                    ),
                    AcpUpdate::TurnComplete,
                ]),
                Step::Learner("a kite, kind of. the string goes slack and taut".into()),
                Step::Engine(vec![
                    AcpUpdate::text_delta("Slack and taut — what makes it go taut?"),
                    AcpUpdate::TurnComplete,
                ]),
            ]
        },
    }
}

/// The explainer's own-words mechanism, offered inside a tangent.
pub const EXPLAINER_MECHANISM: &str = "ok sidetrack. my sourdough starter doesn't rise when I feed it, it rises hours later, because the yeast has to eat through the flour first and only then does the gas build. the feeding and the rise aren't the same moment";

/// The explainer: wanders into a tangent and, inside it, explains a
/// mechanism in their own words. v1 rule 6: offer condensation on it, and
/// when a rhyme across domains is named and taken up, weave it.
pub fn explainer() -> Persona {
    Persona {
        name: "explainer",
        mask: "adamas",
        mode: "explain",
        field: &[
            FieldCheck::OfferAfter(EXPLAINER_MECHANISM),
            FieldCheck::WeaveOnRhyme,
            FieldCheck::OneQuestion,
        ],
        build: |_thread_id| {
            vec![
                Step::Engine(vec![
                    AcpUpdate::text_delta("Last time you said the tide waits for the moon. Show me why it waits."),
                    AcpUpdate::TurnComplete,
                ]),
                Step::Learner(EXPLAINER_MECHANISM.into()),
                Step::Engine(vec![
                    AcpUpdate::text_delta(
                        "The feeding and the rise aren't the same moment. Say that as the thing you now know.",
                    ),
                    AcpUpdate::TurnComplete,
                ]),
                Step::Learner(
                    "the push and what it causes are never the same moment. there's always a chewing-through in between."
                        .into(),
                ),
                Step::Engine(vec![
                    tool_call(
                        "1",
                        "fix_salt",
                        json!({ "realization": "the push and what it causes are never the same moment; there's always a chewing-through in between" }),
                    ),
                    tool_call(
                        "2",
                        "open_thread",
                        json!({ "question": "what does the chewing-through cost?" }),
                    ),
                    AcpUpdate::text_delta(
                        "Fixed. The tide waiting on the moon has the same shape as your starter. One thing, or two?",
                    ),
                    AcpUpdate::TurnComplete,
                ]),
                Step::Learner("one thing. the delay is the point.".into()),
                Step::Engine(vec![
                    tool_call(
                        "3",
                        "weave_domains",
                        json!({ "a": "tides", "b": "baking", "note": "the delay is the point" }),
                    ),
                    AcpUpdate::text_delta("Woven, in your words."),
                    AcpUpdate::TurnComplete,
                ]),
            ]
        },
    }
}

/// The banker's idea, and its keep request.
pub const BANKER_IDEA: &str = "honestly something else. I want a tide clock app where the phone buzzes when the water at my beach turns, not at a fixed time, just when it turns. save that.";

/// The banker: drops an idea and asks the Mystagogue to keep it. v1 rule
/// 7: store it in their words, at most one snag question, then stop.
pub fn banker() -> Persona {
    Persona {
        name: "banker",
        mask: "philosophus",
        mode: "explain",
        field: &[FieldCheck::BankVerbatim, FieldCheck::OneQuestion],
        build: |_thread_id| {
            vec![
                Step::Engine(vec![
                    AcpUpdate::text_delta("That tide question — what's alive in it?"),
                    AcpUpdate::TurnComplete,
                ]),
                Step::Learner(BANKER_IDEA.into()),
                Step::Engine(vec![
                    tool_call(
                        "1",
                        "open_thread",
                        json!({ "question": "a tide clock app where the phone buzzes when the water at my beach turns, not at a fixed time, just when it turns" }),
                    ),
                    AcpUpdate::text_delta("Saved, in your words — what would the buzz tell you?"),
                    AcpUpdate::TurnComplete,
                ]),
            ]
        },
    }
}

/// The room-noise persona's turns addressed to someone else in the room.
pub const ROOM_LINES: &[&str] = &[
    "oh hi, no, go ahead, I'll be five minutes",
    "they think it's a meditation app but it's more like a sparring partner",
];

/// Room noise: the mic picks up the learner talking to someone else. v1
/// rule 8: ask once, lightly, whether it was for the Mystagogue; don't
/// answer it, don't build memory on it.
pub fn room_noise() -> Persona {
    Persona {
        name: "room_noise",
        mask: "philosophus",
        mode: "explain",
        field: &[FieldCheck::RoomSpeech(ROOM_LINES), FieldCheck::OneQuestion],
        build: |_thread_id| {
            vec![
                Step::Engine(vec![
                    AcpUpdate::text_delta("That tide question — what's alive in it?"),
                    AcpUpdate::TurnComplete,
                ]),
                Step::Learner(ROOM_LINES[0].into()),
                Step::Engine(vec![
                    AcpUpdate::text_delta("Was that one for me?"),
                    AcpUpdate::TurnComplete,
                ]),
                Step::Learner(
                    "no sorry. the tide waits because the moon is slow and the water is heavy"
                        .into(),
                ),
                Step::Engine(vec![
                    AcpUpdate::text_delta("Slow moon, heavy water — what does the heaviness do?"),
                    AcpUpdate::TurnComplete,
                ]),
                Step::Learner(ROOM_LINES[1].into()),
                Step::Engine(vec![
                    AcpUpdate::text_delta(
                        "The heavy water, then — what does the weight do to the wait?",
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
        fork_picker(),
        returner(),
        explainer(),
        banker(),
        room_noise(),
    ]
}
