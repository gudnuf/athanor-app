//! Hermetic runner: replays each scripted persona (`personas.rs`) through the
//! real `MockEngine` and a real in-memory `Store`, grades the resulting
//! `SessionTrace` with every Task-13 grader plus two small runner-level
//! checks (mask selection, session close), and assembles a `SuiteReport`.
//!
//! ## Dispatch: the real Mystagogue
//!
//! Tool calls are dispatched through the real [`Mystagogue`] extension. After
//! the engine-seam unification (`Store` is now `Send + Sync` — its
//! `rusqlite::Connection` sits behind a reentrant mutex), `Mystagogue`
//! implements `engine::ToolDispatch` directly, so the persona runner drives the
//! exact same six-tool code path the app uses. (An earlier `StoreDispatch`
//! stand-in existed only because `Mystagogue` couldn't satisfy the Send+Sync
//! bound; it's gone now.)

use std::sync::Arc;

use athanor_core::engine::{AcpPrompt, AcpUpdate, MockEngine, MystagogueEngine};
use athanor_core::register::RegisterParser;
use athanor_core::store::Store;
use athanor_core::{close_session, Mystagogue};

use crate::grade::{grade_mask_fidelity, grade_salt_refusal, grade_spiral, SessionTrace, Turn};
use crate::personas::{Persona, Step};
use crate::report::{CheckResult, ScenarioReport, SuiteReport};

// ---- runner-level checks (beyond the three Task-13 graders) ----

/// The session was opened under the persona's intended mask. Trivial by
/// construction here (the harness passes the mask straight through) — but it
/// exercises the same plumbing a real mask-selection decision would flow
/// through, and documents the intent per persona.
fn check_mask_selected(expected: &str, actual: &str) -> CheckResult {
    let passed = expected == actual;
    CheckResult {
        name: "mask_selected".into(),
        passed,
        detail: if passed {
            format!("session opened under the intended mask ({expected})")
        } else {
            format!("expected mask {expected}, session recorded {actual}")
        },
    }
}

/// The session still lands and closes cleanly — the pacing/patience promise
/// (most pointed for the silent one) is "the plane lands," not "the model
/// fills silence with a lecture."
fn check_session_lands(store: &Store, session_id: &str) -> CheckResult {
    let result = close_session(store, session_id, 1, &[]);
    CheckResult {
        name: "session_lands".into(),
        passed: result.is_ok(),
        detail: match result {
            Ok(()) => "session closed cleanly".into(),
            Err(e) => format!("session failed to close: {e}"),
        },
    }
}

/// Replays one `MockEngine` turn into a coalesced `Vec<Turn>`: contiguous text
/// deltas become one `Assistant` turn, a `ToolCall` flushes the run and records
/// the call (preserving stream order), and reply-register markers are STRIPPED
/// through the same `RegisterParser` the Conductor runs.
///
/// The eval runner drives the engine directly, bypassing the Conductor — so it
/// must strip the markers itself, or a live-engine transcript could carry a raw
/// `<!--reading-->` into a grader or a fixture diff.
async fn replay_engine_step(script: Vec<AcpUpdate>, mystagogue: &Mystagogue) -> Vec<Turn> {
    let engine = MockEngine::new(script);
    // `MockEngine::run_turn` ignores the prompt (the script drives everything);
    // we still hand it the real tool specs.
    let prompt = AcpPrompt {
        system: String::new(),
        turns: Vec::new(),
        tools: Mystagogue::tool_specs(),
    };

    let mut local_turns: Vec<Turn> = Vec::new();
    let mut buffer = String::new();
    let mut register = RegisterParser::default();
    engine
        .run_turn(prompt, mystagogue, &mut |update| match update {
            AcpUpdate::TextDelta { text, .. } => {
                register.push(&text, &mut |chunk, _| buffer.push_str(&chunk));
            }
            AcpUpdate::ToolCall(call) => {
                // Land any held-back marker-prefix as text before the tool
                // interrupts the assistant run.
                register.flush(&mut |chunk, _| buffer.push_str(&chunk));
                if !buffer.is_empty() {
                    local_turns.push(Turn::Assistant(std::mem::take(&mut buffer)));
                }
                local_turns.push(Turn::ToolCall {
                    name: call.name.clone(),
                    args: call.args.to_string(),
                });
            }
            // The dispatched result isn't part of the eval transcript.
            AcpUpdate::ToolResult(_) => {}
            AcpUpdate::TurnComplete => {
                register.flush(&mut |chunk, _| buffer.push_str(&chunk));
            }
        })
        .await
        .expect("mock engine turn");
    // Safety net for a turn that never reached TurnComplete.
    register.flush(&mut |chunk, _| buffer.push_str(&chunk));
    if !buffer.is_empty() {
        local_turns.push(Turn::Assistant(buffer));
    }
    local_turns
}

