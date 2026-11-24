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
        || learner_before(trace, idx).is_some_and(is_leave_signal)
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

// ---- 3. one question; the last turn lands -------------------------------

/// Number of question sentences in `text` ("Who issued it — did you? And
/// when?" is two).
pub fn question_count(text: &str) -> usize {
    crate::grade::terminated_sentences(text)
        .iter()
        .filter(|(_, t)| *t == '?')
        .count()
}

/// Phrases a learner uses to say they're done for now.
const LEAVE_SIGNALS: &[&str] = &[
    "have to go",
    "got to go",
    "gotta go",
    "need to go",
    "i'm done",
    "im done",
    "that's enough",
    "thats enough",
    "enough for today",
    "stop here",
    "signing off",
    "heading out",
    "leave it there",
    "call it a night",
    "call it a day",
];

pub fn is_leave_signal(text: &str) -> bool {
    let lower = text.to_lowercase().replace('\u{2019}', "'");
    LEAVE_SIGNALS.iter().any(|p| lower.contains(p))
}

/// The learner turn nearest before `idx`, if any.
fn learner_before(trace: &SessionTrace, idx: usize) -> Option<&str> {
    trace.turns[..idx].iter().rev().find_map(|t| match t {
        Turn::LearnerText(s) => Some(s.as_str()),
        _ => None,
    })
}

/// One live question per reply (pack v1 rule 3, first half).
///
/// Gated judge question: did any reply hand the learner a choice of
/// questions, or ask a second question inside the first ("…, and why?")?
pub fn grade_one_question(trace: &SessionTrace) -> CheckResult {
    let violations = replies(trace)
        .iter()
        .enumerate()
        .filter_map(|(n, r)| {
            let q = question_count(&r.text);
            (q > 1).then(|| format!("reply {n}: {q} questions in \"{}\"", r.text))
        })
        .collect();
    result(
        "one_question",
        violations,
        "no reply asks more than one question",
    )
}

/// The final turn lands, it doesn't ask (pack v1 rule 3, second half). When
/// a learner turn signals they're done, the reply to it must exist (the
/// Mystagogue lands it) and must contain no question.
///
/// Gated judge question: did the last reply name what stayed open in the
/// learner's words and stop, or did it ask something on the way out?
pub fn grade_landing(trace: &SessionTrace) -> CheckResult {
    let all = replies(trace);
    let mut violations = Vec::new();
    for (i, turn) in trace.turns.iter().enumerate() {
        let Turn::LearnerText(text) = turn else {
            continue;
        };
        if !is_leave_signal(text) {
            continue;
        }
        match all.iter().find(|r| r.after_learner == Some(i)) {
            None => violations.push(format!(
                "turn {i}: learner signalled done; no landing reply"
            )),
            Some(r) if r.text.contains('?') => violations.push(format!(
                "turn {i}: learner signalled done; the reply still asks: \"{}\"",
                r.text
            )),
            Some(r) if r.text.trim().is_empty() => violations.push(format!(
                "turn {i}: learner signalled done; the reply says nothing"
            )),
            Some(_) => {}
        }
    }
    result(
        "landing",
        violations,
        "every done-signal met with a landing that asks nothing",
    )
}

// ---- 4. forks ------------------------------------------------------------

/// "or" uses that aren't a choice between two pictures.
const NOT_A_FORK: &[&str] = &[
    " or not",
    " or so",
    " or something",
    " or anything",
    " or what",
    " or whatever",
];

/// Either/or questions in `text`: question sentences offering two options.
pub fn forks(text: &str) -> Vec<String> {
    crate::grade::terminated_sentences(text)
        .into_iter()
        .filter(|(_, t)| *t == '?')
        .map(|(s, _)| s.trim().to_string())
        .filter(|s| {
            let mut l = format!(" {} ", s.to_lowercase());
            for x in NOT_A_FORK {
                l = l.replace(x, " ");
            }
            l.contains(" or ") || l.contains(", or")
        })
        .collect()
}

/// A learner turn that only picks ("the first one", "the latter", one word).
pub fn is_pick(text: &str) -> bool {
    tokens(text).len() <= 4
}

