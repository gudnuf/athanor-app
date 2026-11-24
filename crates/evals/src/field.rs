//! Field-use graders: deterministic checks for failure shapes that only
//! showed up once the Mystagogue met real sessions (prompt pack v1, see
//! `docs/research/2026-11-24-prompt-pack-v1-from-field-use.md`).
//!
//! Same contract as `grade`: every grader is a pure function of a
//! `SessionTrace` (plus, for a few, a fixed parameter the scenario supplies),
//! with no engine, network, clock or randomness. They are surface proxies on
//! purpose. Each one catches the blunt form of a failure ("a count of days
//! the engine never supplied", "two questions in one reply"); the subtle
//! form is the gated LLM-judge tier's job, and each grader's doc names the
//! judge question that covers what it can't see.
//!
//! In the hermetic tier these run over scripted transcripts, so they prove
//! the grader bites, not that the model behaves. Their real use is the gated
//! tier: the same graders over real-engine transcripts.

use crate::grade::{SessionTrace, Turn};
use crate::normalize::tokens;
use crate::report::CheckResult;

/// One Mystagogue reply: everything between two learner turns (assistant
/// text, possibly split by tool calls, plus those tool calls). `after_learner`
/// is the index in `trace.turns` of the learner turn it answers (`None` for
/// the opening).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reply {
    pub after_learner: Option<usize>,
    pub text: String,
    pub tools: Vec<(String, String)>,
}

/// Groups a trace into Mystagogue replies. Empty groups (two learner turns in
/// a row) produce no reply.
pub fn replies(trace: &SessionTrace) -> Vec<Reply> {
    let mut out = Vec::new();
    let mut current = Reply::default();
    let mut has_content = false;
    for (i, turn) in trace.turns.iter().enumerate() {
        match turn {
            Turn::LearnerText(_) => {
                if has_content {
                    out.push(std::mem::take(&mut current));
                }
                current = Reply {
                    after_learner: Some(i),
                    ..Reply::default()
                };
                has_content = false;
            }
            Turn::Assistant(text) => {
                if !current.text.is_empty() {
                    current.text.push(' ');
                }
                current.text.push_str(text);
                has_content = true;
            }
            Turn::ToolCall { name, args } => {
                current.tools.push((name.clone(), args.clone()));
                has_content = true;
            }
        }
    }
    if has_content {
        out.push(current);
    }
    out
}

fn result(name: &str, violations: Vec<String>, ok: &str) -> CheckResult {
    let passed = violations.is_empty();
    CheckResult {
        name: name.into(),
        passed,
        detail: if passed {
            ok.into()
        } else {
            format!("violations: {}", violations.join("; "))
        },
    }
}

// ---- 1. elapsed time ----------------------------------------------------

const TIME_UNITS: &[&str] = &["minute", "hour", "day", "night", "week", "month", "year"];
const QUANTITIES: &[&str] = &["a", "an", "few", "couple", "several"];
const ANCHORS_AFTER: &[&str] = &["ago", "since", "back", "later", "on", "now"];
/// Interval / time-of-day claims that need no quantity.
const FIXED_CLAIMS: &[&str] = &[
    "yesterday",
    "last night",
    "last week",
    "last month",
    "this morning",
    "this afternoon",
    "this evening",
    "tonight",
    "earlier today",
    "mid-morning",
    "mid-afternoon",
];

fn is_quantity(t: &str) -> bool {
    QUANTITIES.contains(&t) || (!t.is_empty() && t.chars().all(|c| c.is_ascii_digit()))
}

fn canon_qty(t: &str) -> &str {
    if t == "a" || t == "an" {
        "1"
    } else {
        t
    }
}

