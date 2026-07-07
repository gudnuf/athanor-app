//! Machine-comparable eval report. `SuiteReport` serializes to timestamp-free
//! JSON — so two runs of the same pack/prompt-variant diff cleanly, with no
//! spurious noise from wall-clock time — and renders a human summary table.
//!
//! This shape is deliberately generic (pass/fail `CheckResult`s per
//! scenario), unlike rmp's `evals::report` which is specialized to a
//! precision/recall/cost extraction metric. Athanor's graders (Task 13) are
//! deterministic boolean checks (spiral discipline, salt-refusal, mask
//! fidelity), so the report only needs to carry name/passed/detail per check.

use serde::{Deserialize, Serialize};

/// The outcome of a single named check against one scenario (e.g. the
/// spiral-discipline grader, or the salt-refusal grader).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CheckResult {
    pub name: String,
    pub passed: bool,
    pub detail: String,
}

/// All checks run against one scripted scenario (persona), and whether the
/// scenario as a whole passed (every check passed).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ScenarioReport {
    pub id: String,
    pub checks: Vec<CheckResult>,
    pub passed: bool,
}

impl ScenarioReport {
    /// Builds a scenario report from its checks; `passed` is derived (every
    /// check must pass), never set independently, so the two can't drift.
    pub fn new(id: impl Into<String>, checks: Vec<CheckResult>) -> Self {
        let passed = checks.iter().all(|c| c.passed);
        ScenarioReport {
            id: id.into(),
            checks,
            passed,
        }
    }
}

/// A full eval run: which prompt-pack version was exercised, the
/// per-scenario results, and an aggregate. No timestamps, no run IDs, no
/// wall-clock anything — two runs of the same pack version against the same
/// scenarios must serialize identically, so a diff is meaningful.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SuiteReport {
    pub pack_version: String,
    pub scenarios: Vec<ScenarioReport>,
    pub aggregate: Aggregate,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Aggregate {
    pub scenarios: usize,
    pub passed: usize,
    pub failed: usize,
}

impl SuiteReport {
    pub fn assemble(pack_version: impl Into<String>, scenarios: Vec<ScenarioReport>) -> Self {
        let passed = scenarios.iter().filter(|s| s.passed).count();
        let failed = scenarios.len() - passed;
        SuiteReport {
            pack_version: pack_version.into(),
            aggregate: Aggregate {
                scenarios: scenarios.len(),
                passed,
                failed,
            },
            scenarios,
        }
    }
}

/// Fixed-width summary table. Purely for humans; the JSON is the machine
/// artifact used for comparability across runs.
pub fn render_table(suite: &SuiteReport) -> String {
    let mut out = String::new();
    out.push_str(&format!("pack_version: {}\n", suite.pack_version));
    out.push_str(&format!(
        "{:<24} {:>8} {:<}\n",
        "scenario", "passed", "checks"
    ));
    for s in &suite.scenarios {
        let failing: Vec<&str> = s
            .checks
            .iter()
            .filter(|c| !c.passed)
            .map(|c| c.name.as_str())
            .collect();
        let detail = if failing.is_empty() {
            "all ok".to_string()
        } else {
            format!("failed: {}", failing.join(", "))
        };
        out.push_str(&format!("{:<24} {:>8} {}\n", s.id, s.passed, detail));
    }
    let a = &suite.aggregate;
    out.push_str(&format!(
        "TOTAL: {}/{} scenarios passed\n",
        a.passed, a.scenarios
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(name: &str, passed: bool) -> CheckResult {
        CheckResult {
            name: name.into(),
            passed,
            detail: format!("{name} detail"),
        }
    }

    #[test]
    fn scenario_passed_is_derived_from_checks() {
        let all_ok = ScenarioReport::new("a", vec![check("spiral", true), check("salt", true)]);
        assert!(all_ok.passed);

        let one_fails = ScenarioReport::new("b", vec![check("spiral", true), check("salt", false)]);
        assert!(!one_fails.passed);
    }

    #[test]
    fn suite_aggregate_counts_passed_and_failed_scenarios() {
        let suite = SuiteReport::assemble(
            "v0",
            vec![
                ScenarioReport::new("a", vec![check("spiral", true)]),
                ScenarioReport::new("b", vec![check("spiral", false)]),
            ],
        );
        assert_eq!(suite.aggregate.scenarios, 2);
        assert_eq!(suite.aggregate.passed, 1);
        assert_eq!(suite.aggregate.failed, 1);
    }

    #[test]
    fn report_round_trips_through_json_with_no_time_field() {
        let suite = SuiteReport::assemble(
            "v0",
            vec![ScenarioReport::new("a", vec![check("spiral", true)])],
        );
        let json = serde_json::to_string_pretty(&suite).unwrap();

        assert!(json.contains("pack_version"));
        // Timestamp-free: no field name resembling a clock/run-id leaks in.
        for forbidden in ["timestamp", "time", "date", "run_id", "created_at"] {
            assert!(
                !json.to_lowercase().contains(forbidden),
                "report JSON must not contain a `{forbidden}` field: {json}"
            );
        }

        let back: SuiteReport = serde_json::from_str(&json).unwrap();
        assert_eq!(back, suite);
    }

    #[test]
    fn table_renders_one_row_per_scenario_and_a_total() {
        let suite = SuiteReport::assemble(
            "v0",
            vec![ScenarioReport::new(
                "initiation",
                vec![check("spiral", true)],
            )],
        );
        let table = render_table(&suite);
        assert!(table.contains("initiation"));
        assert!(table.contains("pack_version"));
        assert!(table.contains("TOTAL"));
    }
}