/// Ration forks (pack v1 rule 4): at most one either/or question per
/// session, and no `fix_salt` whose nearest learner turn is only a pick
/// answering a fork (the picked option is the Mystagogue's picture, not the
/// learner's grain).
///
/// Gated judge question: did the learner's best images come from their own
/// words or from options the Mystagogue offered, and did any fixed salt
/// rest on an offered option the learner only picked?
pub fn grade_forks(trace: &SessionTrace) -> CheckResult {
    let all = replies(trace);
    let mut violations = Vec::new();
    let mut total = 0;
    for (n, r) in all.iter().enumerate() {
        let f = forks(&r.text);
        total += f.len();
        if total > 1 && !f.is_empty() {
            violations.push(format!(
                "reply {n}: fork #{total} this session: \"{}\"",
                f[0]
            ));
        }
        if r.tools.iter().any(|(name, _)| name == "fix_salt") {
            if let Some(li) = r.after_learner {
                let Turn::LearnerText(pick) = &trace.turns[li] else {
                    continue;
                };
                let forked_before = n > 0 && !forks(&all[n - 1].text).is_empty();
                if forked_before && is_pick(pick) {
                    violations.push(format!(
                        "reply {n}: fix_salt on a picked option (\"{pick}\"), not the learner's own words"
                    ));
                }
            }
        }
    }
    result(
        "forks",
        violations,
        "at most one either/or, and no salt fixed on a pick",
    )
}

// ---- 5. stalled questions ------------------------------------------------

fn question_sentences(text: &str) -> Vec<String> {
    crate::grade::terminated_sentences(text)
        .into_iter()
        .filter(|(_, t)| *t == '?')
        .map(|(s, _)| s.trim().to_string())
        .collect()
}

/// Don't re-ask a stalled question (pack v1 rule 5). `stalled` are the
/// questions the prior session's note marked stalled. Violations: the
/// opening reply asks one of them again (Dice >= 0.5), or any question is
/// asked a third time in one session (Dice >= 0.7 against two earlier ones).
///
/// Gated judge question: given a prior note marking a question stalled, did
/// the opening come at it sideways or open something else, rather than ask
/// it again in new words?
pub fn grade_no_reask(trace: &SessionTrace, stalled: &[&str]) -> CheckResult {
    use crate::normalize::{dice, token_set};
    let all = replies(trace);
    let mut violations = Vec::new();
    if let Some(open) = all.first() {
        for q in question_sentences(&open.text) {
            for st in stalled {
                let d = dice(&token_set(&q), &token_set(st));
                if d >= 0.5 {
                    violations.push(format!(
                        "opening re-asks the stalled question \"{st}\" (dice {d:.2}): \"{q}\""
                    ));
                }
            }
        }
    }
    let mut asked: Vec<String> = Vec::new();
    for (n, r) in all.iter().enumerate() {
        for q in question_sentences(&r.text) {
            let repeats = asked
                .iter()
                .filter(|a| dice(&token_set(a), &token_set(&q)) >= 0.7)
                .count();
            if repeats >= 2 {
                violations.push(format!("reply {n}: asks a third time: \"{q}\""));
            }
            asked.push(q);
        }
    }
    result(
        "no_reask",
        violations,
        "no stalled question reopened, none asked a third time",
    )
}

// ---- 6. offer on a mechanism; record a spoken rhyme ----------------------

/// Phrases that invite the learner to say the grain (condensation §2).
const OFFER_PHRASES: &[&str] = &[
    "say it",
    "say that",
    "in your own words",
    "your own words",
    "now know",
    "something just set",
    "want to keep",
];

/// Phrases that claim a cross-domain rhyme. Deliberately narrow:
/// "you keep coming back to that word" is a within-session reflection, not
/// a rhyme, and is not listed.
const RHYME_PHRASES: &[&str] = &[
    "keep reaching for",
    "rhymes with",
    "same shape as",
    "same shape you",
    "the same move you made",
];

