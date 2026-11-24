//! Deterministic graders over a scripted session trace. Every grader here is
//! a pure function `(&SessionTrace) -> CheckResult` — no engine, no network,
//! no clock, no randomness — so a grade is reproducible byte-for-byte across
//! runs and prompt-pack versions.
//!
//! Three graders, three different slices of the product spec:
//! - `grade_spiral` — the spiral invariant, structurally: `fix_salt` (salt
//!   condensation) is never the last tool call in the sequence between two
//!   salts; it is always followed by `open_thread` before the next
//!   `fix_salt`. This is the machine-checkable half of the spiral
//!   invariant — it says nothing about whether the *content* of the salt is
//!   good, only that the session keeps moving outward after condensing.
//! - `grade_salt_refusal` — condensation refusal: a learner who parrots the
//!   assistant's own phrasing back (Dice similarity to the nearest prior
//!   assistant turn >= 0.7) hasn't produced a salt in their own words, so a
//!   `fix_salt` committed off that parroted text is a discipline violation.
//!   Threshold 0.7 is deliberately stricter than the grader's general
//!   item-match threshold (0.5, see `normalize`) — we only want to flag
//!   near-verbatim echoes, not any paraphrase that happens to share
//!   vocabulary with the assistant.
//! - `grade_mask_fidelity` — a *coarse, structural proxy* for the
//!   Philosophus mask: every assistant turn must ask (contain a `?`), except
//!   a landing line; and no declarative may assert what the learner didn't
//!   supply. A declarative fails only if it brings in two or more content
//!   words the learner never said, AND isn't addressed to the dialogue
//!   (I/you/we, or a word like "question" or "picture"), AND isn't an
//!   imperative set-up ("Picture a table."). Reflections, meta-moves and
//!   set-ups are allowed by the mask itself (`masks/philosophus.md`
//!   ALLOWED). An earlier version failed every declarative; scripted replies
//!   satisfied it, but real model output (gated run 1) did not. It catches
//!   "Entropy always increases in a closed system." It says nothing about
//!   whether a question is *good*, and a fact smuggled inside an imperative
//!   ("Picture a sealed box, nothing in or out.") passes. Both are the gated
//!   LLM-judge tier's job. Graders here only ever look at surface structure.

use std::collections::BTreeSet;

use crate::normalize::{dice, token_set, tokens};
use crate::report::CheckResult;

/// One turn in a scripted session trace. Tool calls carry their name and a
/// single string arg (the condensed salt text, the thread-opening question,
/// etc.) — enough for the graders below, nothing more.
#[derive(Clone, Debug, PartialEq)]
pub enum Turn {
    Assistant(String),
    ToolCall { name: String, args: String },
    LearnerText(String),
}

/// Convenience constructor: an assistant utterance.
pub fn assistant(text: impl Into<String>) -> Turn {
    Turn::Assistant(text.into())
}

/// Convenience constructor: a tool call (`fix_salt`, `open_thread`, ...).
pub fn tool(name: impl Into<String>, args: impl Into<String>) -> Turn {
    Turn::ToolCall {
        name: name.into(),
        args: args.into(),
    }
}

/// Convenience constructor: a learner's own text.
pub fn learner(text: impl Into<String>) -> Turn {
    Turn::LearnerText(text.into())
}

/// An ordered scripted session: tool calls interleaved with assistant and
/// learner turns, plus the mask the session ran under (only relevant to
/// `grade_mask_fidelity` today; `None` for traces that don't exercise a
/// mask-scoped grader).
#[derive(Clone, Debug, PartialEq)]
pub struct SessionTrace {
    pub mask: Option<String>,
    pub turns: Vec<Turn>,
}

impl SessionTrace {
    pub fn new(turns: Vec<Turn>) -> Self {
        SessionTrace { mask: None, turns }
    }

    pub fn with_mask(mask: impl Into<String>, turns: Vec<Turn>) -> Self {
        SessionTrace {
            mask: Some(mask.into()),
            turns,
        }
    }
}

fn is_tool(turn: &Turn, name: &str) -> bool {
    matches!(turn, Turn::ToolCall { name: n, .. } if n == name)
}

