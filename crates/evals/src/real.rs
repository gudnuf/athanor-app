//! The gated real-API tier: the SAME persona scripts the hermetic tier uses,
//! but only their learner lines. The Mystagogue side is the real engine
//! (`GooseEngine` on the pinned model), driven through the real `Conductor`,
//! so the system prompt, history, tools and register stripping are what the
//! app ships. The deterministic graders then run over the real transcript.
//!
//! Everything here except the engine constructor is engine-agnostic and
//! compiles without the `real-api` feature, so the report writer, the
//! env parsing and the runner are tested hermetically against a scripted
//! engine. Only `gated_tests` (feature `real-api`, `#[ignore]`) builds a
//! `GooseEngine`, with the key from `ANTHROPIC_API_KEY` and nowhere else.
//!
//! The judge questions (research doc, per rule) are NOT auto-scored. They are
//! written next to the report as a markdown checklist, with each transcript,
//! for the operator's taste check.
//!
//! Opening: a persona whose script starts with a Mystagogue line gets the
//! Conductor's opening turn first (the real model speaks, then the learner
//! lines follow); one that starts with a learner line (the returner's "hey.")
//! speaks first, as in the app. Each persona seeds its own topic.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use athanor_core::engine::{AcpPrompt, AcpUpdate, EngineError, MystagogueEngine, ToolDispatch};
use athanor_core::store::Store;
use athanor_core::Conductor;
use serde::{Deserialize, Serialize};

use crate::field::{grade_profile_from_silence, is_empty_turn, FieldCheck};
use crate::grade::{grade_mask_fidelity, grade_salt_refusal, grade_spiral, SessionTrace, Turn};
use crate::personas::{Persona, Step};
use crate::report::{CheckResult, ScenarioReport, SuiteReport};

/// The prompt-pack version under test (matches `run_suite`).
pub const PACK_VERSION: &str = "v1";
pub const DEFAULT_MAX_TURNS: usize = 6;

// ---- cost guard ----

#[derive(Clone, Debug, PartialEq)]
pub struct RealConfig {
    /// Learner turns per persona (`EVALS_REAL_TURNS`, default 6).
    pub max_turns: usize,
    /// `EVALS_REAL_PERSONAS` comma list; `None` = every persona.
    pub personas: Option<Vec<String>>,
}

impl RealConfig {
    pub fn parse(turns: Option<&str>, personas: Option<&str>) -> Result<Self, String> {
        let max_turns = match turns.map(str::trim).filter(|s| !s.is_empty()) {
            None => DEFAULT_MAX_TURNS,
            Some(s) => match s.parse::<usize>() {
                Ok(n) if n > 0 => n,
                _ => {
                    return Err(format!(
                        "EVALS_REAL_TURNS must be a positive integer, got {s:?}"
                    ))
                }
            },
        };
        let personas = personas
            .map(|s| {
                s.split(',')
                    .map(|p| p.trim().to_string())
                    .filter(|p| !p.is_empty())
                    .collect::<Vec<_>>()
            })
            .filter(|v| !v.is_empty());
        Ok(RealConfig {
            max_turns,
            personas,
        })
    }

    pub fn from_env() -> Result<Self, String> {
        Self::parse(
            std::env::var("EVALS_REAL_TURNS").ok().as_deref(),
            std::env::var("EVALS_REAL_PERSONAS").ok().as_deref(),
        )
    }
}

/// Narrows `all` to the named personas (in the order named). An unknown name
/// is an error, so a typo can't silently run nothing.
pub fn select_personas(
    all: Vec<Persona>,
    names: Option<&[String]>,
) -> Result<Vec<Persona>, String> {
    let Some(names) = names else { return Ok(all) };
    let mut pool: Vec<Option<Persona>> = all.into_iter().map(Some).collect();
    let mut out = Vec::new();
    for name in names {
        let slot = pool
            .iter_mut()
            .find(|p| p.as_ref().is_some_and(|p| p.name == name))
            .ok_or_else(|| format!("unknown persona {name:?} in EVALS_REAL_PERSONAS"))?;
        out.push(slot.take().unwrap());
    }
    Ok(out)
}

/// Whether the persona's script has the Mystagogue speak first.
pub fn opens_with_mystagogue(persona: &Persona, seed_thread_id: &str) -> bool {
    matches!(
        (persona.build)(seed_thread_id).first(),
        Some(Step::Engine(_))
    )
}