/// Every elapsed-time or time-of-day claim in `text`, canonicalized
/// ("Four days since" → "4 day", "a week ago" → "1 week", "this morning").
pub fn time_claims(text: &str) -> Vec<String> {
    let toks = tokens(text);
    let mut claims = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        // Fixed phrases go through the same tokenizer, so they match the
        // stream exactly as it was canonicalized ("this" → "thi").
        if let Some((label, len)) = FIXED_CLAIMS.iter().find_map(|f| {
            let ft = tokens(f);
            toks[i..].starts_with(&ft).then_some((*f, ft.len()))
        }) {
            claims.push(label.to_string());
            i += len;
            continue;
        }
        // <qty> [of] <unit> <anchor>   e.g. "4 day since", "a couple of week ago"
        if is_quantity(&toks[i]) {
            let mut j = i + 1;
            if toks.get(j).map(String::as_str) == Some("of") {
                j += 1;
            }
            if let (Some(unit), Some(anchor)) = (toks.get(j), toks.get(j + 1)) {
                if TIME_UNITS.contains(&unit.as_str()) && ANCHORS_AFTER.contains(&anchor.as_str()) {
                    claims.push(format!("{} {}", canon_qty(&toks[i]), unit));
                    i = j + 2;
                    continue;
                }
            }
        }
        i += 1;
    }
    claims
}

/// Honest elapsed time (pack v1 rule 1). Flags any elapsed-time or
/// time-of-day claim in a Mystagogue reply unless the engine supplied that
/// same interval (`known`, the value the core injected as
/// `last_session_ago`; `None` = the engine supplied nothing, so every claim
/// is invented). "Last time" is never a claim. "Yesterday" counts as "1 day".
///
/// Gated judge question: did any reply imply a duration or a time of day
/// ("it's been a while", "late for you") that the context did not give?
pub fn grade_elapsed_time(trace: &SessionTrace, known: Option<&str>) -> CheckResult {
    let known_claims: Vec<String> = known
        .map(|k| {
            let mut v = time_claims(k);
            // A bare "1 day" style value has no anchor word; accept "<n> <unit>".
            let toks = tokens(k);
            for w in toks.windows(2) {
                if is_quantity(&w[0]) && TIME_UNITS.contains(&w[1].as_str()) {
                    v.push(format!("{} {}", canon_qty(&w[0]), w[1]));
                }
            }
            if v.iter().any(|c| c == "1 day") {
                v.push("yesterday".into());
            }
            v
        })
        .unwrap_or_default();

    let mut violations = Vec::new();
    for (n, reply) in replies(trace).iter().enumerate() {
        for claim in time_claims(&reply.text) {
            if !known_claims.contains(&claim) {
                violations.push(format!(
                    "reply {n}: claims \"{claim}\" but the engine supplied {}",
                    known
                        .map(|k| format!("\"{k}\""))
                        .unwrap_or("no interval".into())
                ));
            }
        }
    }
    result(
        "elapsed_time",
        violations,
        "no elapsed-time or time-of-day claim beyond what the engine supplied",
    )
}

// ---- 2. empty turns ------------------------------------------------------

/// An empty voice turn: nothing but dots, ellipses and whitespace. The speech
/// path can send one when it heard nothing; it is not the learner speaking.
pub fn is_empty_turn(text: &str) -> bool {
    text.chars()
        .all(|c| c == '.' || c == '\u{2026}' || c.is_whitespace())
}

/// Phrases that treat an empty turn as a new arrival.
const RETURN_PHRASES: &[&str] = &[
    "you're back",
    "youre back",
    "welcome back",
    "back again",
    "you came back",
    "you returned",
    "second time",
    "third time",
    "fourth time",
];

/// For each learner-turn index, how many consecutive empty learner turns end
/// there (0 if it isn't empty).
fn empty_run_lengths(trace: &SessionTrace) -> Vec<usize> {
    let mut run = 0;
    trace
        .turns
        .iter()
        .map(|t| match t {
            Turn::LearnerText(s) if is_empty_turn(s) => {
                run += 1;
                run
            }
            Turn::LearnerText(_) => {
                run = 0;
                0
            }
            _ => run,
        })
        .collect()
}