/// Structural spiral-discipline check: every `fix_salt` tool call must be
/// followed — before the next `fix_salt`, or before the trace ends — by an
/// `open_thread` tool call. A `fix_salt` with nothing after it (end of
/// trace) is a violation, same as one immediately followed by another
/// `fix_salt`.
pub fn grade_spiral(trace: &SessionTrace) -> CheckResult {
    let fix_indices: Vec<usize> = trace
        .turns
        .iter()
        .enumerate()
        .filter_map(|(i, t)| is_tool(t, "fix_salt").then_some(i))
        .collect();

    let mut violations = Vec::new();
    for (k, &i) in fix_indices.iter().enumerate() {
        let next_fix = fix_indices.get(k + 1).copied().unwrap_or(trace.turns.len());
        let has_open_thread = trace.turns[i + 1..next_fix]
            .iter()
            .any(|t| is_tool(t, "open_thread"));
        if !has_open_thread {
            violations.push(i);
        }
    }

    let passed = violations.is_empty();
    let detail = if passed {
        "every fix_salt is followed by open_thread before the next fix_salt (or trace end)".into()
    } else {
        format!(
            "fix_salt at turn index(es) {violations:?} had no open_thread before the next fix_salt or the end of the trace"
        )
    };
    CheckResult {
        name: "spiral".into(),
        passed,
        detail,
    }
}

/// Condensation-refusal check: for every `fix_salt(text)`, find the nearest
/// preceding `LearnerText` and, before that, the nearest preceding
/// `Assistant` turn. If the learner text is a near-verbatim echo of that
/// assistant turn (Dice >= 0.7), the salt was parroted, not condensed in the
/// learner's own words — a violation. A `fix_salt` with no preceding learner
/// text (nothing to have parroted) is not flagged.
pub fn grade_salt_refusal(trace: &SessionTrace) -> CheckResult {
    let mut violations = Vec::new();

    for (i, turn) in trace.turns.iter().enumerate() {
        if !is_tool(turn, "fix_salt") {
            continue;
        }

        let Some(learner_idx) = (0..i)
            .rev()
            .find(|&j| matches!(trace.turns[j], Turn::LearnerText(_)))
        else {
            continue;
        };
        let Turn::LearnerText(learner_text) = &trace.turns[learner_idx] else {
            unreachable!()
        };

        let Some(assistant_idx) = (0..learner_idx)
            .rev()
            .find(|&j| matches!(trace.turns[j], Turn::Assistant(_)))
        else {
            continue;
        };
        let Turn::Assistant(assistant_text) = &trace.turns[assistant_idx] else {
            unreachable!()
        };

        let score = dice(&token_set(learner_text), &token_set(assistant_text));
        if score >= 0.7 {
            violations.push((i, score));
        }
    }

    let passed = violations.is_empty();
    let detail = if passed {
        "no fix_salt committed off a parroted (dice >= 0.7) learner echo".into()
    } else {
        format!(
            "fix_salt at turn index(es) {violations:?} (turn, dice-score) committed off a learner echo scoring >= 0.7 against the nearest prior assistant turn"
        )
    };
    CheckResult {
        name: "salt_refusal".into(),
        passed,
        detail,
    }
}

/// Splits `text` into sentences on `.`/`!`/`?`, keeping the terminal
/// punctuation with each sentence. A trailing fragment with no terminal
/// punctuation is dropped — it isn't a complete sentence to judge.
pub(crate) fn terminated_sentences(text: &str) -> Vec<(String, char)> {
    let mut sentences = Vec::new();
    let mut current = String::new();
    for c in text.chars() {
        current.push(c);
        if c == '.' || c == '!' || c == '?' {
            sentences.push((std::mem::take(&mut current), c));
        }
    }
    sentences
}

