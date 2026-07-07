//! Hermetic runner: replays each scripted persona (`personas.rs`) through the
//! real `MockEngine` and a real in-memory `Store`, grades the resulting
//! `SessionTrace` with every Task-13 grader plus two small runner-level
//! checks (mask selection, session close), and assembles a `SuiteReport`.
//!
//! ## The dispatch adapter, and why it exists (a real deviation — read this)
//!
//! `engine::ToolDispatch` (Task 8) is declared `Send + Sync` — the natural
//! bound for a trait object handed across an async engine call. `Mystagogue`
//! (Task 9) cannot satisfy it: it holds `Arc<Store>`, and `Store` wraps a
//! single `rusqlite::Connection`, which is `Send` but not `Sync` — so
//! `Arc<Store>` is neither `Send` nor `Sync` (both of `Arc<T>`'s marker impls
//! require `T: Send + Sync` *together*; `Store` only has the first half).
//! `mystagogue/acp.rs`'s own INTEGRATION NOTE flags exactly this gap and
//! recommends relaxing `engine::ToolDispatch` to `?Send` at integration time
//! (the turn is single-threaded, so the bound was never load-bearing) — but
//! that is a change to `athanor-core` src, out of scope for this evals-only
//! task.
//!
//! Until that lands, `StoreDispatch` below is a small Send+Sync-safe
//! stand-in: it holds `Arc<Mutex<Store>>` (`Mutex<T>` is `Sync` whenever
//! `T: Send`, which `Store` is) and dispatches the same six tools directly
//! against `Store`'s public API — the exact calls `Mystagogue::run` makes,
//! just not routed through that struct, since a `Mutex<Store>` can't be
//! threaded through `Mystagogue::new`'s `Arc<Store>` parameter without
//! changing its signature. This is flagged as a deviation in the task
//! report, not silently worked around: the real fix is relaxing
//! `engine::ToolDispatch`'s bound so `evals` can consume `Mystagogue`
//! directly, as Task 9 intended.

use std::sync::{Arc, Mutex};

use athanor_core::engine::{
    AcpPrompt, AcpToolCall, AcpToolResult, AcpUpdate, MockEngine, MystagogueEngine, ToolDispatch,
};
use athanor_core::store::Store;
use athanor_core::{close_session, CoreError};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::grade::{grade_mask_fidelity, grade_salt_refusal, grade_spiral, SessionTrace, Turn};
use crate::personas::{Persona, Step};
use crate::report::{CheckResult, ScenarioReport, SuiteReport};

struct StoreDispatch {
    store: Arc<Mutex<Store>>,
}

#[async_trait::async_trait]
impl ToolDispatch for StoreDispatch {
    async fn dispatch(&self, call: AcpToolCall) -> AcpToolResult {
        let store = self.store.lock().expect("store mutex poisoned");
        let value =
            run_tool(&store, &call.name, call.args).unwrap_or_else(|e| json!({ "error": e }));
        AcpToolResult { id: call.id, value }
    }
}

// ---- tool dispatch, mirroring Mystagogue::run's match arms (see module
// docs: Mystagogue itself can't be used here without relaxing
// engine::ToolDispatch's Send+Sync bound) ----

#[derive(Deserialize)]
struct FixSaltArgs {
    realization: String,
    thread_id: String,
    #[serde(default)]
    domains: Vec<String>,
    #[serde(default)]
    child_question: Option<String>,
}

#[derive(Deserialize)]
struct OpenThreadArgs {
    question: String,
    #[serde(default)]
    domain: Option<String>,
}

#[derive(Deserialize)]
struct EvaporateArgs {
    id: String,
}

#[derive(Deserialize)]
struct KindleArgs {
    term: String,
}

#[derive(Deserialize)]
struct WeaveArgs {
    a: String,
    b: String,
    note: String,
}

#[derive(Deserialize)]
struct UpdateMemoryArgs {
    section: String,
    content: String,
}