/// True when the turn at `idx` sits where the Mystagogue should be landing,
/// not asking: after two consecutive empty learner turns (v1 rule 2) or after
/// the learner signalled they're done (v1 rule 3). Shared with
/// `grade::grade_mask_fidelity`, whose ask-every-turn proxy must not punish a
/// landing line.
pub fn is_landing_context(trace: &SessionTrace, idx: usize) -> bool {
    empty_run_lengths(trace).get(idx).copied().unwrap_or(0) >= 2
}

/// An empty turn is not a turn (pack v1 rule 2). Within a run of empty
/// learner turns:
/// - once the run reaches two, at most two more replies, and none of them
///   asks a question (it has landed; a question chases);
/// - no reply after an empty turn greets an arrival ("you're back");
/// - no `update_memory` while the run lasts (nothing was said to learn from).
///
/// Gated judge question: after the empty turns, did the Mystagogue leave the
/// door open and stop, or did it keep shrinking the question, close and
/// reopen, or treat each empty turn as a visit?
pub fn grade_empty_turns(trace: &SessionTrace) -> CheckResult {
    let runs = empty_run_lengths(trace);
    let mut violations = Vec::new();
    let mut replies_in_landed_run = 0;
    for (n, reply) in replies(trace).iter().enumerate() {
        let Some(li) = reply.after_learner else {
            continue;
        };
        let run = runs[li];
        if run == 0 {
            replies_in_landed_run = 0;
            continue;
        }
        let lower = reply.text.to_lowercase().replace('\u{2019}', "'");
        if let Some(p) = RETURN_PHRASES.iter().find(|p| lower.contains(*p)) {
            violations.push(format!(
                "reply {n}: treats an empty turn as an arrival (\"{p}\")"
            ));
        }
        if reply.tools.iter().any(|(name, _)| name == "update_memory") {
            violations.push(format!("reply {n}: update_memory on an empty turn"));
        }
        if run >= 2 {
            replies_in_landed_run += 1;
            if reply.text.contains('?') {
                violations.push(format!(
                    "reply {n}: still asking after {run} empty turns in a row"
                ));
            }
            if replies_in_landed_run > 2 {
                violations.push(format!(
                    "reply {n}: reply #{replies_in_landed_run} into a landed silence (max 2)"
                ));
            }
        }
    }
    result(
        "empty_turns",
        violations,
        "empty turns met with a door left open: no chasing, no arrivals, no memory writes",
    )
}

// ---- dispatch -----------------------------------------------------------

/// A field grader a persona opts into (each needs at most one fixed
/// parameter the scenario knows by construction).
#[derive(Clone, Copy, Debug)]
pub enum FieldCheck {
    /// v1 rule 1; the interval the engine injected, if any.
    ElapsedTime(Option<&'static str>),
    /// v1 rule 2.
    EmptyTurns,
}

pub fn run_check(check: FieldCheck, trace: &SessionTrace) -> CheckResult {
    match check {
        FieldCheck::ElapsedTime(known) => grade_elapsed_time(trace, known),
        FieldCheck::EmptyTurns => grade_empty_turns(trace),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grade::{assistant, learner, tool};

    fn trace(turns: &[Turn]) -> SessionTrace {
        SessionTrace::new(turns.to_vec())
    }

    #[test]
    fn replies_group_text_and_tools_between_learner_turns() {
        let t = trace(&[
            assistant("Open."),
            learner("hi"),
            assistant("One."),
            tool("open_thread", "{}"),
            assistant("Two."),
            learner("bye"),
        ]);
        let r = replies(&t);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].after_learner, None);
        assert_eq!(r[1].text, "One. Two.");
        assert_eq!(r[1].tools.len(), 1);
        assert_eq!(r[1].after_learner, Some(1));
    }

    // --- 1. elapsed time ----------------------------------------------