/// Words that carry no domain content on their own.
const FUNCTION_WORDS: &[&str] = &[
    "a",
    "an",
    "the",
    "and",
    "or",
    "but",
    "so",
    "if",
    "then",
    "than",
    "of",
    "to",
    "in",
    "on",
    "at",
    "for",
    "with",
    "from",
    "by",
    "as",
    "into",
    "onto",
    "about",
    "over",
    "under",
    "up",
    "down",
    "out",
    "off",
    "toward",
    "is",
    "are",
    "was",
    "were",
    "be",
    "been",
    "being",
    "am",
    "do",
    "does",
    "did",
    "doesn",
    "don",
    "didn",
    "isn",
    "aren",
    "wasn",
    "t",
    "s",
    "re",
    "ve",
    "ll",
    "d",
    "m",
    "have",
    "has",
    "had",
    "it",
    "its",
    "that",
    "this",
    "these",
    "those",
    "there",
    "here",
    "what",
    "which",
    "who",
    "how",
    "why",
    "when",
    "where",
    "not",
    "no",
    "yes",
    "just",
    "only",
    "even",
    "still",
    "yet",
    "again",
    "now",
    "very",
    "too",
    "also",
    "can",
    "could",
    "will",
    "would",
    "should",
    "may",
    "might",
    "must",
    "need",
    "enough",
    "start",
    "keep",
    "ready",
    "rush",
    "fine",
    "good",
    "okay",
    "ok",
    "right",
    "way",
    "either",
    "both",
    "all",
    "any",
    "some",
    "one",
    "bit",
    "while",
    "first",
    "next",
    "fair",
    "sure",
    "sorry",
    "much",
    "more",
    "less",
    "something",
    "nothing",
    "anything",
    "everything",
    "else",
    "same",
    "other",
    "own",
    "set",
    "aside",
    "got",
];

/// Nouns that make a sentence about the dialogue, not the domain.
const DIALOGUE_WORDS: &[&str] = &[
    "meant",
    "sounded",
    "heard",
    "word",
    "question",
    "thread",
    "picture",
    "phrase",
    "answer",
    "sentence",
    "idea",
    "image",
    "point",
    "definition",
    "list",
    "snag",
    "session",
    "mic",
];

/// First/second person: a sentence addressed to the dialogue.
const PERSON_WORDS: &[&str] = &[
    "i", "me", "my", "mine", "you", "your", "yours", "we", "us", "our", "let",
];

/// Verbs that open an imperative set-up ("Picture a table.", "Say more.").
const IMPERATIVE_VERBS: &[&str] = &[
    "say", "picture", "imagine", "hold", "take", "try", "tell", "slow", "let", "don", "leave",
    "go", "start", "think", "look", "keep", "stay", "put", "give", "name", "sit", "notice", "stop",
    "come", "consider", "suppose", "describe", "show",
];

fn in_list(list: &[&str], tok: &str) -> bool {
    list.iter()
        .any(|w| tokens(w).first().map(String::as_str) == Some(tok))
}

/// The coarse "asserts a domain fact" test for one declarative sentence: it
/// fails only if it brings in two or more content words the learner never
/// said AND is not about the dialogue (no I/you/we, no dialogue noun) AND is
/// not an imperative set-up. Reflections ("A kite. Slack and taut."),
/// meta-moves ("That word's been sitting open.") and set-ups ("Picture a
/// table.") pass; "Entropy always increases in a closed system." fails.
fn asserts_new_fact(sentence: &str, learner_words: &BTreeSet<String>) -> bool {
    let toks = tokens(sentence);
    let Some(first) = toks.first() else {
        return false;
    };
    if in_list(IMPERATIVE_VERBS, first) {
        return false;
    }
    if toks
        .iter()
        .any(|t| in_list(PERSON_WORDS, t) || in_list(DIALOGUE_WORDS, t))
    {
        return false;
    }
    let novel = toks
        .iter()
        .filter(|t| !t.chars().all(|c| c.is_ascii_digit()))
        .filter(|t| !in_list(FUNCTION_WORDS, t))
        .filter(|t| !learner_words.contains(*t))
        .count();
    novel >= 2
}

