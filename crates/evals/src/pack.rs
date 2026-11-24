//! Pack-contract checks: does the ASSEMBLED prompt still state each field-use
//! rule (prompt pack v1)?
//!
//! The 16 snapshot tests in `athanor-core/tests/prompt_snapshots.rs` lock
//! assembly structure and redact prose on purpose. That leaves a gap: a
//! rewording that silently drops a rule passes every snapshot. These checks
//! close it, narrowly. Each rule names the surface it must reach the model
//! through (a session prompt under some mask, or the close-only condense
//! prompt) and a few short anchors, matched case-insensitively. Anchors are
//! the rule's load-bearing words, not its prose: rewording around them is
//! free; removing one is a visible, deliberate change to this table.
//!
//! This is the hermetic half of each v1 change. It fails on a pack that
//! doesn't carry the rule and passes on one that does; it says nothing about
//! whether the model obeys. That's the behavioural graders (`field`) run in
//! the gated tier.

use athanor_core::prompt::{assemble, assemble_condensation};
use athanor_core::store::Store;

use crate::report::CheckResult;

/// Where a rule must reach the model.
#[derive(Clone, Copy, Debug)]
pub enum Surface {
    /// The live session prompt under this mask (mode `explain`).
    Session(&'static str),
    /// The close-only distillation prompt (`condense.md` path).
    Condense,
}

/// One v1 field rule, as the pack must state it.
pub struct PackRule {
    pub id: &'static str,
    pub surface: Surface,
    pub anchors: &'static [&'static str],
}

/// The v1 field rules, in the usage review's ranked order.
pub const FIELD_RULES: &[PackRule] = &[
    PackRule {
        id: "1_elapsed_time",
        surface: Surface::Session("philosophus"),
        anchors: &["{{last_session_ago}}", "\"last time\"", "a date on a note"],
    },
    PackRule {
        id: "2_empty_turn",
        surface: Surface::Session("philosophus"),
        anchors: &[
            "an empty turn",
            "two in a row",
            "don't say they're back",
            "never on the strength of silence",
        ],
    },
    PackRule {
        id: "2_empty_turn_close",
        surface: Surface::Condense,
        anchors: &["empty turns", "tell you about the mic, not the learner"],
    },
    PackRule {
        id: "3_one_question_landing",
        surface: Surface::Session("philosophus"),
        anchors: &[
            "one question per reply",
            "the same question twice in new words",
            "the last thing you say is not a question",
        ],
    },
    PackRule {
        id: "4_ration_forks",
        surface: Surface::Session("adamas"),
        anchors: &[
            "ration the either/or",
            "at most one a session",
            "a picture you offered",
            "say it their way before you build on it",
        ],
    },
    PackRule {
        id: "5_stalled_session",
        surface: Surface::Session("philosophus"),
        anchors: &[
            "has stalled",
            "marks a question stalled",
            "don't read the thread back",
        ],
    },
    PackRule {
        id: "5_stalled_close",
        surface: Surface::Condense,
        anchors: &["call it stalled"],
    },
    PackRule {
        id: "6_offer_and_weave",
        surface: Surface::Session("adamas"),
        anchors: &[
            "even mid-tangent",
            "a rhyme said aloud",
            "before you cut again",
        ],
    },
    PackRule {
        id: "7_keep_in_their_words",
        surface: Surface::Session("philosophus"),
        anchors: &["keep it in their words", "`bank_pull`", "one snag question"],
    },
    PackRule {
        id: "7_pulls_quote_them",
        surface: Surface::Condense,
        anchors: &["a `pulls` line quotes them"],
    },
    PackRule {
        id: "8_room_speech_session",
        surface: Surface::Session("philosophus"),
        anchors: &["not every voice is for you", "ask once, lightly"],
    },
    PackRule {
        id: "8_room_speech_close",
        surface: Surface::Condense,
        anchors: &["for someone else in the room"],
    },
    PackRule {
        id: "9_no_style_from_silence_close",
        surface: Surface::Condense,
        anchors: &[
            "never infer how they learn from them",
            "one session is not a pattern",
            "leave `how_i_learn` as it is",
        ],
    },
    PackRule {
        id: "9_no_style_from_silence_session",
        surface: Surface::Session("philosophus"),
        anchors: &["how they learn from one session"],
    },
    PackRule {
        id: "spiral_child_question",
        surface: Surface::Session("philosophus"),
        anchors: &["`child_question`", "that makes a twin"],
    },
    PackRule {
        id: "10_say_less",
        surface: Surface::Session("philosophus"),
        anchors: &[
            "say less than they did",
            "under twenty-five words",
            "more than sixty",
        ],
    },
    PackRule {
        id: "11_landed_reply",
        surface: Surface::Session("philosophus"),
        anchors: &["answer it with one short line", "under twenty words"],
    },
];