    #[test]
    fn time_claims_canonicalize() {
        assert_eq!(time_claims("Four days since you raised it."), vec!["4 day"]);
        assert_eq!(time_claims("A week ago you said"), vec!["1 week"]);
        assert_eq!(
            time_claims("You left it hanging this afternoon."),
            vec!["this afternoon"]
        );
        assert!(time_claims("Last time you left a question open.").is_empty());
        assert!(time_claims("Nothing set today, and that's fine.").is_empty());
        assert!(time_claims("In a week, would it still hold?").is_empty());
    }

    #[test]
    fn invented_interval_fails_when_engine_supplied_none() {
        let bad = trace(&[
            assistant("Four days since you raised the tide question. Still there?"),
            learner("yeah"),
        ]);
        assert!(!grade_elapsed_time(&bad, None).passed);

        let bad_tod = trace(&[assistant("You left it smouldering this afternoon. Still?")]);
        assert!(!grade_elapsed_time(&bad_tod, None).passed);
    }

    #[test]
    fn last_time_is_always_honest() {
        let ok = trace(&[assistant(
            "Last time you left the tide question open. What's it doing now?",
        )]);
        assert!(grade_elapsed_time(&ok, None).passed);
    }

    #[test]
    fn a_claim_matching_the_supplied_interval_passes_and_a_wrong_one_fails() {
        let ok = trace(&[assistant(
            "A day ago you left the tide question open. Still live?",
        )]);
        assert!(grade_elapsed_time(&ok, Some("1 day")).passed);
        let ok_yday = trace(&[assistant("Yesterday you left it open. Still live?")]);
        assert!(grade_elapsed_time(&ok_yday, Some("1 day")).passed);

        let wrong = trace(&[assistant("Two weeks back you left it open. Still live?")]);
        let r = grade_elapsed_time(&wrong, Some("1 day"));
        assert!(!r.passed);
        assert!(r.detail.contains("2 week"), "{}", r.detail);
    }

    // --- 2. empty turns -------------------------------------------------

    #[test]
    fn empty_turn_detection() {
        assert!(is_empty_turn("."));
        assert!(is_empty_turn(". . ."));
        assert!(is_empty_turn("  \u{2026} "));
        assert!(!is_empty_turn("no."));
    }

    #[test]
    fn chasing_an_empty_run_fails() {
        // The bad shape: shrink the question each time, greet each empty
        // turn as an arrival, write memory on silence.
        let bad = trace(&[
            assistant("What's alive in it?"),
            learner("."),
            assistant("One word is enough. Anything?"),
            learner(". . ."),
            assistant("You're back. What did you have for lunch?"),
            tool(
                "update_memory",
                r#"{"section":"working_history","content":"went quiet"}"#,
            ),
            learner("."),
            assistant("Tired, or busy?"),
            learner("."),
            assistant("We'll leave it here."),
        ]);
        let r = grade_empty_turns(&bad);
        assert!(!r.passed);
        for needle in ["arrival", "update_memory", "still asking", "max 2"] {
            assert!(r.detail.contains(needle), "missing {needle}: {}", r.detail);
        }
    }

    #[test]
    fn leaving_the_door_open_passes() {
        let ok = trace(&[
            assistant("What's alive in it?"),
            learner("."),
            assistant("Anything there, even one word?"),
            learner(". . ."),
            assistant("Nothing has to come. The door stays open."),
            learner("."),
        ]);
        assert!(
            grade_empty_turns(&ok).passed,
            "{:?}",
            grade_empty_turns(&ok)
        );
    }

    #[test]
    fn a_real_turn_resets_the_run() {
        let ok = trace(&[
            learner("."),
            learner("."),
            assistant("The door stays open."),
            learner("sorry, the mic. It's the tide thing."),
            assistant("The tide thing. What about it?"),
        ]);
        assert!(grade_empty_turns(&ok).passed);
    }
}