/// Coarse, hermetic proxy for Philosophus mask fidelity (see module docs for
/// scope). Only fires when `trace.mask == Some("philosophus")`; traces under
/// any other (or no) mask pass trivially — this grader has nothing to say
/// about them.
pub fn grade_mask_fidelity(trace: &SessionTrace) -> CheckResult {
    if trace.mask.as_deref() != Some("philosophus") {
        return CheckResult {
            name: "mask_fidelity".into(),
            passed: true,
            detail: "not a philosophus-mask trace; this proxy is scoped to philosophus only".into(),
        };
    }

    let mut violations = Vec::new();
    for (i, turn) in trace.turns.iter().enumerate() {
        let Turn::Assistant(text) = turn else {
            continue;
        };
        // A landing line (identity §5, pack v1 rules 2–3) states what stayed
        // open and stops. It is not an asking turn, so the proxy skips it.
        if crate::field::is_landing_context(trace, i) {
            continue;
        }

        if !text.contains('?') {
            violations.push(format!("turn {i}: no '?' present in \"{text}\""));
            continue;
        }

        let learner_words: BTreeSet<String> = trace.turns[..i]
            .iter()
            .filter_map(|t| match t {
                Turn::LearnerText(s) => Some(tokens(s)),
                _ => None,
            })
            .flatten()
            .collect();
        for (sentence, terminal) in terminated_sentences(text) {
            if terminal == '.'
                && !sentence.contains('?')
                && asserts_new_fact(&sentence, &learner_words)
            {
                violations.push(format!(
                    "turn {i}: declarative asserting what the learner didn't supply \"{}\"",
                    sentence.trim()
                ));
            }
        }
    }

    let passed = violations.is_empty();
    let detail = if passed {
        "every philosophus turn asks, and no declarative asserts what the learner didn't supply"
            .into()
    } else {
        format!("violations: {}", violations.join("; "))
    };
    CheckResult {
        name: "mask_fidelity".into(),
        passed,
        detail,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trace(turns: &[Turn]) -> SessionTrace {
        SessionTrace::new(turns.to_vec())
    }

    fn trace_mask(mask: &str, turns: &[Turn]) -> SessionTrace {
        SessionTrace::with_mask(mask, turns.to_vec())
    }

    // --- grade_spiral -------------------------------------------------

    #[test]
    fn spiral_check_requires_open_thread_after_every_fix_salt() {
        let ok = trace(&[
            tool("fix_salt", "A"),
            tool("open_thread", "q1"),
            tool("fix_salt", "B"),
            tool("open_thread", "q2"),
        ]);
        assert!(grade_spiral(&ok).passed);

        let bad = trace(&[
            tool("fix_salt", "A"),
            tool("fix_salt", "B"),
            tool("open_thread", "q1"),
        ]);
        assert!(
            !grade_spiral(&bad).passed,
            "fix_salt A had no open_thread before the next fix_salt"
        );
    }

    #[test]
    fn spiral_violation_at_end_of_trace() {
        // The last tool call in the whole trace is fix_salt, with nothing
        // after it at all — no next fix_salt to bound the search, so the
        // search runs to the end of the trace and still finds no
        // open_thread. Must be flagged same as a mid-trace violation.
        let bad = trace(&[tool("open_thread", "q0"), tool("fix_salt", "A")]);
        let result = grade_spiral(&bad);
        assert!(
            !result.passed,
            "trailing fix_salt with no subsequent open_thread must fail"
        );
        assert!(
            result.detail.contains('1'),
            "should name the violating index"
        );
    }

    // --- grade_salt_refusal --------------------------------------------

    #[test]
    fn salt_refusal_rejects_parroted_condensation() {
        // learner echoes the assistant's phrasing (dice >= 0.7) then a
        // fix_salt fires -> FAIL
        let parrot = trace(&[
            assistant("entropy is disorder"),
            learner("entropy is disorder"),
            tool("fix_salt", "entropy is disorder"),
        ]);
        assert!(!grade_salt_refusal(&parrot).passed);

        // learner's OWN words (low overlap) -> fix_salt allowed -> PASS
        let own = trace(&[
            assistant("entropy is disorder"),
            learner("its the count of ways i cant tell apart"),
            tool("fix_salt", "ways i cant tell apart"),
        ]);
        assert!(grade_salt_refusal(&own).passed);
    }

    #[test]
    fn salt_refusal_fails_at_exactly_the_0_7_threshold() {
        // Two 10-token sets sharing exactly 7 tokens: dice = 2*7/(10+10) = 0.7.
        // Threshold is ">=", so exactly-0.7 must still be flagged as parroted.
        let assistant_text = "alpha bravo charlie delta echo foxtrot golf hotel india juliet";
        let learner_text = "alpha bravo charlie delta echo foxtrot golf kilo lima mike";

        let score = dice(&token_set(assistant_text), &token_set(learner_text));
        assert!(
            (score - 0.7).abs() < 1e-9,
            "fixture must score exactly 0.7, got {score}"
        );

        let at_threshold = trace(&[
            assistant(assistant_text),
            learner(learner_text),
            tool("fix_salt", learner_text),
        ]);
        assert!(
            !grade_salt_refusal(&at_threshold).passed,
            "dice == 0.7 must be treated as parroted (threshold is inclusive)"
        );
    }

    #[test]
    fn salt_refusal_ignores_fix_salt_with_no_preceding_learner_text() {
        // No LearnerText anywhere before the fix_salt -> nothing to have
        // parroted -> not flagged.
        let no_learner = trace(&[
            assistant("entropy is disorder"),
            tool("fix_salt", "entropy"),
        ]);
        assert!(grade_salt_refusal(&no_learner).passed);
    }

    // --- grade_mask_fidelity --------------------------------------------

    #[test]
    fn philosophus_mask_emits_no_declaratives() {
        let ok = trace_mask(
            "philosophus",
            &[assistant("what would happen if you doubled it?")],
        );
        assert!(grade_mask_fidelity(&ok).passed);

        let bad = trace_mask("philosophus", &[assistant("Entropy always increases.")]);
        assert!(
            !grade_mask_fidelity(&bad).passed,
            "a bare declarative in Philosophus is a violation"
        );
    }

    #[test]
    fn philosophus_turn_with_no_question_mark_at_all_fails() {
        // No terminal punctuation whatsoever, and no '?' either — still a
        // violation, since the mask requires every turn to actually ask
        // something.
        let bad = trace_mask("philosophus", &[assistant("just breathe")]);
        let result = grade_mask_fidelity(&bad);
        assert!(!result.passed, "an assistant turn with no '?' must fail");
    }

    #[test]
    fn mask_fidelity_is_a_no_op_outside_philosophus() {
        // Same bare declarative, but the trace isn't scoped to philosophus
        // (no mask at all) -> this grader has nothing to say, passes
        // trivially.
        let unscoped = trace(&[assistant("Entropy always increases.")]);
        assert!(grade_mask_fidelity(&unscoped).passed);

        let other_mask = trace_mask("adamas", &[assistant("Entropy always increases.")]);
        assert!(grade_mask_fidelity(&other_mask).passed);
    }

    #[test]
    fn philosophus_mask_allows_multiple_sentences_if_all_end_in_question() {
        let ok = trace_mask(
            "philosophus",
            &[assistant(
                "What is entropy, really? And why does it only ever grow?",
            )],
        );
        assert!(grade_mask_fidelity(&ok).passed);
    }

    // Real-output shapes (gated run 1, synthetic personas): the mask allows
    // reflections of the learner's words, meta-moves about the dialogue and
    // imperatives that set up a question. None of these assert a fact.
    #[test]
    fn philosophus_allows_reflections_meta_moves_and_set_ups() {
        let ok = trace_mask(
            "philosophus",
            &[
                learner("a kite, kind of. the string goes slack and taut"),
                assistant("A kite. Slack and taut. When the string goes slack, what's happening in your picture?"),
                learner("no sorry. the tide waits because the moon is slow and the water is heavy"),
                assistant("No need to be sorry. \"The tide waits because the moon is slow and the water is heavy.\" What does \"waits\" mean there?"),
                assistant("That word's been sitting open. Before any definition, what picture comes to mind?"),
                assistant("Let's not define it. Picture a table. It doesn't have to be right. What's the first thing you'd put on it?"),
                assistant("Kept, in your words. One snag. By \"turns,\" what do you mean?"),
                assistant("That sounded like it was meant for someone else. Was it for me?"),
            ],
        );
        let r = grade_mask_fidelity(&ok);
        assert!(r.passed, "{}", r.detail);
    }

    #[test]
    fn philosophus_still_fails_a_fact_the_learner_never_supplied() {
        for line in [
            "Entropy always increases in a closed system. Why?",
            "Heat flows from hot to cold. What follows?",
            "Isolated systems drift toward disorder. What does that tell you?",
        ] {
            let bad = trace_mask(
                "philosophus",
                &[
                    learner("what happens to disorder over time"),
                    assistant(line),
                ],
            );
            assert!(!grade_mask_fidelity(&bad).passed, "should fail: {line}");
        }
    }
}