fn run_tool(store: &Store, name: &str, args: Value) -> Result<Value, String> {
    let stringify = |e: CoreError| e.to_string();
    match name {
        "fix_salt" => {
            let a: FixSaltArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let realization = store
                .fix_salt(
                    &a.thread_id,
                    &a.realization,
                    &a.domains,
                    a.child_question.as_deref(),
                )
                .map_err(stringify)?;
            Ok(json!({
                "realization_id": realization.id,
                "child_thread_id": realization.child_thread_id,
            }))
        }
        "open_thread" => {
            let a: OpenThreadArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let domain_id = match a.domain.as_deref() {
                Some(name) if !name.trim().is_empty() => {
                    Some(store.upsert_domain(name).map_err(stringify)?.id)
                }
                _ => None,
            };
            let thread = store
                .open_thread(&a.question, domain_id.as_deref(), None)
                .map_err(stringify)?;
            Ok(json!({ "thread_id": thread.id }))
        }
        "evaporate_thread" => {
            let a: EvaporateArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            store.evaporate_thread(&a.id).map_err(stringify)?;
            Ok(json!({ "thread_id": a.id, "state": "evaporated" }))
        }
        "kindle_passage" => {
            let a: KindleArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let kindled = store.kindle_passage(&a.term, None).map_err(stringify)?;
            Ok(json!({ "term": a.term, "kindled": kindled }))
        }
        "weave_domains" => {
            let a: WeaveArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let corr = store
                .weave_domains(&a.a, &a.b, &a.note)
                .map_err(stringify)?;
            store
                .kindle_passage("CITRINITAS", Some(&corr.id))
                .map_err(stringify)?;
            store
                .kindle_passage("AZOTH", Some(&corr.id))
                .map_err(stringify)?;
            Ok(json!({ "correspondence_id": corr.id }))
        }
        "update_memory" => {
            let a: UpdateMemoryArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            store
                .set_profile_section(&a.section, &a.content)
                .map_err(stringify)?;
            Ok(json!({ "section": a.section, "ok": true }))
        }
        other => Err(format!("unknown tool: {other}")),
    }
}

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
fn check_session_lands(store: &Mutex<Store>, session_id: &str) -> CheckResult {
    let guard = store.lock().expect("store mutex poisoned");
    let result = close_session(&guard, session_id, 1, &[]);
    CheckResult {
        name: "session_lands".into(),
        passed: result.is_ok(),
        detail: match result {
            Ok(()) => "session closed cleanly".into(),
            Err(e) => format!("session failed to close: {e}"),
        },
    }
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

    let store = Arc::new(Mutex::new(store));
    let dispatch = StoreDispatch {
        store: store.clone(),
    };

    let steps = (persona.build)(&thread.id);
    let mut turns: Vec<Turn> = Vec::new();

    for step in steps {
        match step {
            Step::Learner(text) => turns.push(Turn::LearnerText(text)),
            Step::Engine(script) => {
                let engine = MockEngine::new(script);
                // `MockEngine::run_turn` ignores the prompt entirely (the
                // script drives everything), and `Mystagogue::tool_specs()`
                // returns the *mystagogue-local* `AcpToolSpec` mirror (see
                // module docs), not `engine::AcpToolSpec` — so there's
                // nothing real to put here yet.
                let prompt = AcpPrompt {
                    system: String::new(),
                    user_turns: Vec::new(),
                    tools: Vec::new(),
                };

                // Coalesce contiguous TextDelta pieces into one Assistant
                // turn, flushing whenever a ToolCall interrupts them — this
                // preserves the actual stream order (text, then tool, then
                // more text, ...) instead of reordering by update kind.
                let mut local_turns: Vec<Turn> = Vec::new();
                let mut buffer = String::new();
                engine
                    .run_turn(prompt, &dispatch, &mut |update| match update {
                        AcpUpdate::TextDelta(delta) => buffer.push_str(&delta),
                        AcpUpdate::ToolCall(call) => {
                            if !buffer.is_empty() {
                                local_turns.push(Turn::Assistant(std::mem::take(&mut buffer)));
                            }
                            local_turns.push(Turn::ToolCall {
                                name: call.name.clone(),
                                args: call.args.to_string(),
                            });
                        }
                        AcpUpdate::TurnComplete => {}
                    })
                    .await
                    .expect("mock engine turn");
                if !buffer.is_empty() {
                    local_turns.push(Turn::Assistant(buffer));
                }
                turns.extend(local_turns);
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

    fn check<'a>(report: &'a ScenarioReport, name: &str) -> &'a CheckResult {
        report
            .checks
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("no '{name}' check in {report:?}"))
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