/// A small fixed store so every placeholder renders (the rule text lives in
/// the assets; the store only has to make assembly total).
fn fixture_store() -> Store {
    let store = Store::open_in_memory("pack-contract").expect("store");
    let d = store.upsert_domain("tides").expect("domain");
    store
        .open_thread("why does the tide wait for the moon?", Some(&d.id), None)
        .expect("thread");
    store
}

/// The text a surface delivers to the model.
pub fn surface_text(surface: Surface) -> String {
    let store = fixture_store();
    match surface {
        Surface::Session(mask) => assemble(mask, "explain", None, &store).system_prompt,
        Surface::Condense => assemble_condensation(&store, None),
    }
}

/// Checks one rule's anchors against its surface.
pub fn check_rule(rule: &PackRule) -> CheckResult {
    // Whitespace-folded, so an anchor that wraps across a line still matches.
    let fold = |s: &str| {
        s.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    };
    let text = fold(&surface_text(rule.surface));
    let missing: Vec<&str> = rule
        .anchors
        .iter()
        .copied()
        .filter(|a| !text.contains(&fold(a)))
        .collect();
    CheckResult {
        name: format!("pack:{}", rule.id),
        passed: missing.is_empty(),
        detail: if missing.is_empty() {
            format!("{:?} carries every anchor", rule.surface)
        } else {
            format!("{:?} is missing anchor(s) {missing:?}", rule.surface)
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_field_rule_is_stated_in_the_pack() {
        let failures: Vec<CheckResult> = FIELD_RULES
            .iter()
            .map(check_rule)
            .filter(|c| !c.passed)
            .collect();
        assert!(
            failures.is_empty(),
            "pack is missing v1 rules: {failures:#?}"
        );
    }

    #[test]
    fn a_missing_anchor_fails() {
        let rule = PackRule {
            id: "probe",
            surface: Surface::Condense,
            anchors: &["an anchor no prompt will ever contain"],
        };
        assert!(!check_rule(&rule).passed);
    }

    /// v1 rule 1 applied to the pack itself: no example line in any asset
    /// may model an interval or time of day the core didn't supply. The one
    /// allowed claim is the rule's own counter-example ("not 'this
    /// morning.'" in identity §1).
    #[test]
    fn the_pack_models_no_invented_clock() {
        use athanor_core::prompt::assets::*;
        let assets = [
            ("identity", IDENTITY),
            ("condensation", CONDENSATION),
            ("condense", CONDENSE),
            ("initiation", INITIATION),
            ("philosophus", MASK_PHILOSOPHUS),
            ("adamas", MASK_ADAMAS),
            ("solve", MASK_SOLVE),
            ("trace", MODE_TRACE),
            ("explain", MODE_EXPLAIN),
            ("predict", MODE_PREDICT),
            ("challenge", MODE_CHALLENGE),
            ("design", MODE_DESIGN),
        ];
        let mut found = Vec::new();
        for (name, text) in assets {
            for claim in crate::field::time_claims(text) {
                if !(name == "identity" && claim == "this morning") {
                    found.push(format!("{name}: \"{claim}\""));
                }
            }
        }
        assert!(
            found.is_empty(),
            "prompt examples model an invented clock: {found:?}"
        );
    }
}
