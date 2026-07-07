# Plan Review — Athanor App Implementation (Phases 0–3)

**Reviewer:** plan-reviewer (sonnet)
**Date:** 2026-07-06
**Target:** `docs/superpowers/plans/2026-07-06-athanor-app-implementation.md` (1078 lines, 15 tasks)
**Method:** every goose-crate claim checked against the real `v1.41.0` checkout (scratchpad clone); rmp donor code read directly; all six worked examples hand-recomputed; repo diffed against the live `~/athanor-app` (Tasks 1–3 already built).

## Overall verdict: **APPROVE WITH EDITS.**

The plan is architecturally sound, internally consistent, and the arithmetic holds. **Zero hard blockers.** The corrections below are (a) reconciling Tasks 1–5 against work that landed after drafting, (b) three factual fixes to the VERIFY list from the real goose source, and (c) a handful of PASS-WITH-EDITS nits. The single highest-value correction is **Task 4 Step 2's build command** (build the *rlib*, not all targets) — without it the Goose spike can false-RED on a link error that never touches the embed path.

---

## Worked-example recomputation (all six reproduce)

| Example | Plan claim | Recomputed | Verdict |
|---|---|---|---|
| PCM buffer (Task 2) | 80,000 samp/5s → 312.5 KiB; 9s=144,000; stride 64,000; W2 needs 208,000 | 5·16000=80,000; ×4=320,000 B ÷1024 = **312.5 KiB**; 9·16000=**144,000**; stride 4s=**64,000**; W2 start 128,000+80,000=**208,000>144,000** | ✅ exact. Confirmed against rmp `chunk.rs:103` (`144_000 // 9 s → [0,5s) and [4s,9s)`) and `stream_append_only.rs:33` (`208_000 // three windows`). 48k→16k factor 3 and 4096f=85.33ms→1365 samp all correct. |
| RTF headroom (Task 5) | base.en 0.51/59.8=0.0085≈0.009; keep-up RTF<0.8; device 0.045–0.09 → 0.23–0.45s/window | 0.51/59.8=**0.00853**; 5·RTF<4 ⇒ **RTF<0.8**; 5–10× of 0.009 = 0.045–0.09; 5×that=**0.225–0.45s** ≪4s | ✅ holds. (Spike measured 0.526/59.765=**0.0088**; plan's 0.51/59.8 is a slightly rounded illustrative pair, same order.) |
| Migration runner (Task 6) | user_version 0→ entry0 wrapped `PRAGMA user_version=1` (i+1); broken N leaves N−1 | rmp `migrations.rs:134` `migrate_with`: `.enumerate().skip(version)`, wraps `BEGIN;…PRAGMA user_version={i+1};COMMIT;` | ✅ verbatim match. Rollback test logic correct. |
| Wisdom accounting (Task 7) | 7+5 min same day → 1 tending row, minutes=12, wisdom=1; next day→2 | day PK ⇒ 2nd `record_tending` hits PK upsert-add → minutes=12, count(*)=1; new day inserts → 2 | ✅ correct. |
| Dice salt-refusal (Task 13) | token_set("entropy is disorder")=**{entropy,is,disorder} (3)**; parrot dice=2·3/(3+3)=1.0; own=0/6=0.0 | rmp `token_set` **filters STOPWORDS** → {entropy,disorder} = **2 tokens**; parrot dice=2·2/(2+2)=**1.0**; own ∩=∅ → **0.0** | ⚠️ **conclusions correct, token illustration wrong** — "is" is dropped as a stopword, so it's 2 tokens and 2·2/4, not 3 and 2·3/6. Same 1.0/0.0 result. Fix the annotation so a builder doesn't hardcode a 3-token expectation. |
| Snapshot matrix (Task 14) | 3 masks × 5 modes + 1 initiation = 16 | 3·5=15, +1=**16** | ✅ correct. |

---

## VERIFY-AT-SPIKE list — corrected against real goose `v1.41.0`

Checked the actual `crates/goose/Cargo.toml` at tag `v1.41.0` (clone in scratchpad) plus the in-flight iOS build log.

- **#1 Portable feature set — CORRECT THE PLAN.** `portable-default` **exists** (`= ["rustls-tls", "aws-providers", "telemetry", "otel"]`). But two plan assumptions are wrong:
  - `rustls-tls` is **not** "no server" — it explicitly enables `dep:axum-server` + `axum-server/tls-rustls` (plus `sqlx`, `oauth2`, `jsonwebtoken`, `rcgen`, `pem`, **`aws-lc-rs`**). The spec's "no axum server, engine only" is not achievable by feature selection alone at this tag. The real excludes that matter are `local-inference`/`mlx`/`cuda`/`vulkan` (candle), `system-keyring` (keyring), `nostr`, `aws-providers`, `otel`. The confirmed-minimal build the spike used is `--no-default-features --features rustls-tls` (accepting axum-server as an unused transitive).
  - Plan text §VERIFY-1 is otherwise accurate that there are no `llama-cpp`/`deno`/`v8` features by name.
- **#2 rlib crate-type — RESOLVED GREEN.** `libgoose.rlib` **built successfully** for `aarch64-apple-ios` (artifact present in `target-ios/…/debug/libgoose.rlib`). Usable as a normal lib dependency.
- **#6 ACP type crates — CORRECT THE PLAN.** Reality at this tag: **`agent-client-protocol` = "1.0"** (not `"*"`, not alpha), **`agent-client-protocol-schema` = "1.1"** (this is where the **`unstable`** feature lives, NOT on the main crate), **`agent-client-protocol-http` = "1.0"**, and **`goose-sdk-types`** is `version.workspace = true` → **1.41.0**, *not* "0.1.0-alpha". Fix the spike Cargo.toml (plan line 329): it puts `unstable` on `agent-client-protocol` and uses `version="*"`; it should pin `agent-client-protocol="1.0"` and put `unstable` on `-schema="1.1"`. Good news: these are 1.x, more stable than the plan/spec feared — de-escalate the "0.1.0-alpha" risk note.
- **#7 Tag pin — RESOLVED.** `v1.41.0` is the latest release, cloned and checked out (`39c27c387`). It is the pin.
- **#8 iOS cross-compile — RESOLVED FOR THE RLIB; reframe the criterion.** The **rlib compiles clean** for device. The link error in the build log (`___chkstk_darwin` undefined, from `aws-lc-rs`/`blake3`) occurs only when linking goose's **own bins** (`build_canonical_models`, etc.), which athanor-core never builds (deps don't build bins). The missing symbol is a compiler-rt builtin that **Xcode supplies at final app-link** — it is not an rlib-embed blocker. **Restate #8 as:** "portable rlib cross-compiles GREEN; final app/bin link must provide compiler-rt (Xcode does; raw `cargo build` of a bin does not)."
- **#9 UniFFI version — RESOLVED, STRIKE IT.** The live repo already builds on **UniFFI 0.31 proc-macro mode** (`crates/ffi`, genesis commit `df9bd55`). No longer open.
- **#3/#4/#5 Embed/extension/streaming API — STILL OPEN (host round-trip pending).** `Agent::new()` and `AgentConfig::new(...)` exist (`agents/agent.rs:319/188`), so an in-process construction path is real; exact provider/extension-registration/streaming calls are for the round-trip test to nail. Keep open.
- **#10 Whisper device RTF — STILL OPEN (operator-gated).** Mac RTF + iOS rlib cross-compile are GREEN per the whisper spike report; device row is gudnuf's.

---

## Per-task verdicts

**Phase 0 (Tasks 1–3) — reviewed as a DIFF vs the already-built repo. See gap list below.**

- **Task 1 — PASS-WITH-EDITS (mostly built).** Repo genesis is live (6 commits, public). Gaps vs plan: no secret-scan CI step, no `.env.example`, no `*.bin`/model-blob gitignore, plan file not copied into `docs/plans/`. CI diverges (macos-14 + rustup, not Nix devshell). Details in gap list.
- **Task 2 — PASS.** `crates/stt` copied verbatim, hermetic (`default=[]`, `whisper=["dep:whisper-rs"]`, `=0.16.0` metal optional) — all confirmed. Only nit: grimoire provenance line omits the rmp git rev (plan Step 4 asked for `@ <rev>`; HEAD is `af4afe1`). Worked example fully validated against rmp source.
- **Task 3 — PASS-WITH-EDITS (built but diverges).** The plan's fail-closed secret-scan is **absent** from the live `ci.yml`, and CI runs on `macos-14` with `dtolnay/rust-toolchain` rather than the Nix devshell the plan specifies ("CI matches local exactly"). Reconcile: either add the secret-scan + move to Nix, or amend the plan to match the built CI and justify the macOS runner.

**Phase 1 (Tasks 4–5)**

- **Task 4 — PASS-WITH-EDITS.** Structure is right and the RED fallback is sound. Required edits: (1) **Step 2 build the rlib, not all targets** — use `cargo build --lib --target aarch64-apple-ios` (or build the spike crate whose only bin is intended), else the aws-lc-rs bin-link error false-REDs a GREEN rlib; document that final app-link needs compiler-rt via Xcode. (2) Apply the #1/#6 crate/feature corrections to the spike `Cargo.toml`. (3) The spike is in flight in the scratchpad; when `forge/athanor-app/spike-goose-ios-report.md` lands, fold its confirmed embed API into Tasks 4 & 8. GREEN criteria 1–4 are well-formed.
- **Task 5 — PASS.** Substantially done per `spike-whisper-report.md` (Mac base.en RTF 0.0088 through the production crate; iOS rlib cross-compile clean, device SDK + Metal linked; turnkey device runbook staged; GO-provisional). One reconciliation: the report's device path uses **whisper.cpp's own SwiftUI example app**, not the plan Step 1's `SttStream::with_model` bench harness — acceptable (fastest RTF-only answer; the plan's harness stays valid for the sim/mac pre-check). Plan wanted nothing the spike failed to cover.