/// Offer condensation when the learner explains a mechanism in their own
/// words (pack v1 rule 6, first half). `mechanism` is the learner turn the
/// scenario knows is one (by construction); within the next two replies the
/// Mystagogue must offer (an invitation phrase) or fix salt.
///
/// Gated judge question: when the learner explained how something works in
/// their own words, even inside a tangent, did the Mystagogue notice and
/// offer condensation, or steer back to its own thread?
pub fn grade_offer_after(trace: &SessionTrace, mechanism: &str) -> CheckResult {
    let all = replies(trace);
    let Some(li) = trace
        .turns
        .iter()
        .position(|t| matches!(t, Turn::LearnerText(s) if s == mechanism))
    else {
        return result(
            "offer_after_mechanism",
            vec!["scenario's mechanism turn not found in trace".into()],
            "",
        );
    };
    let window: Vec<&Reply> = all
        .iter()
        .filter(|r| r.after_learner.is_some_and(|i| i >= li))
        .take(2)
        .collect();
    let offered = window.iter().any(|r| {
        let l = r.text.to_lowercase();
        OFFER_PHRASES.iter().any(|p| l.contains(p)) || r.tools.iter().any(|(n, _)| n == "fix_salt")
    });
    result(
        "offer_after_mechanism",
        if offered {
            vec![]
        } else {
            vec![format!(
                "no offer within two replies of the learner's own-words mechanism at turn {li}"
            )]
        },
        "own-words mechanism met with an offer within two replies",
    )
}