/// The persona's own learner lines, in order, capped. The scripted Mystagogue
/// steps are dropped: the real engine answers instead.
pub fn learner_lines(persona: &Persona, seed_thread_id: &str, max: usize) -> Vec<String> {
    (persona.build)(seed_thread_id)
        .into_iter()
        .filter_map(|s| match s {
            Step::Learner(t) => Some(t),
            Step::Engine(_) => None,
        })
        .take(max)
        .collect()
}

// ---- transcript recording ----

/// Folds a live `AcpUpdate` stream (already marker-stripped by the Conductor)
/// into grader `Turn`s: contiguous text becomes one `Assistant` turn, a tool
/// call flushes it and is recorded in stream order.
#[derive(Default)]
pub struct TurnRecorder {
    pub turns: Vec<Turn>,
    buffer: String,
}

impl TurnRecorder {
    pub fn learner(&mut self, text: &str) {
        self.flush();
        self.turns.push(Turn::LearnerText(text.to_string()));
    }

    pub fn push(&mut self, update: AcpUpdate) {
        match update {
            AcpUpdate::TextDelta { text, .. } => self.buffer.push_str(&text),
            AcpUpdate::ToolCall(call) => {
                self.flush();
                self.turns.push(Turn::ToolCall {
                    name: call.name,
                    args: call.args.to_string(),
                });
            }
            AcpUpdate::ToolResult(_) => {}
            AcpUpdate::TurnComplete => self.flush(),
        }
    }

    pub fn flush(&mut self) {
        if !self.buffer.is_empty() {
            self.turns
                .push(Turn::Assistant(std::mem::take(&mut self.buffer)));
        }
    }

    pub fn finish(mut self) -> Vec<Turn> {
        self.flush();
        self.turns
    }
}

// ---- the close pass ----

/// What the close (condensation) pass produced: the raw model text and what
/// `parse_condensation` made of it (the NOTE and PROFILE the app would store).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct CloseRecord {
    pub persona: String,
    pub raw: String,
    pub note: Option<String>,
    pub profile: Vec<(String, String)>,
}

impl CloseRecord {
    pub fn parse(persona: &str, raw: &str) -> Self {
        let parsed = athanor_core::prompt::parse_condensation(raw);
        CloseRecord {
            persona: persona.to_string(),
            raw: raw.to_string(),
            note: parsed.note,
            profile: parsed.profile_updates,
        }
    }
}

/// Forwards to `inner` and keeps the text of the tool-less call: that is the
/// condensation pass (`Conductor::condense` sends no tools; session turns
/// always carry the tool specs). Lets the runner use the app's own close path
/// and still see the raw output it discards.
struct CloseTap<'a> {
    inner: &'a dyn MystagogueEngine,
    close_text: Mutex<Option<String>>,
}

#[async_trait]
impl MystagogueEngine for CloseTap<'_> {
    async fn run_turn(
        &self,
        prompt: AcpPrompt,
        tools: &dyn ToolDispatch,
        sink: &mut (dyn FnMut(AcpUpdate) + Send),
    ) -> Result<(), EngineError> {
        if !prompt.tools.is_empty() {
            return self.inner.run_turn(prompt, tools, sink).await;
        }
        let mut text = String::new();
        let res = self
            .inner
            .run_turn(prompt, tools, &mut |u| {
                if let AcpUpdate::TextDelta { text: t, .. } = &u {
                    text.push_str(t);
                }
                sink(u);
            })
            .await;
        *self.close_text.lock().unwrap() = Some(text);
        res
    }
}

// ---- running one persona ----

pub struct RealRun {
    pub report: ScenarioReport,
    pub turns: Vec<Turn>,
    /// `None` when the session never ran a turn (nothing to condense).
    pub close: Option<CloseRecord>,
}

