# real-api reports

Output of the gated real-API tier (`cargo test -p evals --features real-api -- --ignored`):
`<pack_version>-<model>.json` (deterministic grader results) and `.judge.md` (taste-check checklist + transcripts). Only this README is tracked.

Env: ANTHROPIC_API_KEY (required, env only), EVALS_REAL_PERSONAS (comma list), EVALS_REAL_TURNS (default 6).