/// Record a spoken rhyme (pack v1 rule 6, second half): a reply that claims
/// a cross-domain rhyme must be followed by `weave_domains` in that reply or
/// the next (once the learner takes it up).
///
/// Gated judge question: did the Mystagogue name a link between two of the
/// learner's domains without recording it, or record one the learner never
/// took up?
pub fn grade_weave_on_rhyme(trace: &SessionTrace) -> CheckResult {
    let all = replies(trace);
    let mut violations = Vec::new();
    for (n, r) in all.iter().enumerate() {
        let l = r.text.to_lowercase();
        let Some(p) = RHYME_PHRASES.iter().find(|p| l.contains(*p)) else {
            continue;
        };
        let woven = all[n..]
            .iter()
            .take(2)
            .any(|r| r.tools.iter().any(|(name, _)| name == "weave_domains"));
        if !woven {
            violations.push(format!(
                "reply {n}: names a rhyme (\"{p}\") but no weave_domains follows"
            ));
        }
    }
    result(
        "weave_on_rhyme",
        violations,
        "every spoken rhyme is recorded with weave_domains",
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
    /// v1 rule 3.
    OneQuestion,
    /// v1 rule 3.
    Landing,
    /// v1 rule 4.
    Forks,
    /// v1 rule 5; the questions the prior note marked stalled.
    NoReask(&'static [&'static str]),
    /// v1 rule 6; the learner turn that explains a mechanism.
    OfferAfter(&'static str),
    /// v1 rule 6.
    WeaveOnRhyme,
}

pub fn run_check(check: FieldCheck, trace: &SessionTrace) -> CheckResult {
    match check {
        FieldCheck::ElapsedTime(known) => grade_elapsed_time(trace, known),
        FieldCheck::EmptyTurns => grade_empty_turns(trace),
        FieldCheck::OneQuestion => grade_one_question(trace),
        FieldCheck::Landing => grade_landing(trace),
        FieldCheck::Forks => grade_forks(trace),
        FieldCheck::NoReask(stalled) => grade_no_reask(trace, stalled),
        FieldCheck::OfferAfter(mechanism) => grade_offer_after(trace, mechanism),
        FieldCheck::WeaveOnRhyme => grade_weave_on_rhyme(trace),
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

    // --- 3. one question; landing ----------------------------------------

    #[test]
    fn stacked_questions_fail() {
        let bad = trace(&[assistant(
            "Who set that rule — did you? And if you did, when?",
        )]);
        assert!(!grade_one_question(&bad).passed);
        let ok = trace(&[assistant("Who set that rule?")]);
        assert!(grade_one_question(&ok).passed);
    }

    #[test]
    fn asking_on_the_way_out_fails_and_landing_passes() {
        let bad = trace(&[
            assistant("What's holding the other end?"),
            learner("not sure. I have to go, actually."),
            assistant("Before you go: what would it take to find out?"),
        ]);
        assert!(!grade_landing(&bad).passed);

        let silent_exit = trace(&[
            assistant("What's holding the other end?"),
            learner("I have to go."),
        ]);
        assert!(
            !grade_landing(&silent_exit).passed,
            "a done-signal needs a landing"
        );

        let ok = trace(&[
            assistant("What's holding the other end?"),
            learner("not sure. I have to go, actually."),
            assistant("The other end stays open. We'll start there."),
        ]);
        assert!(grade_landing(&ok).passed);
        assert!(is_landing_context(&ok, 2));
    }

    // --- 4. forks --------------------------------------------------------

    #[test]
    fn fork_detection() {
        assert_eq!(forks("Is it a ladder, or a river?").len(), 1);
        assert_eq!(forks("Rested or restless?").len(), 1);
        assert!(forks("Does it hold or not?").is_empty());
        assert!(forks("What else is there?").is_empty());
        assert!(
            forks("It's a ladder or a river.").is_empty(),
            "not a question"
        );
    }

    #[test]
    fn fork_every_turn_and_salt_on_a_pick_fail() {
        let bad = trace(&[
            assistant("Is the moon pulling, or is the water leaning?"),
            learner("the first one"),
            assistant("A rope or a sheet?"),
            learner("the second"),
            tool(
                "fix_salt",
                r#"{"realization":"the moon pulls a sheet of water"}"#,
            ),
            tool("open_thread", r#"{"question":"what holds the other end?"}"#),
        ]);
        let r = grade_forks(&bad);
        assert!(!r.passed);
        assert!(r.detail.contains("fork #2"), "{}", r.detail);
        assert!(r.detail.contains("picked option"), "{}", r.detail);
    }

    #[test]
    fn one_fork_then_their_own_picture_passes() {
        let ok = trace(&[
            assistant("Is the moon pulling, or is the water leaning?"),
            learner("the first one"),
            assistant("Pulling — say it your way: what does it look like?"),
            learner(
                "like someone lifting a corner of a bedsheet and everything slides toward them",
            ),
            tool(
                "fix_salt",
                r#"{"realization":"lifting a corner of a bedsheet and everything slides toward them"}"#,
            ),
            tool("open_thread", r#"{"question":"who lifts the corner?"}"#),
        ]);
        assert!(grade_forks(&ok).passed, "{:?}", grade_forks(&ok));
    }

    // --- 5. stalled questions ---------------------------------------------

    #[test]
    fn reopening_a_stalled_question_fails() {
        let bad = trace(&[assistant(
            "Last time: what's holding the other end of the sheet?",
        )]);
        assert!(!grade_no_reask(&bad, &["what's holding the other end?"]).passed);

        let sideways = trace(&[assistant(
            "Last time you had the moon lifting a sheet — where else have you seen something pulled from far away?",
        )]);
        assert!(grade_no_reask(&sideways, &["what's holding the other end?"]).passed);
    }

    #[test]
    fn a_third_asking_fails() {
        let bad = trace(&[
            assistant("What's holding the other end?"),
            learner("hm"),
            assistant("So what's holding the other end?"),
            learner("dunno"),
            assistant("Really — what is holding the other end?"),
        ]);
        let r = grade_no_reask(&bad, &[]);
        assert!(!r.passed);
        assert!(r.detail.contains("third time"), "{}", r.detail);
    }

    // --- 6. offer; weave ---------------------------------------------------

    const MECH: &str = "it rises hours later because the yeast has to eat through the flour first";

    #[test]
    fn steering_past_a_mechanism_fails_and_an_offer_passes() {
        let bad = trace(&[
            assistant("Why does the tide wait?"),
            learner(MECH),
            assistant("Interesting. Back to the tide: why does it wait?"),
            learner("no idea"),
            assistant("Think about the moon."),
        ]);
        assert!(!grade_offer_after(&bad, MECH).passed);

        let ok = trace(&[
            assistant("Why does the tide wait?"),
            learner(MECH),
            assistant("The feeding and the rise aren't the same moment. Say that as the thing you now know."),
        ]);
        assert!(grade_offer_after(&ok, MECH).passed);
    }

    #[test]
    fn a_spoken_rhyme_needs_a_weave() {
        let bad = trace(&[
            assistant("You keep reaching for delay — the tide, the starter. One thing?"),
            learner("yeah, same thing"),
            assistant("Good."),
        ]);
        assert!(!grade_weave_on_rhyme(&bad).passed);

        let ok = trace(&[
            assistant("You keep reaching for delay — the tide, the starter. One thing?"),
            learner("yeah, same thing"),
            tool(
                "weave_domains",
                r#"{"a":"tides","b":"baking","note":"delay is the point"}"#,
            ),
        ]);
        assert!(grade_weave_on_rhyme(&ok).passed);

        let reflection = trace(&[assistant(
            "You keep coming back to that word. What is it doing?",
        )]);
        assert!(
            grade_weave_on_rhyme(&reflection).passed,
            "not a rhyme claim"
        );
    }
}
