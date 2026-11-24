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
}