**Phase 2 (Tasks 6–11)**

- **Task 6 — PASS-WITH-EDITS.** Schema is internally consistent: 11 tables match the `count=11` test; the `threads↔realizations` FK cycle resolves because both cross-link columns (`parent_realization_id`, `child_thread_id`) are **nullable** and `realizations.thread_id` always points to the already-existing parent thread in `fix_salt` — with immediate FK checking, every row-write references only extant rows or NULL. Forward FK refs at `CREATE` (`traces.session_id`→`sessions` created later; `threads.parent_realization_id`→`realizations` created later) are legal in SQLite. `unchecked_transaction()` (used for the insert-order trick) is confirmed rmp/rusqlite-0.32 API. **Edit:** the DDL defines **no indices**; `ripe_threads` scans `threads.state`+`last_worked`. Fine for an on-device single-user DB, but state it explicitly (correctness OK, note added). Migration worked example validated verbatim.
- **Task 7 — PASS.** State DAG + wisdom-counts-days logic is coherent and matches the worked example.
- **Task 8 — PASS.** Executable hermetically with no spike dependency (mock-only; `engine/goose.rs` is a `#[cfg]` stub). ACP-owned-types-at-the-seam is correctly the point. Dry-ran as a cold builder: every path/type present; test helpers (`RecordingDispatch`, `demo_prompt`) are normal scaffolding.
- **Task 9 — PASS.** The load-bearing spiral test is well-specified; `fix_salt` single-transaction (realization → child thread → back-link → parent Fixed → kindle SALT) is FK-consistent with the Task 6 schema. `fix_salt` as sole writer + `try_mutate_realization`→`Err(Immutable)` enforces immutability by absence. Dry-ran clean.
- **Task 10 — PASS-WITH-EDITS.** Assembly order + determinism (no `now()`/random) is right and snapshot-friendly. **Edit:** ingest content from the existing `forge/athanor-app/prompt-pack-v0/` rather than inventing placeholders (see reconciliation). File-layout mismatch: plan wants `prompts/modes/{trace,explain,predict,challenge,design}.md` (5 files) but the pack ships one `modes.md`; and `identity.md`/`condensation.md` vs pack's `core-identity.md`/`condensation-protocol.md`. Decide the on-disk layout and map the pack onto it.
- **Task 11 — PASS.** Thin CLI, logic-in-core invariant preserved; hermetic smoke test is coherent.