/// Replays one persona end-to-end: opens a fresh in-memory store and seed
/// thread, opens the session under the persona's mask/mode, drives every
/// `Step` through `MockEngine` (recording the full transcript — assistant
/// text, tool calls, and learner turns — into a `SessionTrace`), grades it,
/// and closes the session. Returns the graded `ScenarioReport`.
pub async fn run_persona(persona: &Persona) -> ScenarioReport {
    let store = Store::open_in_memory(persona.name).expect("open in-memory store");
    let domain = store.upsert_domain("entropy").expect("seed domain");
    let thread = store
        .open_thread("what is entropy?", Some(&domain.id), None)
        .expect("seed thread");
    let session = store
        .create_session(Some(&thread.id), persona.mask, persona.mode)
        .expect("create session");

    let store = Arc::new(store);
    let mystagogue = Mystagogue::new(Arc::clone(&store));

    let steps = (persona.build)(&thread.id);
    let mut turns: Vec<Turn> = Vec::new();

    for step in steps {
        match step {
            Step::Learner(text) => turns.push(Turn::LearnerText(text)),
            Step::Engine(script) => {
                turns.extend(replay_engine_step(script, &mystagogue).await);
            }
        }
    }

    let trace = SessionTrace::with_mask(persona.mask, turns);

    let checks = vec![
        grade_spiral(&trace),
        grade_salt_refusal(&trace),
        grade_mask_fidelity(&trace),
        check_mask_selected(persona.mask, &session.mask),
        check_session_lands(&store, &session.id),
    ];

    ScenarioReport::new(persona.name, checks)
}

/// Runs every persona and assembles the aggregate `SuiteReport`.
pub async fn run_suite(personas: &[Persona]) -> SuiteReport {
    let mut scenarios = Vec::with_capacity(personas.len());
    for persona in personas {
        scenarios.push(run_persona(persona).await);
    }
    SuiteReport::assemble("v0", scenarios)
}