/// Runs one persona's learner lines through `engine` via a real `Conductor`
/// over a fresh in-memory store, then grades the transcript.
pub async fn run_persona_real(
    engine: &dyn MystagogueEngine,
    persona: &Persona,
    max_turns: usize,
) -> RealRun {
    let store = Arc::new(Store::open_in_memory(persona.name).expect("open in-memory store"));
    let (question, domain_name) = persona.topic.unwrap_or(("what is entropy?", "entropy"));
    let domain = store.upsert_domain(domain_name).expect("seed domain");
    let thread = store
        .open_thread(question, Some(&domain.id), None)
        .expect("seed thread");
    seed_prior_state(&store, persona, &thread.id);

    let lines = learner_lines(persona, &thread.id, max_turns);
    let mut conductor = Conductor::begin(
        Arc::clone(&store),
        persona.mask,
        persona.mode,
        Some(&thread.id),
    )
    .expect("begin session");

    let mut rec = TurnRecorder::default();
    let mut engine_error: Option<String> = None;
    // A persona scripted to start with the Mystagogue speaking gets the
    // Conductor's opening turn (the arrival marker, no learner text), so the
    // real model opens the session before the first learner line. A persona
    // that starts with the learner ("hey.") just speaks first, as in the app.
    if opens_with_mystagogue(persona, &thread.id) {
        let mut sink = |u: AcpUpdate| rec.push(u);
        if let Err(e) = conductor.open_turn(engine, &mut sink).await {
            engine_error = Some(e.to_string());
        }
    }
    for line in &lines {
        if engine_error.is_some() {
            break;
        }
        rec.learner(line);
        let mut sink = |u: AcpUpdate| rec.push(u);
        if let Err(e) = conductor.run_turn(engine, Some(line), &mut sink).await {
            engine_error = Some(e.to_string());
            break;
        }
    }
    // The app's own close path: condense (best-effort, tapped for its raw
    // text), then land the session.
    let tap = CloseTap {
        inner: engine,
        close_text: Mutex::new(None),
    };
    // A learner who never said anything with content leaves nothing to distill
    // (the core's honest close); the runner doesn't ask for one either.
    let silent = lines.iter().all(|l| is_empty_turn(l));
    let condense_err = if engine_error.is_none() && !silent {
        conductor.condense(&tap).await.err().map(|e| e.to_string())
    } else {
        None
    };
    let close_record = tap
        .close_text
        .lock()
        .unwrap()
        .take()
        .map(|raw| CloseRecord::parse(persona.name, &raw));
    let close = conductor.close(1);
    let turns = rec.finish();

    let engine_failed = engine_error.is_some();
    let trace = SessionTrace::with_mask(persona.mask, turns.clone());
    let mut checks = vec![
        grade_spiral(&trace),
        grade_salt_refusal(&trace),
        grade_mask_fidelity(&trace),
        CheckResult {
            name: "engine_ok".into(),
            passed: engine_error.is_none(),
            detail: engine_error.unwrap_or_else(|| format!("{} turn(s) completed", lines.len())),
        },
        CheckResult {
            name: "session_lands".into(),
            passed: close.is_ok(),
            detail: match close {
                Ok(_) => "session closed cleanly".into(),
                Err(e) => format!("session failed to close: {e}"),
            },
        },
    ];
    checks.extend(
        persona
            .field
            .iter()
            .map(|c| crate::field::run_check(*c, &trace)),
    );
    checks.push(CheckResult {
        name: "close_pass".into(),
        passed: close_record.is_some() || engine_failed || silent,
        detail: match (&close_record, &condense_err) {
            (Some(_), None) => "close pass produced output".into(),
            (Some(_), Some(e)) => format!("close pass errored after output: {e}"),
            (None, Some(e)) => format!("close pass failed: {e}"),
            (None, None) if silent => {
                "not applicable: the learner never spoke, so no close pass".into()
            }
            (None, None) if engine_failed => "skipped: a session turn failed".into(),
            (None, None) => "close pass produced no output".into(),
        },
    });
    if let Some(rec) = &close_record {
        checks.push(grade_profile_from_silence(&trace, &rec.raw));
    }

    RealRun {
        report: ScenarioReport::new(persona.name, checks),
        turns,
        close: close_record,
    }
}

/// Prior-session state a persona's script assumes but cannot carry in its
/// learner lines: the returner's stalled-question note.
fn seed_prior_state(store: &Store, persona: &Persona, thread_id: &str) {
    if persona.name != "returner" {
        return;
    }
    let prior = store
        .create_session(Some(thread_id), persona.mask, persona.mode)
        .expect("prior session");
    store
        .add_session_note(
            &prior.id,
            Some(thread_id),
            "Worked the sheet picture. Asked what holds the other end twice; no answer either time. Stalled: come at it sideways or let it rest.",
        )
        .expect("prior note");
}

