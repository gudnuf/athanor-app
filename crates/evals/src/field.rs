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

/// An empty voice turn: no letter or digit at all (dots, ellipses, a lone
/// "?", whitespace). The speech path can send one when it heard nothing; it
/// is not the learner speaking.
pub fn is_empty_turn(text: &str) -> bool {
    // Mirrors athanor_core's Conductor::is_contentful_turn exactly: a turn
    // with no letter or digit is not speech.
    !text.chars().any(char::is_alphanumeric)
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

// ---- 7. "save that" means their words ------------------------------------

/// Ways a learner asks the Mystagogue to keep something.
const KEEP_REQUESTS: &[&str] = &[
    "bank that",
    "bank this",
    "bank it",
    "save that",
    "save this",
    "keep that",
    "hold onto that",
    "hold on to that",
    "write that down",
];

/// Tools that store learner material, and the arg that carries the text.
const STORING_TOOLS: &[(&str, &str)] = &[
    ("bank_pull", "text"),
    ("open_thread", "question"),
    ("update_memory", "content"),
];

fn arg_text(args: &str, key: &str) -> String {
    serde_json::from_str::<serde_json::Value>(args)
        .ok()
        .and_then(|v| v.get(key).and_then(|s| s.as_str()).map(str::to_string))
        .unwrap_or_default()
}

/// Fraction of `stored`'s content tokens that appear in `source`.
fn containment(stored: &str, source: &str) -> f64 {
    use crate::normalize::token_set;
    let st = token_set(stored);
    if st.is_empty() {
        return 0.0;
    }
    let so = token_set(source);
    st.intersection(&so).count() as f64 / st.len() as f64
}

/// A keep request stores the learner's words (pack v1 rule 7). After a
/// learner turn asking to keep something, the next storing tool call
/// (`bank_pull`, `open_thread`, `update_memory`) must carry text at least
/// 60% drawn from the learner's own words (that turn plus the one before,
/// for "…yes, save that"). Fixing salt instead is a violation (an idea is
/// not a realization), as is storing nothing or asking more than one
/// question in the reply.
///
/// Gated judge question: did the stored text read as the learner's
/// sentence, or as the Mystagogue's summary of it?
pub fn grade_bank_verbatim(trace: &SessionTrace) -> CheckResult {
    let all = replies(trace);
    let mut violations = Vec::new();
    for (i, turn) in trace.turns.iter().enumerate() {
        let Turn::LearnerText(text) = turn else {
            continue;
        };
        let lower = text.to_lowercase();
        if !KEEP_REQUESTS.iter().any(|k| lower.contains(k)) {
            continue;
        }
        let mut source = text.clone();
        if let Some(prev) = learner_before(trace, i) {
            source.push(' ');
            source.push_str(prev);
        }
        let Some(reply) = all.iter().find(|r| r.after_learner == Some(i)) else {
            violations.push(format!("turn {i}: keep request with no reply"));
            continue;
        };
        if reply.tools.iter().any(|(n, _)| n == "fix_salt") {
            violations.push(format!("turn {i}: keep request fixed as salt"));
        }
        let stored = reply.tools.iter().find_map(|(n, args)| {
            STORING_TOOLS
                .iter()
                .find(|(t, _)| t == n)
                .map(|(_, key)| arg_text(args, key))
        });
        match stored {
            None => violations.push(format!("turn {i}: keep request, nothing stored")),
            Some(st) => {
                let c = containment(&st, &source);
                if c < 0.6 {
                    violations.push(format!(
                        "turn {i}: stored text is {:.0}% the learner's words (min 60%): \"{st}\"",
                        c * 100.0
                    ));
                }
            }
        }
        let q = question_count(&reply.text);
        if q > 1 {
            violations.push(format!(
                "turn {i}: {q} questions after a keep request (max 1)"
            ));
        }
    }
    result(
        "bank_verbatim",
        violations,
        "every keep request stored in the learner's own words",
    )
}

// ---- 8. speech meant for someone else ------------------------------------

/// Phrases that check who a turn was for.
const ADDRESSEE_CHECKS: &[&str] = &[
    "for me",
    "to me?",
    "talking to me",
    "someone else",
    "someone there",
];

/// Room speech (pack v1 rule 8). `room` are the learner turns the scenario
/// knows were addressed to someone else in the room. The Mystagogue checks
/// once, lightly, whether a room turn was for it: at least one check across
/// the session, and never a check on a turn that WAS for it (re-asking after
/// the learner answered is pressing). A fresh room turn may get its own
/// check. It never answers a room turn's content (Dice >= 0.3 between a
/// non-check reply and the room line), and never stores it (an
/// `update_memory` whose content is >= 50% drawn from a room line).
///
/// Gated judge question (also over the close NOTE/PROFILE, which this trace
/// doesn't carry): did anything the Mystagogue said, noted or profiled rest
/// on a turn that was plausibly for someone else?
pub fn grade_room_speech(trace: &SessionTrace, room: &[&str]) -> CheckResult {
    use crate::normalize::{dice, token_set};
    let all = replies(trace);
    let mut violations = Vec::new();
    let mut checks = 0;
    for (i, turn) in trace.turns.iter().enumerate() {
        let Turn::LearnerText(text) = turn else {
            continue;
        };
        let Some(r) = all.iter().find(|r| r.after_learner == Some(i)) else {
            continue;
        };
        let lower = r.text.to_lowercase();
        let is_check = ADDRESSEE_CHECKS.iter().any(|p| lower.contains(p));
        if !room.contains(&text.as_str()) {
            if is_check {
                violations.push(format!(
                    "turn {i}: checks the addressee on a turn that was for it (after the learner answered): \"{}\"",
                    r.text
                ));
            }
            continue;
        }
        if is_check {
            checks += 1;
        } else {
            let d = dice(&token_set(&r.text), &token_set(text));
            if d >= 0.3 {
                violations.push(format!(
                    "turn {i}: answers speech meant for someone else (dice {d:.2}): \"{}\"",
                    r.text
                ));
            }
        }
    }
    for (n, r) in all.iter().enumerate() {
        for (name, args) in &r.tools {
            if name != "update_memory" {
                continue;
            }
            let content = arg_text(args, "content");
            for line in room {
                if containment(&content, line) >= 0.5 {
                    violations.push(format!(
                        "reply {n}: update_memory built on room speech \"{line}\""
                    ));
                }
            }
        }
    }
    if !room.is_empty() && checks == 0 {
        violations.push("never checked whether a room turn was for it".into());
    }
    result(
        "room_speech",
        violations,
        "room speech checked lightly, not pressed, not answered, not stored",
    )
}

// ---- 9. no learning style from silence ----------------------------------

/// Words that make a `how_i_learn` claim out of quiet, brevity or absence.
const SILENCE_CLAIMS: &[&str] = &[
    "silence",
    "silent",
    "quiet",
    "pause",
    "terse",
    "short repl",
    "short answer",
    "near-empty",
    "few words",
    "one-word",
    "gap",
    "goes dark",
    "went dark",
];

/// The close-pass profile refine never infers learning style from silence,
/// short turns or gaps (pack v1 rule 9). `condense_output` is the close
/// pass's raw text, parsed by the real `parse_condensation`. Violations:
/// - any `how_i_learn` addition when the session gave nothing to learn from
///   (every learner turn empty or at most three tokens);
/// - any `how_i_learn` addition whose claim rests on silence, brevity or
///   gaps, whatever the session held.
///
/// Gated judge question: does each `how_i_learn` line rest on something the
/// learner said or did with words, seen more than once, or on one session's
/// quiet?
pub fn grade_profile_from_silence(trace: &SessionTrace, condense_output: &str) -> CheckResult {
    let parsed = athanor_core::prompt::parse_condensation(condense_output);
    let thin = trace
        .turns
        .iter()
        .filter_map(|t| match t {
            Turn::LearnerText(s) => Some(s),
            _ => None,
        })
        .all(|s| is_empty_turn(s) || tokens(s).len() <= 3);
    let mut violations = Vec::new();
    for (section, text) in &parsed.profile_updates {
        if section != "how_i_learn" {
            continue;
        }
        if thin {
            violations.push(format!(
                "how_i_learn written from a session with no substantive learner turn: \"{text}\""
            ));
        }
        let lower = text.to_lowercase();
        if let Some(w) = SILENCE_CLAIMS.iter().find(|w| lower.contains(*w)) {
            violations.push(format!("how_i_learn claim rests on \"{w}\": \"{text}\""));
        }
    }
    result(
        "profile_from_silence",
        violations,
        "no how_i_learn claim built on silence, short turns or gaps",
    )
}

// ---- 10. say less than they did -----------------------------------------

/// Spoken words in `text` (whitespace runs with at least one letter/digit).
pub fn word_count(text: &str) -> usize {
    text.split_whitespace()
        .filter(|w| w.chars().any(char::is_alphanumeric))
        .count()
}

/// Below this many learner words in a session, a ratio is noise (one terse
/// learner makes any reply "too long"); only the per-reply cap applies.
pub const TALK_RATIO_MIN_LEARNER_WORDS: usize = 30;
/// identity §2: "A session where you talked more than they did is a session
/// you lost." Taken literally: Mystagogue words <= learner words.
pub const TALK_RATIO_MAX: f64 = 1.0;
/// identity §2's number: under twenty-five words a reply, the opening too.
/// One reflection and one question, about one spoken breath.
pub const REPLY_MAX_WORDS: usize = 25;
/// ...unless the learner's last turn ran longer than this.
pub const LONG_LEARNER_TURN_WORDS: usize = 60;

/// Say less than they did (pack v1 rule 10), as identity §2 states it:
/// - every reply, the opening move included, stays within
///   [`REPLY_MAX_WORDS`] unless the learner turn it answers ran over
///   [`LONG_LEARNER_TURN_WORDS`];
/// - once the learner has said [`TALK_RATIO_MIN_LEARNER_WORDS`] or more
///   (empty turns excluded), Mystagogue words in replies / learner words
///   must not exceed [`TALK_RATIO_MAX`]. The opening move answers no learner
///   turn, so it is not charged to the ratio.
///
/// Gated judge question: did the Mystagogue talk more than the learner, or
/// answer a short turn with a paragraph?
pub fn grade_talk_ratio(trace: &SessionTrace) -> CheckResult {
    let learner_words: usize = trace
        .turns
        .iter()
        .filter_map(|t| match t {
            Turn::LearnerText(s) if !is_empty_turn(s) => Some(word_count(s)),
            _ => None,
        })
        .sum();
    let all = replies(trace);
    let mut violations = Vec::new();
    for (n, r) in all.iter().enumerate() {
        let answered = r
            .after_learner
            .and_then(|i| match &trace.turns[i] {
                Turn::LearnerText(s) => Some(word_count(s)),
                _ => None,
            })
            .unwrap_or(0);
        let w = word_count(&r.text);
        if w > REPLY_MAX_WORDS && answered <= LONG_LEARNER_TURN_WORDS {
            let what = if r.after_learner.is_none() {
                "opening"
            } else {
                "reply"
            };
            violations.push(format!(
                "{what} {n}: {w} words to a {answered}-word turn (max {REPLY_MAX_WORDS})"
            ));
        }
    }
    if learner_words >= TALK_RATIO_MIN_LEARNER_WORDS {
        let mine: usize = all
            .iter()
            .filter(|r| r.after_learner.is_some())
            .map(|r| word_count(&r.text))
            .sum();
        let ratio = mine as f64 / learner_words as f64;
        if ratio > TALK_RATIO_MAX {
            violations.push(format!(
                "Mystagogue {mine} words vs learner {learner_words} (ratio {ratio:.2} > {TALK_RATIO_MAX})"
            ));
        }
    }
    result(
        "talk_ratio",
        violations,
        "every reply under the word budget, and less said than the learner",
    )
}

// ---- 11. a landed reply is one short line -------------------------------

/// A reply to an empty turn or a done-signal: at most this many sentences
/// and words. One spoken line, about seven seconds aloud.
pub const LANDED_MAX_SENTENCES: usize = 2;
pub const LANDED_MAX_WORDS: usize = 20;

/// A landed reply is one short line (pack v1 rules 2–3, "knowing when to
/// stop"). Every reply to an empty learner turn or to a done-signal must fit
/// in [`LANDED_MAX_SENTENCES`] sentences and [`LANDED_MAX_WORDS`] words.
///
/// Gated judge question: was each reply to an empty turn or a goodbye one
/// short line, or did it keep talking?
pub fn grade_landed_reply(trace: &SessionTrace) -> CheckResult {
    let mut violations = Vec::new();
    for (n, r) in replies(trace).iter().enumerate() {
        let Some(li) = r.after_learner else {
            continue;
        };
        let Turn::LearnerText(l) = &trace.turns[li] else {
            continue;
        };
        if !(is_empty_turn(l) || is_leave_signal(l)) {
            continue;
        }
        let mut sentences = crate::grade::terminated_sentences(&r.text).len();
        let tail_has_words = r
            .text
            .rsplit(['.', '!', '?'])
            .next()
            .is_some_and(|t| word_count(t) > 0);
        if tail_has_words {
            sentences += 1;
        }
        let words = word_count(&r.text);
        if sentences > LANDED_MAX_SENTENCES || words > LANDED_MAX_WORDS {
            violations.push(format!(
                "reply {n}: {sentences} sentences / {words} words to an empty turn or goodbye (max {LANDED_MAX_SENTENCES} / {LANDED_MAX_WORDS}): \"{}\"",
                r.text
            ));
        }
    }
    result(
        "landed_reply",
        violations,
        "every reply to an empty turn or a goodbye is one short line",
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
    /// v1 rule 7.
    BankVerbatim,
    /// v1 rule 8; the learner turns addressed to someone else.
    RoomSpeech(&'static [&'static str]),
    /// v1 rule 10.
    TalkRatio,
    /// v1 rules 2–3: a landed reply is one short line.
    LandedReply,
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
        FieldCheck::BankVerbatim => grade_bank_verbatim(trace),
        FieldCheck::RoomSpeech(room) => grade_room_speech(trace, room),
        FieldCheck::TalkRatio => grade_talk_ratio(trace),
        FieldCheck::LandedReply => grade_landed_reply(trace),
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
        // Same line as core's Conductor::is_contentful_turn: any letter or
        // digit is speech; punctuation alone is not.
        assert!(is_empty_turn("?"));
        assert!(is_empty_turn(" — ! "));
        assert!(!is_empty_turn("1"));
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
        // Run-1 shape: one question, then the same one again in new words.
        let restated = trace(&[assistant(
            "What does \"waits\" mean there? What is the tide waiting for?",
        )]);
        assert!(!grade_one_question(&restated).passed);
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

    const MECH: &str = "the rope goes tight long before the boat moves because the wet sand has to let go of the hull first";

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
            assistant("The pull and the slide aren't the same moment. Say that as the thing you now know."),
        ]);
        assert!(grade_offer_after(&ok, MECH).passed);
    }

    #[test]
    fn a_spoken_rhyme_needs_a_weave() {
        let bad = trace(&[
            assistant("You keep reaching for delay — the tide, the kayak. One thing?"),
            learner("yeah, same thing"),
            assistant("Good."),
        ]);
        assert!(!grade_weave_on_rhyme(&bad).passed);

        let ok = trace(&[
            assistant("You keep reaching for delay — the tide, the kayak. One thing?"),
            learner("yeah, same thing"),
            tool(
                "weave_domains",
                r#"{"a":"tides","b":"boats","note":"delay is the point"}"#,
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

    // --- 7. keep requests --------------------------------------------------

    const IDEA: &str = "I want a tide clock app where the phone buzzes when the water at my beach turns, not at a fixed time. save that.";

    #[test]
    fn a_paraphrased_bank_fails() {
        let bad = trace(&[
            learner(IDEA),
            tool(
                "open_thread",
                r#"{"question":"Event-driven ambient notification as a design principle versus scheduled alerting"}"#,
            ),
            assistant("Saved. Is it about control? Or about attention?"),
        ]);
        let r = grade_bank_verbatim(&bad);
        assert!(!r.passed);
        assert!(
            r.detail.contains("min 60%") && r.detail.contains("max 1"),
            "{}",
            r.detail
        );

        let as_salt = trace(&[
            learner(IDEA),
            tool("fix_salt", r#"{"realization":"tide clock"}"#),
        ]);
        assert!(!grade_bank_verbatim(&as_salt).passed);

        let nothing = trace(&[learner(IDEA), assistant("Saved.")]);
        assert!(!grade_bank_verbatim(&nothing).passed);
    }

    #[test]
    fn their_words_stored_passes() {
        let ok = trace(&[
            learner(IDEA),
            tool(
                "open_thread",
                r#"{"question":"a tide clock app where the phone buzzes when the water at my beach turns, not at a fixed time"}"#,
            ),
            assistant("Saved, in your words — what would the buzz tell you?"),
        ]);
        assert!(
            grade_bank_verbatim(&ok).passed,
            "{:?}",
            grade_bank_verbatim(&ok)
        );
    }

    // --- 8. room speech ------------------------------------------------------

    const ROOM: &[&str] = &[
        "oh hi, no, go ahead, I'll be five minutes",
        "they think it's a meditation app but it's more like a sparring partner",
    ];

    #[test]
    fn answering_and_profiling_room_speech_fails() {
        let bad = trace(&[
            assistant("What's alive in it?"),
            learner(ROOM[0]),
            assistant("Five minutes is fine. Where were we?"),
            learner(ROOM[1]),
            assistant("A sparring partner, not a meditation app. Is that what you want?"),
            tool(
                "update_memory",
                r#"{"section":"how_i_learn","content":"sees this as a sparring partner, not a meditation app"}"#,
            ),
        ]);
        let r = grade_room_speech(&bad, ROOM);
        assert!(!r.passed);
        for needle in ["answers speech", "update_memory built", "never checked"] {
            assert!(r.detail.contains(needle), "missing {needle}: {}", r.detail);
        }
    }

    #[test]
    fn one_light_check_then_carry_on_passes() {
        let ok = trace(&[
            assistant("What's alive in it?"),
            learner(ROOM[0]),
            assistant("Was that one for me?"),
            learner("no sorry. the tide waits because the moon is slow and the water is heavy"),
            assistant("Slow moon, heavy water — what does the heaviness do?"),
            learner(ROOM[1]),
            assistant("The heavy water, then — what does the weight do to the wait?"),
        ]);
        assert!(
            grade_room_speech(&ok, ROOM).passed,
            "{:?}",
            grade_room_speech(&ok, ROOM)
        );
    }

    // --- 9. no learning style from silence -------------------------------

    fn silent_session() -> SessionTrace {
        trace(&[
            assistant("What's alive in it?"),
            learner("."),
            assistant("Anything there, even one word?"),
            learner(". . ."),
            assistant("Nothing has to come. The door stays open."),
            learner("ok"),
        ])
    }

    #[test]
    fn a_silent_session_must_not_refine_how_i_learn() {
        let bad = "NOTE: Opened on the tide question; nothing came back.\n\
                   PROFILE how_i_learn: Long silences are the norm; small concrete questions work better than open ones.";
        let r = grade_profile_from_silence(&silent_session(), bad);
        assert!(!r.passed);
        assert!(
            r.detail.contains("no substantive learner turn"),
            "{}",
            r.detail
        );
        assert!(r.detail.contains("silence"), "{}", r.detail);

        let ok = "NOTE: Opened on the tide question; the mic sent empty turns and nothing condensed. The question stays open.";
        assert!(grade_profile_from_silence(&silent_session(), ok).passed);
    }

    #[test]
    fn a_silence_claim_fails_even_in_a_full_session() {
        let full = trace(&[
            assistant("What's alive in it?"),
            learner("the moon pulls a sheet and the water slides toward the corner it lifts"),
        ]);
        let bad = "NOTE: Built the sheet picture.\nPROFILE how_i_learn: Goes quiet when pressed, then answers in short replies.";
        assert!(!grade_profile_from_silence(&full, bad).passed);

        let ok = "NOTE: Built the sheet picture.\nPROFILE how_i_learn: Reaches for a physical picture before a definition.";
        assert!(grade_profile_from_silence(&full, ok).passed);
    }

    #[test]
    fn one_check_per_room_turn_passes_and_rechecking_an_answer_fails() {
        // Run-1 shape: two separate room turns, one light check each.
        let ok = trace(&[
            learner(ROOM[0]),
            assistant("Was that for me, or someone in the room? I'm here when you're ready."),
            learner("no sorry. the tide waits because the moon is slow and the water is heavy"),
            assistant("What does \"waits\" mean there?"),
            learner(ROOM[1]),
            assistant("That sounded like it was meant for someone else. Was it for me?"),
        ]);
        let r = grade_room_speech(&ok, ROOM);
        assert!(r.passed, "{}", r.detail);

        // Pressing: the learner answered, and it checks again anyway.
        let bad = trace(&[
            learner(ROOM[0]),
            assistant("Was that one for me?"),
            learner("no sorry. the tide waits because the moon is slow and the water is heavy"),
            assistant("Are you sure that was for me?"),
        ]);
        let r = grade_room_speech(&bad, ROOM);
        assert!(!r.passed);
        assert!(
            r.detail.contains("after the learner answered"),
            "{}",
            r.detail
        );
    }

    // --- 10. talk ratio ----------------------------------------------------

    #[test]
    fn a_paragraph_to_a_short_turn_fails_the_quiet_cap() {
        // Run-1 shape: 17 learner words, replies of 40+.
        let bad = trace(&[
            learner("actually, what about magnetism instead?"),
            assistant("Magnetism it is. Start with a prediction. Picture a bar magnet with a north end and a south end. You snap it clean in half through the middle. What do you have in your two hands now? Commit to an answer and say why."),
        ]);
        let r = grade_talk_ratio(&bad);
        assert!(!r.passed);
        assert!(r.detail.contains("max 25"), "{}", r.detail);

        let ok = trace(&[
            learner("actually, what about magnetism instead?"),
            assistant("Magnetism, then. Snap a bar magnet in half — what's in your hands?"),
        ]);
        assert!(grade_talk_ratio(&ok).passed);
    }

    #[test]
    fn out_talking_a_talking_learner_fails_the_ratio() {
        // Run-1 shape: ratio ~1.4 on a 31-word learner turn.
        let learner_turn = "honestly something else. I want a tide clock app where the phone buzzes when the water at my beach turns, not at a fixed time, just when it turns. save that.";
        let bad = trace(&[
            learner(learner_turn),
            assistant("Kept, in your words: a tide clock where the phone buzzes when the water at your beach turns. Not at a fixed time, just when it turns. One snag. By turns, do you mean both high and low water, or just one of them?"),
        ]);
        let r = grade_talk_ratio(&bad);
        assert!(!r.passed);
        assert!(r.detail.contains("ratio"), "{}", r.detail);

        let ok = trace(&[
            learner(learner_turn),
            assistant("Kept, in your words. By \"turns,\" do you mean high water, low, or both?"),
        ]);
        assert!(grade_talk_ratio(&ok).passed, "{:?}", grade_talk_ratio(&ok));
    }

    #[test]
    fn empty_turns_do_not_count_as_learner_words() {
        assert_eq!(word_count(". . ."), 0);
        assert_eq!(word_count("A kite — slack."), 3);
    }

    // --- 11. landed reply ---------------------------------------------------

    #[test]
    fn a_four_sentence_answer_to_an_empty_turn_fails() {
        // Run-1 shape (dead_air).
        let bad = trace(&[
            assistant("What picture comes to mind?"),
            learner(". . ."),
            assistant("No rush. The question will keep. Whenever a picture comes to you, start there. Even a rough one will do."),
        ]);
        let r = grade_landed_reply(&bad);
        assert!(!r.passed);
        assert!(r.detail.contains("4 sentences"), "{}", r.detail);

        // Run-1 shape (leaver): a long goodbye.
        let long_bye = trace(&[
            learner("not sure. I have to go, actually."),
            assistant("Go. Your picture is kept as you said it: the moon pulls a sheet, and the water goes where the sheet goes. We haven't worked out what the sheet is yet. That's where we'll pick up."),
        ]);
        assert!(!grade_landed_reply(&long_bye).passed);

        let ok = trace(&[
            learner(". . ."),
            assistant("Nothing has to come. The door stays open."),
            learner("I have to go."),
            assistant("What the sheet is stays open. We start there."),
        ]);
        assert!(
            grade_landed_reply(&ok).passed,
            "{:?}",
            grade_landed_reply(&ok)
        );
    }

    #[test]
    fn the_opening_move_is_capped_but_not_charged_to_the_ratio() {
        // Run-2 shape: a 38-word opener fails the per-reply budget...
        let long_open = trace(&[
            assistant("\"What is entropy?\" You left that one open. Let's not chase the word yet. A cup of hot coffee sits on a desk and nobody touches it. What's the one thing that cup never does on its own?"),
            learner("I don't know."),
            assistant("Good. Skip the \"never.\" What had the coffee turned into?"),
        ]);
        let r = grade_talk_ratio(&long_open);
        assert!(!r.passed);
        assert!(r.detail.contains("opening 0"), "{}", r.detail);

        // ...but a within-budget opener is never charged to the ratio.
        let learner_turn = "the tide thing: I think the moon pulls a sheet and the water just goes where the sheet goes, and nobody told it to";
        let ok = trace(&[
            assistant("That tide question, the one you left open: what have you actually seen it do, out there on the water?"),
            learner(learner_turn),
            learner("and the sheet is thin, really thin"),
            assistant("A thin sheet. What's pulling it?"),
        ]);
        let r = grade_talk_ratio(&ok);
        assert!(r.passed, "{}", r.detail);
    }

    #[test]
    fn a_long_learner_turn_earns_a_longer_reply() {
        let long_turn = "ok so here is the whole thing as I see it right now, and bear with me because it is a lot: the moon pulls on the water, but the water is heavy and the beach is shallow, so the pull shows up late, and the lateness changes with the shape of the bay, and that is why my local tide table never matches the moon rising";
        assert!(word_count(long_turn) > 60);
        let t = trace(&[
            learner(long_turn),
            assistant("The pull shows up late, and the lateness changes with the shape of the bay. That's a mechanism, in your words. Say it once as the thing you now know: what decides how late the water is?"),
        ]);
        assert!(grade_talk_ratio(&t).passed, "{:?}", grade_talk_ratio(&t));
    }
}