/// The gated real-API LLM-judge tier (Task 15 Step 4 scaffold). Reads
/// `ANTHROPIC_API_KEY`; when a real engine is wired (Phase 4's
/// `GooseEngine`/`goosed_client`), this tier replays the four personas
/// through it (not `MockEngine`), asks `prompts/judge.md` to grade each
/// session, and writes the report to `docs/research/` — never to CI, and
/// never automatically: the review's reconciliation #7 requires the judge
/// see the FULL transcript (assistant + tool calls + `LearnerText`), the same
/// `SessionTrace` shape the deterministic graders already use, not just the
/// Mystagogue tool-call side — mask-fidelity and condensation-honesty
/// grading are meaningless without the learner's actual words.
///
/// Today this only proves the gate: no key means no call, full stop. The
/// real engine wiring is follow-up work once Phase 4's engine lands.
pub fn gated_llm_judge_enabled() -> bool {
    std::env::var("ANTHROPIC_API_KEY").is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::personas::{all_personas, eager_parroter, silent_one, stuck_one, tangent_chaser};
    use athanor_core::engine::AcpToolCall;
    use serde_json::json;

    fn check<'a>(report: &'a ScenarioReport, name: &str) -> &'a CheckResult {
        report
            .checks
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("no '{name}' check in {report:?}"))
    }

    /// The rider on lane 13: the eval runner bypasses the Conductor, so it must
    /// strip reply-register markers itself. A live-engine transcript must never
    /// carry a raw `<!--reading-->` into a grader or fixture — even split across
    /// deltas, and even when a tool call interrupts the reply.
    #[tokio::test]
    async fn reading_markers_are_stripped_from_eval_transcripts() {
        let store = Arc::new(Store::open_in_memory("strip").unwrap());
        store.open_thread("what is entropy?", None, None).unwrap();
        let mystagogue = Mystagogue::new(Arc::clone(&store));

        // A reply with a reading passage whose open marker is split across two
        // deltas, and a tool call mid-reply.
        let script = vec![
            AcpUpdate::text_delta("A quick nudge. <!--rea"),
            AcpUpdate::text_delta("ding-->A measured lesson.<!--/reading--> "),
            AcpUpdate::ToolCall(AcpToolCall {
                id: "1".into(),
                name: "open_thread".into(),
                args: json!({ "question": "and then?" }),
            }),
            AcpUpdate::text_delta("Back to it."),
            AcpUpdate::TurnComplete,
        ];
        let turns = replay_engine_step(script, &mystagogue).await;

        let assistant_text: String = turns
            .iter()
            .filter_map(|t| match t {
                Turn::Assistant(s) => Some(s.as_str()),
                _ => None,
            })
            .collect();
        assert!(
            !assistant_text.contains("<!--"),
            "no register marker leaks into the eval transcript: {assistant_text:?}"
        );
        assert!(
            assistant_text.contains("A measured lesson.") && assistant_text.contains("Back to it."),
            "the stripped prose is preserved: {assistant_text:?}"
        );
        assert!(
            turns
                .iter()
                .any(|t| matches!(t, Turn::ToolCall { name, .. } if name == "open_thread")),
            "the tool call is still recorded in stream order"
        );
    }

    #[tokio::test]
    async fn eager_parroter_fails_salt_refusal() {
        let report = run_persona(&eager_parroter()).await;
        assert!(
            !check(&report, "salt_refusal").passed,
            "the parroter's fix_salt echoed the assistant's own words verbatim"
        );
        // Independent of the refusal failure: tool order is still correct.
        assert!(check(&report, "spiral").passed);
        assert!(!report.passed, "a failing check must fail the scenario");
    }

    #[tokio::test]
    async fn stuck_one_enters_solve_and_lands_clean() {
        let report = run_persona(&stuck_one()).await;
        assert!(
            check(&report, "mask_selected").passed,
            "stuck one's session must open under Solve"
        );
        assert!(
            check(&report, "salt_refusal").passed,
            "no false salt: nothing was fixed just to look productive"
        );
        assert!(check(&report, "spiral").passed);
        assert!(check(&report, "session_lands").passed);
        assert!(report.passed);
    }

    #[tokio::test]
    async fn tangent_chaser_opens_many_threads_with_no_salt_and_still_lands() {
        let report = run_persona(&tangent_chaser()).await;
        assert!(check(&report, "spiral").passed, "no fix_salt at all fired");
        assert!(check(&report, "session_lands").passed);
        assert!(report.passed);
    }

    #[tokio::test]
    async fn silent_one_passes_mask_fidelity_and_still_lands() {
        let report = run_persona(&silent_one()).await;
        assert!(
            check(&report, "mask_fidelity").passed,
            "every philosophus turn must stay a question, even a terse one"
        );
        assert!(check(&report, "session_lands").passed);
        assert!(report.passed);
    }

    #[tokio::test]
    async fn run_suite_aggregates_all_four_personas() {
        let personas = all_personas();
        let suite = run_suite(&personas).await;
        assert_eq!(suite.aggregate.scenarios, 4);
        assert_eq!(suite.aggregate.passed, 3, "only the eager parroter fails");
        assert_eq!(suite.aggregate.failed, 1);
    }

    #[test]
    #[ignore = "gated real-API tier: requires ANTHROPIC_API_KEY, on-demand only, never in CI"]
    fn gated_llm_judge_grades_full_transcripts_against_prompts_judge_md() {
        assert!(
            gated_llm_judge_enabled(),
            "set ANTHROPIC_API_KEY to run the gated LLM-judge tier"
        );
        // Real-engine replay + prompts/judge.md grading over the FULL
        // transcript (assistant + tool calls + LearnerText — review
        // reconciliation #7) lands here once Phase 4's real engine
        // (GooseEngine/goosed_client) is wired. Never runs in CI: no key,
        // and #[ignore] besides.
        todo!("wire real engine + prompts/judge.md once Phase 4's engine lands");
    }

    #[test]
    fn suite_report_matches_checked_in_fixture() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let suite = rt.block_on(run_suite(&all_personas()));
        let json = serde_json::to_string_pretty(&suite).unwrap();

        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/expected_persona_report.json"
        );
        if std::env::var("UPDATE_FIXTURES").is_ok() {
            std::fs::write(path, format!("{json}\n")).expect("write fixture");
        }
        let expected = std::fs::read_to_string(path).unwrap_or_else(|_| {
            panic!("missing fixture at {path}; run with UPDATE_FIXTURES=1 to generate it")
        });
        assert_eq!(
            json.trim(),
            expected.trim(),
            "SuiteReport JSON drifted from the checked-in fixture — if intentional, \
             re-run with UPDATE_FIXTURES=1 and review the diff"
        );
    }
}