// ---- output ----

/// The persisted report: timestamp-free, so two runs diff cleanly.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RealReport {
    pub model: String,
    pub max_turns: usize,
    pub suite: SuiteReport,
    /// Raw close-pass output and its parsed NOTE/PROFILE, per persona.
    pub closes: Vec<CloseRecord>,
}

/// `<dir>/<pack_version>-<model>.json`, with the model made path-safe.
pub fn report_path(dir: &Path, pack_version: &str, model: &str) -> PathBuf {
    let safe: String = model
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '.' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    dir.join(format!("{pack_version}-{safe}.json"))
}

pub fn report_dir() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/reports/real-api"))
}

/// Writes the JSON report (and returns its path). Creates `dir` if needed.
pub fn write_report(dir: &Path, report: &RealReport) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = report_path(dir, &report.suite.pack_version, &report.model);
    let json = serde_json::to_string_pretty(report).expect("serialize RealReport");
    std::fs::write(&path, format!("{json}\n"))?;
    Ok(path)
}

/// Per-rule judge question, from `docs/research/2026-11-24-prompt-pack-v1-from-field-use.md`.
fn judge_question(check: &FieldCheck) -> (&'static str, &'static str) {
    match check {
        FieldCheck::TalkRatio => ("10. Say less than they did", "Did the Mystagogue talk more than the learner, or answer a short turn with a paragraph?"),
        FieldCheck::ElapsedTime(_) => ("1. You have no clock", "Did any reply state or imply a duration or a time of day (\"it's been a while\", \"late for you\") that the context did not give?"),
        FieldCheck::EmptyTurns => ("2. An empty turn isn't a turn", "After the empty turns, did the Mystagogue leave the door open and stop? Or did it keep shrinking the question, close and then reopen, or treat each empty turn as a visit?"),
        FieldCheck::OneQuestion | FieldCheck::Landing => ("3. One question per reply; the last turn lands", "Did the last reply name what stayed open in the learner's words and stop, or did it ask something on the way out? Did any reply hand the learner a choice of questions?"),
        FieldCheck::Forks => ("4. Ration the either/or", "Did the learner's best images come from their own words or from options the Mystagogue offered? Does any fixed salt rest on an offered option the learner only picked?"),
        FieldCheck::NoReask(_) => ("5. A stalled question rests", "Given a prior note marking a question stalled, did the opening come at it sideways or open something else, rather than ask it again in new words?"),
        FieldCheck::OfferAfter(_) | FieldCheck::WeaveOnRhyme => ("6. Offer on an own-words mechanism; weave a spoken rhyme", "When the learner explained how something works in their own words, did the Mystagogue notice and offer, or steer back to its own thread? Was any recorded weave one the learner actually took up?"),
        FieldCheck::BankVerbatim => ("7. \"Save that\" keeps their words", "Does the stored text read as the learner's sentence, or as the Mystagogue's summary of it?"),
        FieldCheck::RoomSpeech(_) => ("8. Not every voice is for you", "This one covers the close NOTE and PROFILE too, which the live trace doesn't carry: did anything the Mystagogue said, noted or profiled rest on a turn that was plausibly for someone else?"),
    }
}

const RULE_9: (&str, &str) = ("9. No learning style from silence", "Does each `how_i_learn` line rest on something the learner said or did with words, seen more than once, or on one session's quiet?");

fn render_turn(t: &Turn) -> String {
    match t {
        Turn::LearnerText(s) => format!("- **learner:** {s}"),
        Turn::Assistant(s) => format!("- **mystagogue:** {s}"),
        Turn::ToolCall { name, args } => format!("- _tool_ `{name}` {args}"),
    }
}