**Phase 3 (Tasks 12–15)**

- **Task 12 — PASS.** `normalize::{token_set,dice}` and timestamp-free `report` copy the confirmed rmp shapes; the 0.5 item-match threshold in rmp is real (`normalize.rs:271/282`), so Task 13's stricter 0.7 is a deliberate, consistent choice.
- **Task 13 — PASS-WITH-EDITS.** Graders are pure/deterministic and correct in intent. **Edit:** fix the Dice worked-example token count ("is" is a stopword → 2 tokens; result unchanged at 1.0). The Philosophus mask proxy (declarative-detection) is explicitly scoped as coarse — good.
- **Task 14 — PASS-WITH-EDITS.** Same ingest note as Task 10. Matrix count (16) is correct. Snapshots lock structure, not prose — right call.
- **Task 15 — PASS-WITH-EDITS.** **Edit (reconciliation #7):** make explicit that the gated LLM-judge is fed the **full transcript including learner turns** (not just Mystagogue output) — mask-fidelity/condensation-honesty grading is meaningless without the learner side. The deterministic `SessionTrace` already carries `LearnerText`; Step 4's judge wiring must pass the same. Hermetic-in-CI / gated-never-in-CI split is correct.

---

## Phase-0 gap list (becomes a small follow-up task)

What the plan wanted for Tasks 1–3 that the live repo lacks:

1. **Secret-scan CI step MISSING.** Plan Task 3 has a fail-closed `git grep … exit 1` gate; live `ci.yml` has no such step. This is the constitutional gate — add it. (High priority given public repo.)
2. **`*.bin` / model-blob gitignore MISSING.** Plan `.gitignore` ignores `*.bin` + `spikes/whisper-ios/models/`; live `.gitignore` has neither. Whisper `.bin` models could be committed by accident.
3. **`.env.example` MISSING.** `.gitignore` already whitelists it (`!.env.example`) but the file was never created. Add a committed key-less template.
4. **CI does not use the Nix devshell.** Live CI = `macos-14` + `dtolnay/rust-toolchain@stable` + `Swatinem/rust-cache`; plan = `ubuntu-latest` + Nix (`nix develop -c …`) so "CI matches local exactly." Decide which is canonical; if keeping macOS/rustup, amend the plan and justify (hermetic tier needs neither macOS nor Metal).
5. **Plan not copied into `docs/plans/`.** `docs/plans/` holds only `.gitkeep`; plan Task 1 Step 1 copies this file in as the repo's working copy.
6. **Grimoire provenance rev missing.** `docs/grimoire.md` records the stt copy but omits the rmp git rev (`af4afe1`) the plan asked for.

What the repo has that the plan didn't anticipate (all fine / ahead of plan):
- `crates/ffi` already scaffolded on **UniFFI 0.31 proc-macro** (resolves VERIFY #9).
- `apps/ios` SwiftUI shell already scaffolded via xcodegen; `justfile`, `docs/agent-brief.md`, `docs/invariants.md`, committed `Cargo.lock`. `.gitignore` is broader than the plan's (adds `.env.*`, `*.mobileprovision`, `.direnv/`, iOS build dirs).

**No file collision** between Tasks 6–15 (all in `athanor-core`/`cli`/`evals`/`ffi`) and the in-flight endpointing builder (in `crates/stt`) — confirmed. Task 2's stt copy is already committed, so endpointing edits land on the copy, not a shared path.

---

## Decision-consistency check (plan Self-Review items)

- (a) **Mask-fidelity hermetic proxy** — consistent; explicitly scoped coarse with deep fidelity deferred to the gated judge. (Pair with the Task 15 full-transcript edit.)
- (b) **FK-cycle insert order** — sound; relies on nullable cross-link columns + immediate FK checking, not deferral. Verified against schema + rmp `unchecked_transaction`.
- (c) **Child-thread synthesis strictness** — consistent with the spiral invariant being structural (Task 9 synthesizes even when the model omits the child question).
- (d) **stt copy provenance** — consistent with PROCESS.md's "copy, not depend; the diff is the graduation signal"; only the git-rev annotation is missing (gap #6).

None contradict the specs or PROCESS.md invariants.