/// The operator's taste-check checklist: per persona, its judge questions
/// (de-duplicated, unscored) and the full transcript they apply to.
pub fn judge_checklist(model: &str, runs: &[(&Persona, &RealRun)]) -> String {
    let mut out = format!(
        "# Real-API taste check — pack {PACK_VERSION}, {model}\n\nNot auto-scored. Read each transcript and answer each question.\n"
    );
    for (persona, run) in runs {
        out.push_str(&format!("\n## {}\n\n", persona.name));
        let mut seen: Vec<&str> = Vec::new();
        for c in persona.field {
            let (rule, q) = judge_question(c);
            if seen.contains(&rule) {
                continue;
            }
            seen.push(rule);
            out.push_str(&format!("- [ ] **{rule}** — {q}\n"));
        }
        if seen.is_empty() {
            out.push_str("- [ ] No field rule targets this persona; does the session read as the Mystagogue's voice under this mask?\n");
        }
        out.push_str(&format!("- [ ] **{}** — {}\n", RULE_9.0, RULE_9.1));
        out.push_str("\n<details><summary>transcript</summary>\n\n");
        for t in &run.turns {
            out.push_str(&render_turn(t));
            out.push('\n');
        }
        out.push_str("\n</details>\n");
        if let Some(c) = &run.close {
            out.push_str("\n**Close pass (raw):**\n\n```\n");
            out.push_str(&c.raw);
            out.push_str("\n```\n\n");
            out.push_str(&format!(
                "- parsed NOTE: {}\n",
                c.note.as_deref().unwrap_or("(none)")
            ));
            if c.profile.is_empty() {
                out.push_str("- parsed PROFILE: (none)\n");
            }
            for (sec, add) in &c.profile {
                out.push_str(&format!("- parsed PROFILE `{sec}`: {add}\n"));
            }
        }
    }
    out
}

/// One human line per persona.
pub fn result_line(r: &ScenarioReport) -> String {
    let failing: Vec<&str> = r
        .checks
        .iter()
        .filter(|c| !c.passed)
        .map(|c| c.name.as_str())
        .collect();
    if failing.is_empty() {
        format!("{:<16} PASS ({} checks)", r.id, r.checks.len())
    } else {
        format!("{:<16} FAIL: {}", r.id, failing.join(", "))
    }
}

/// Runs the selected personas, writes the JSON report and the checklist into
/// `dir`, prints a line per persona, and returns the report.
pub async fn run_real_suite(
    engine: &dyn MystagogueEngine,
    model: &str,
    cfg: &RealConfig,
    personas: Vec<Persona>,
    dir: &Path,
) -> std::io::Result<RealReport> {
    let mut runs = Vec::new();
    for p in &personas {
        let run = run_persona_real(engine, p, cfg.max_turns).await;
        println!("{}", result_line(&run.report));
        runs.push(run);
    }
    let report = RealReport {
        model: model.to_string(),
        max_turns: cfg.max_turns,
        suite: SuiteReport::assemble(
            PACK_VERSION,
            runs.iter().map(|r| r.report.clone()).collect(),
        ),
        closes: runs.iter().filter_map(|r| r.close.clone()).collect(),
    };
    let json_path = write_report(dir, &report)?;
    let pairs: Vec<(&Persona, &RealRun)> = personas.iter().zip(runs.iter()).collect();
    let md_path = json_path.with_extension("judge.md");
    std::fs::write(&md_path, judge_checklist(model, &pairs))?;
    println!("wrote {}\nwrote {}", json_path.display(), md_path.display());
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::personas::{all_personas, dead_air, explainer, returner};
    use async_trait::async_trait;
    use athanor_core::engine::{AcpPrompt, EngineError, ToolDispatch};
    use std::collections::VecDeque;
    use std::sync::Mutex;

    /// One scripted reply per `run_turn`; records the prompts it was given.
    struct PerTurn {
        replies: Mutex<VecDeque<Vec<AcpUpdate>>>,
        systems: Mutex<Vec<String>>,
    }

    impl PerTurn {
        fn new(replies: Vec<Vec<AcpUpdate>>) -> Self {
            Self {
                replies: Mutex::new(replies.into()),
                systems: Mutex::default(),
            }
        }
    }

    #[async_trait]
    impl MystagogueEngine for PerTurn {
        async fn run_turn(
            &self,
            prompt: AcpPrompt,
            _tools: &dyn ToolDispatch,
            sink: &mut (dyn FnMut(AcpUpdate) + Send),
        ) -> Result<(), EngineError> {
            self.systems.lock().unwrap().push(prompt.system);
            let next = self.replies.lock().unwrap().pop_front();
            match next {
                Some(r) => {
                    for u in r {
                        sink(u);
                    }
                    Ok(())
                }
                None => Err(EngineError::Other("script exhausted".into())),
            }
        }
    }

    /// Engine calls a session makes before the close pass: the opener (when
    /// the script has the Mystagogue speak first) plus one per learner line.
    fn session_calls(p: &Persona) -> usize {
        learner_lines(p, "t", 6).len() + usize::from(opens_with_mystagogue(p, "t"))
    }

    fn say(t: &str) -> Vec<AcpUpdate> {
        vec![AcpUpdate::text_delta(t), AcpUpdate::TurnComplete]
    }

    #[test]
    fn config_defaults_and_overrides() {
        let d = RealConfig::parse(None, None).unwrap();
        assert_eq!((d.max_turns, d.personas), (6, None));
        let c = RealConfig::parse(Some("3"), Some("leaver, returner,")).unwrap();
        assert_eq!(c.max_turns, 3);
        assert_eq!(
            c.personas,
            Some(vec!["leaver".to_string(), "returner".to_string()])
        );
        assert!(RealConfig::parse(Some("0"), None).is_err());
        assert!(RealConfig::parse(Some("x"), None).is_err());
        assert_eq!(
            RealConfig::parse(Some(""), Some("")).unwrap().personas,
            None
        );
    }

    #[test]
    fn select_personas_filters_in_order_and_rejects_typos() {
        let got =
            select_personas(all_personas(), Some(&["returner".into(), "leaver".into()])).unwrap();
        assert_eq!(
            got.iter().map(|p| p.name).collect::<Vec<_>>(),
            ["returner", "leaver"]
        );
        assert_eq!(select_personas(all_personas(), None).unwrap().len(), 11);
        assert!(select_personas(all_personas(), Some(&["nobody".into()])).is_err());
    }

    #[test]
    fn learner_lines_drop_the_scripted_mystagogue_and_cap() {
        let lines = learner_lines(&returner(), "t", 6);
        assert_eq!(
            lines,
            ["hey.", "a kite, kind of. the string goes slack and taut"]
        );
        assert!(learner_lines(&crate::personas::explainer(), "t", 1).len() == 1);
    }

    #[test]
    fn recorder_coalesces_text_and_orders_tools() {
        let mut r = TurnRecorder::default();
        r.learner("hi");
        r.push(AcpUpdate::text_delta("a "));
        r.push(AcpUpdate::text_delta("b"));
        r.push(AcpUpdate::ToolCall(athanor_core::engine::AcpToolCall {
            id: "1".into(),
            name: "open_thread".into(),
            args: serde_json::json!({"question": "q"}),
        }));
        r.push(AcpUpdate::text_delta("c"));
        r.push(AcpUpdate::TurnComplete);
        let t = r.finish();
        assert!(matches!(&t[0], Turn::LearnerText(s) if s == "hi"));
        assert!(matches!(&t[1], Turn::Assistant(s) if s == "a b"));
        assert!(matches!(&t[2], Turn::ToolCall { name, .. } if name == "open_thread"));
        assert!(matches!(&t[3], Turn::Assistant(s) if s == "c"));
    }

    #[tokio::test]
    async fn a_persona_runs_through_the_conductor_and_is_graded() {
        let p = explainer();
        let n = session_calls(&p);
        let mut replies: Vec<_> = (0..n).map(|_| say("I'm here.")).collect();
        replies.push(say("NOTE: Sourdough rise, learner's own words.\nPROFILE how_i_learn: goes quiet, so likely a slow processor"));
        let engine = PerTurn::new(replies);
        let run = run_persona_real(&engine, &p, 6).await;
        assert!(run.report.checks.iter().any(|c| c.name == "spiral"));
        assert!(run.report.checks.iter().any(|c| c.name == "one_question"));
        assert!(
            run.report
                .checks
                .iter()
                .find(|c| c.name == "engine_ok")
                .unwrap()
                .passed
        );
        assert!(
            run.report
                .checks
                .iter()
                .find(|c| c.name == "session_lands")
                .unwrap()
                .passed
        );
        assert_eq!(
            engine.systems.lock().unwrap().len(),
            n + 1,
            "turns + close pass"
        );
        let close = run.close.expect("close pass recorded");
        assert!(close.raw.contains("slow processor"));
        assert_eq!(
            close.note.as_deref(),
            Some("Sourdough rise, learner's own words.")
        );
        assert_eq!(close.profile.len(), 1);
        let g = run
            .report
            .checks
            .iter()
            .find(|c| c.name == "profile_from_silence")
            .unwrap();
        assert!(
            !g.passed,
            "a silence-based how_i_learn line must fail rule 9: {g:?}"
        );
    }

    #[tokio::test]
    async fn a_clean_close_passes_rule_9() {
        let p = explainer();
        let n = session_calls(&p);
        let mut replies: Vec<_> = (0..n).map(|_| say("Here.")).collect();
        replies.push(say("NOTE: Sourdough rise, learner's own words."));
        let engine = PerTurn::new(replies);
        let run = run_persona_real(&engine, &p, 6).await;
        let g = run
            .report
            .checks
            .iter()
            .find(|c| c.name == "profile_from_silence")
            .unwrap();
        assert!(g.passed, "{g:?}");
    }

    #[tokio::test]
    async fn a_silent_session_gets_no_close_pass_and_that_is_not_a_failure() {
        let p = dead_air();
        let n = session_calls(&p);
        // Only session replies are scripted: a close-pass call would exhaust it.
        let engine = PerTurn::new(
            (0..n)
                .map(|_| say("Nothing has to come — what's there?"))
                .collect(),
        );
        let run = run_persona_real(&engine, &p, 6).await;
        assert_eq!(
            engine.systems.lock().unwrap().len(),
            n,
            "no close pass call"
        );
        assert!(run.close.is_none());
        let c = run
            .report
            .checks
            .iter()
            .find(|c| c.name == "close_pass")
            .unwrap();
        assert!(c.passed && c.detail.starts_with("not applicable"), "{c:?}");
        assert!(
            !run.report
                .checks
                .iter()
                .any(|c| c.name == "profile_from_silence"),
            "rule 9 has nothing to grade without a close pass"
        );
        // (Other graders judge the scripted reply text; only the close pass is
        // under test here.)
    }

    #[tokio::test]
    async fn the_mystagogue_opens_first_unless_the_script_starts_with_the_learner() {
        // dead_air's script opens with the Mystagogue: opener, then 3 lines.
        let p = dead_air();
        assert_eq!(session_calls(&p), 4);
        let mut replies = vec![say("What's alive in the tide question?")];
        replies.extend((0..3).map(|_| say("Here — anything?")));
        replies.push(say("NOTE: x"));
        let engine = PerTurn::new(replies);
        let run = run_persona_real(&engine, &p, 6).await;
        assert!(matches!(&run.turns[0], Turn::Assistant(s) if s.contains("tide question")));
        assert!(matches!(&run.turns[1], Turn::LearnerText(s) if s == "."));
        // the returner starts with the learner's arrival line: no opener.
        let r = returner();
        assert!(!opens_with_mystagogue(&r, "t"));
        let engine = PerTurn::new(vec![say("Hm.")]);
        let run = run_persona_real(&engine, &r, 6).await;
        assert!(matches!(&run.turns[0], Turn::LearnerText(s) if s == "hey."));
    }

    #[tokio::test]
    async fn each_persona_seeds_its_own_topic() {
        let p = returner();
        let engine = PerTurn::new(vec![say("Hm.")]);
        let _ = run_persona_real(&engine, &p, 6).await;
        let sys = engine.systems.lock().unwrap()[0].clone();
        assert!(
            sys.contains("why does the tide wait for the moon?"),
            "{sys}"
        );
        assert!(!sys.contains("what is entropy?"));
        let q = crate::personas::eager_parroter();
        let engine = PerTurn::new(vec![say("Hm.")]);
        let _ = run_persona_real(&engine, &q, 6).await;
        assert!(engine.systems.lock().unwrap()[0].contains("what is entropy?"));
    }

    #[tokio::test]
    async fn the_returner_prompt_carries_the_stalled_note() {
        let p = returner();
        let engine = PerTurn::new(vec![say("Hm.")]);
        let _ = run_persona_real(&engine, &p, 6).await;
        let sys = engine.systems.lock().unwrap()[0].clone();
        assert!(
            sys.contains("Stalled: come at it sideways"),
            "seeded note reaches the prompt"
        );
    }

    #[tokio::test]
    async fn an_engine_error_is_a_failed_check_not_a_panic() {
        let p = dead_air();
        let engine = PerTurn::new(vec![]);
        let run = run_persona_real(&engine, &p, 6).await;
        let c = run
            .report
            .checks
            .iter()
            .find(|c| c.name == "engine_ok")
            .unwrap();
        assert!(!c.passed && c.detail.contains("script exhausted"), "{c:?}");
        assert!(!run.report.passed);
    }

    #[test]
    fn report_path_names_pack_and_model_and_writer_is_timestamp_free() {
        let dir = std::env::temp_dir().join(format!("evals-real-{}", std::process::id()));
        assert_eq!(
            report_path(&dir, "v1", "claude-opus-5-5")
                .file_name()
                .unwrap(),
            "v1-claude-opus-5-5.json"
        );
        let rep = RealReport {
            model: "claude-opus-5-5".into(),
            max_turns: 6,
            suite: SuiteReport::assemble("v1", vec![ScenarioReport::new("x", vec![])]),
            closes: vec![CloseRecord::parse("x", "NOTE: n")],
        };
        let path = write_report(&dir, &rep).unwrap();
        let a = std::fs::read_to_string(&path).unwrap();
        write_report(&dir, &rep).unwrap();
        assert_eq!(a, std::fs::read_to_string(&path).unwrap());
        let back: RealReport = serde_json::from_str(&a).unwrap();
        assert_eq!(back, rep);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn checklist_carries_each_rules_question_once_and_the_transcript() {
        let p = returner();
        let run = RealRun {
            report: ScenarioReport::new("returner", vec![]),
            turns: vec![
                Turn::LearnerText("a kite".into()),
                Turn::Assistant("Taut?".into()),
            ],
            close: Some(CloseRecord::parse(
                "returner",
                "NOTE: Kite, slack and taut.",
            )),
        };
        let md = judge_checklist("m", &[(&p, &run)]);
        assert!(md.contains("## returner"));
        assert!(md.contains("5. A stalled question rests"));
        assert!(md.contains("1. You have no clock"));
        assert_eq!(md.matches("3. One question per reply").count(), 1);
        assert!(md.contains("9. No learning style from silence"));
        assert!(md.contains("parsed NOTE: Kite, slack and taut."));
        assert!(md.contains("**learner:** a kite") && md.contains("**mystagogue:** Taut?"));
    }

    #[tokio::test]
    async fn the_suite_writes_json_and_checklist_side_by_side() {
        let dir = std::env::temp_dir().join(format!("evals-real-suite-{}", std::process::id()));
        let p = dead_air();
        let n = session_calls(&p);
        let mut replies: Vec<_> = (0..n).map(|_| say("Here.")).collect();
        replies.push(say("NOTE: ok"));
        let engine = PerTurn::new(replies);
        let cfg = RealConfig::parse(None, None).unwrap();
        run_real_suite(&engine, "claude-opus-5-5", &cfg, vec![p], &dir)
            .await
            .unwrap();
        assert!(dir.join("v1-claude-opus-5-5.json").exists());
        assert!(dir.join("v1-claude-opus-5-5.judge.md").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Feature-gated live tier. `cargo test -p evals --features real-api -- --ignored`
#[cfg(all(test, feature = "real-api"))]
mod gated_tests {
    use super::*;
    use athanor_core::engine::{GooseEngine, MYSTAGOGUE_MODEL};

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "gated real-API tier: requires ANTHROPIC_API_KEY, on-demand only, never in CI"]
    async fn real_api_tier_runs_personas_against_the_pinned_model() {
        let key = std::env::var("ANTHROPIC_API_KEY")
            .expect("set ANTHROPIC_API_KEY (env only, never a file) to run the real-API tier");
        let cfg = RealConfig::from_env().expect("EVALS_REAL_* env");
        let personas = select_personas(crate::personas::all_personas(), cfg.personas.as_deref())
            .expect("EVALS_REAL_PERSONAS");
        let engine = GooseEngine::new(key, Some(MYSTAGOGUE_MODEL.to_string()), None);
        let report = run_real_suite(&engine, MYSTAGOGUE_MODEL, &cfg, personas, &report_dir())
            .await
            .expect("write real-api report");
        // Judgement is the operator's: failing graders are reported, not asserted,
        // so one regression doesn't hide the other transcripts.
        println!(
            "TOTAL: {}/{} personas passed the deterministic graders",
            report.suite.aggregate.passed, report.suite.aggregate.scenarios
        );
    }
}
