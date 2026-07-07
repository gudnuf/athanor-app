# Athanor App Implementation Plan (Phases 0–3)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stand up the `athanor-app` repo, prove the two load-bearing spikes (Goose embedded on iOS; Whisper RTF on device), and build the hermetic Rust skeleton — tria prima store, session state machine, embedded-Goose Mystagogue behind ACP seams, prompt assembly, dev CLI, and the eval harness scaffolding — up to but not including the voice-first shippable UI.

**Architecture:** A `damsac/rmp`-style Cargo workspace. All meaning lives in `athanor-core` (Rust): a SQLite tria prima store, a session state machine, prompt assembly, and an embedded Goose engine driving a single in-process "Mystagogue" extension whose six tools write to the store. Every seam we own between our code and the Goose engine speaks ACP types, so engine churn can't reach our shapes. Voice (the Bellows) is a copied `crates/stt` whisper pipeline; the Swift shell owns only audio capture. A dev CLI over `athanor-core` gives desktop prompt iteration; an `evals` crate grades prompt versions with deterministic (hermetic) and gated real-API tiers.

**Tech Stack:** Rust (edition 2021, stable 1.95 toolchain via rust-overlay), `rusqlite` (bundled SQLite), `uniffi` 0.28+ (proc-macro mode), `whisper-rs =0.16.0` (Metal, feature-gated), the `goose` crate as an rlib (feature-gated, pinned tag), `agent-client-protocol` + `goose-sdk-types` for seam types, Nix devshell, GitHub Actions CI.

## Global Constraints

- **Repo is PUBLIC from commit zero** (`github.com/gudnuf/athanor-app`). Secrets discipline is constitutional: `.env` is gitignored and shell-sourced, NEVER read into agent context or committed; no keys, certs, or `.p12` anywhere in the tree; a secret-scan gate runs in CI and before any force-push.
- **Commits are clean** — no `Co-Authored-By` footers. Rebase + ff-merge, linear history. Commit on green only.
- **Workspace tests stay hermetic by default.** The `whisper` feature (native whisper.cpp), the `goose` feature (real engine), and any real-API path are feature/env-gated OFF so `cargo test --workspace` needs no native toolchain, no model file, no network, no API key.
- **UniFFI 0.31 target** for bindings per the product spec; the rmp donor uses 0.28 — pin whichever the current template resolves and record it (VERIFY-AT-SPIKE #9). Proc-macro mode only (no UDL, no build.rs).
- **rmp invariants, machine-checkable:** no business logic in Swift; salt (`realizations`) is immutable once approved; fire (`tending`) is append-only; every `fix_salt` spawns a child thread (the spiral invariant — also a deterministic eval check).
- **ACP types at every owned seam** touching the Goose engine. Engine upgrades are their own deliberate task, never incidental. The Goose tag is pinned at spike time and recorded in `STATE.md` and this plan (Task 4).
- **Meta docs** (PROCESS.md, STATE.md) live in `~/athanor/projects/athanor-app/`, NOT the app repo. Plans/research/grimoire travel *with* the code in `~/athanor-app/docs/` because the repo is public-process. This plan file is authored at `~/athanor/docs/superpowers/plans/`; **Task 1 copies it into `~/athanor-app/docs/plans/2026-07-06-athanor-app-implementation.md`** as the repo's working copy.
- **Toolchain invocation:** all cargo runs go through the devshell: `nix develop -c cargo ...` (or inside `direnv`-loaded shell). Never assume a host rustup.
- **Two operator gates** (a human — gudnuf — must act, an agent cannot): (1) the on-device iPhone runs in Spikes 1a-device and 1b-device (Tasks 4 & 5); (2) approval of this plan before any Task begins. Agents run the simulator / mac-Metal pre-checks alone and STOP at the device line.

---

## File Structure

```
~/athanor-app/
  Cargo.toml                         # workspace: crates/*, excludes spikes/
  rust-toolchain.toml                # channel = stable
  flake.nix / flake.lock             # devshell: rust 1.95 + iOS targets, cmake, clang, LIBCLANG_PATH
  .envrc                             # use flake
  .gitignore                         # /target, .env, /screenshots, model blobs
  .env                               # gitignored; ANTHROPIC_API_KEY for dev CLI + gated evals
  README.md
  .github/workflows/ci.yml           # hermetic tier only
  docs/
    plans/2026-07-06-athanor-app-implementation.md   # working copy of THIS file
    research/                        # spike reports land here
    grimoire.md                      # the app's own dated realizations
  crates/
    athanor-core/                    # ALL meaning
      src/lib.rs
      src/error.rs
      src/ids.rs                     # new_id() (uuid v7)
      src/domain.rs                  # tria prima domain structs + enums
      src/store/mod.rs               # Store::open / migrate / now / clock
      src/store/migrations.rs        # append-only MIGRATIONS array + user_version runner
      src/store/domains.rs           # sulfur: domains + pull_notes
      src/store/threads.rs           # mercury: thread lifecycle
      src/store/realizations.rs      # salt: fix_salt + spiral child-thread (immutable)
      src/store/tending.rs           # fire: append-only day rows + wisdom count
      src/store/profile.rs           # learner profile sections (update_memory)
      src/store/traces.rs            # one-line session traces
      src/store/kindling.rs          # Tabula passage kindling events
      src/store/correspondences.rs   # weave_domains (Azoth schema, mask deferred)
      src/session.rs                 # session state machine
      src/prompt/mod.rs              # assembly: profile + ripe mercury + mode + mask
      src/prompt/assets.rs           # loads versioned prompt assets from prompts/
      src/engine/mod.rs              # MystagogueEngine trait (ACP-typed seam)
      src/engine/acp.rs             # our ACP-shaped structs (survive engine churn)
      src/engine/mock.rs            # MockEngine: scripted ACP responses (hermetic)
      src/engine/goose.rs           # #[cfg(feature="goose")] real embed
      src/extension/mod.rs          # Mystagogue extension: 6 tools -> store
      prompts/                       # versioned prompt assets (assets.rs reads these)
        identity.md  masks/*.md  modes/*.md  condensation.md  initiation.md  judge.md
    stt/                             # COPIED verbatim from ~/murmur-rmp/crates/stt
    ffi/                             # UniFFI bridge skeleton (thin in P0-3; fills in P4)
    athanor-cli/                     # thin dev CLI over athanor-core (desktop iteration)
    evals/                           # dev-side harness: graders + personas + report
  spikes/                            # EXCLUDED from workspace (prefix match, not glob)
    goose-ios/                       # Spike 1a
    whisper-ios/                     # Spike 1b (may symlink models, gitignored)
  apps/ios/                          # SwiftUI shell (dormant scaffold in P0-3)
```

---

## VERIFY-AT-SPIKE LIST (unverified goose/toolchain assumptions)

Every assumption below is unconfirmed from the planning host. The Task 4 spike worker resolves each and records the answer in `docs/research/2026-07-0X-goose-ios-spike.md` and `STATE.md`. Do NOT let any later task harden one of these into a claim until the spike confirms it.

1. **Portable feature set.** The `goose` crate exposes a `portable-default` feature. ASSUMED it (or `--no-default-features --features rustls-tls`) yields an rlib with no `system-keyring`, no `nostr`, no `local-inference`/`mlx`/`cuda`/`vulkan`, no server/CLI. The spec's "drops llama-cpp, V8/deno" language predates the current feature list (there are no `llama-cpp`/`deno`/`v8` features by name in v1.41.0 — the relevant excludes are `local-inference`, `mlx`, `system-keyring`, `nostr`). Confirm the exact minimal set that cross-compiles for `aarch64-apple-ios`.
2. **rlib crate-type.** `goose` has no `[lib] crate-type` override → defaults to rlib. ASSUMED usable as a normal path/git dependency. Confirm it builds as a lib dependency (not just a bin workspace member).
3. **Embed/agent construction API.** There is NO stable embed API. ASSUMED an in-process `Agent` (or equivalent) can be constructed, given an Anthropic provider + API key, without spawning `goosed` or a CLI. Confirm the actual constructor path and record the exact types.
4. **Custom in-process extension registration.** ASSUMED a builtin-style extension exposing named tools can be registered on the in-process agent (no external MCP process, which iOS forbids). Confirm the registration trait/mechanism and tool-schema shape.
5. **Streaming seam.** ASSUMED `session/update`-style notifications are observable in-process (callback/stream) for token-by-token output. Confirm the notification type and subscription mechanism.
6. **ACP type crates.** ASSUMED `agent-client-protocol` (with the `unstable` feature) + `goose-sdk-types` (0.1.0-alpha) provide the structs we mirror at our seam. Confirm crate names/versions and which types are actually stable enough to name.
7. **Tag pin.** ASSUMED we pin a specific tag (latest release observed: `v1.41.0`, 2026-07-03). The spike worker picks and records the exact tag/rev.
8. **iOS cross-compile cleanliness.** ASSUMED the portable rlib cross-compiles for `aarch64-apple-ios` with only cmake/clang from the devshell (no host-only C deps sneaking in via a transitive feature). This is the whole point of Spike 1a; treat red honestly.
9. **UniFFI version.** Product spec says 0.31; rmp donor uses 0.28. Confirm which the template resolves and pin it.
10. **whisper-rs on iOS device.** RTF measured only on Mac (RESULTS.md). Device tier is unrun (operator-gated). base.en Mac RTF 0.009; ASSUMED device stays < 1.0. Confirm in Task 5-device.

---

# PHASE 0 — Repo Genesis

## Task 1: Scaffold the workspace from the rmp template

**Files:**
- Create: `~/athanor-app/Cargo.toml`, `rust-toolchain.toml`, `flake.nix`, `.envrc`, `.gitignore`, `README.md`, `.env` (gitignored, empty placeholder), `docs/plans/2026-07-06-athanor-app-implementation.md` (copy of this file), `docs/grimoire.md`
- Reference (read-only, do NOT depend on): `~/murmur-rmp/Cargo.toml`, `~/murmur-rmp/flake.nix`, `~/murmur-rmp/.gitignore`, `~/murmur-rmp/rust-toolchain.toml`

**Interfaces:**
- Produces: a buildable empty workspace and a public git remote. Later tasks add crates to `[workspace] members`.

- [ ] **Step 1: Create the tree and copy config scaffolding**

```bash
mkdir -p ~/athanor-app/{crates,spikes,apps/ios,docs/plans,docs/research,.github/workflows}
cd ~/athanor-app
git init -b main
cp ~/murmur-rmp/rust-toolchain.toml .
cp ~/murmur-rmp/flake.nix .            # then edit description string below
cp ~/murmur-rmp/flake.lock .
cp ~/murmur-rmp/.envrc .
cp ~/athanor/docs/superpowers/plans/2026-07-06-athanor-app-implementation.md docs/plans/
```

- [ ] **Step 2: Write `Cargo.toml`** (workspace root — members added as crates land; note the non-glob exclude)

```toml
[workspace]
resolver = "2"
members = ["crates/athanor-core", "crates/stt", "crates/ffi", "crates/athanor-cli", "crates/evals"]
exclude = ["spikes"]   # prefix match — NOT "spikes/*" (workspace.exclude is not glob; cargo #11405)

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
async-trait = "0.1"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }
rusqlite = { version = "0.32", features = ["bundled"] }
uuid = { version = "1", features = ["v7"] }
```

  Until `crates/*` exist, temporarily set `members = []` so `cargo metadata` succeeds; each crate task re-adds its own member line. (Or create the crates as empty stubs in this task — either is acceptable; the deliverable is a green `nix develop -c cargo metadata`.)

- [ ] **Step 3: Write `.gitignore`** (secrets discipline is constitutional)

```gitignore
/target
**/*.rs.bk
.DS_Store

# local secrets — NEVER committed
.env
*.p12
*.pem
certs/

# simulator/QA capture evidence
/screenshots/

# whisper model blobs (downloaded by shell/spike, never committed)
*.bin
spikes/whisper-ios/models/
```

- [ ] **Step 4: Edit `flake.nix`** — change only the `description` to `"Athanor app — Rust core workspace"`. Keep the pinned `1.95.0` toolchain, the three iOS targets (`aarch64-apple-ios`, `aarch64-apple-ios-sim`, `x86_64-apple-ios`), `cmake`, `clang`, and `LIBCLANG_PATH` exactly as the donor has them (whisper-rs-sys bindgen needs libclang; goose cross-compile needs the iOS targets).

- [ ] **Step 5: Write `.env` placeholder and `README.md`**

`.env` (gitignored — this file is created but its real content is filled by the operator, never an agent):
```bash
# gitignored. Source with `set -a; . ./.env; set +a` in a shell only.
# ANTHROPIC_API_KEY=...   # for the dev CLI and gated real-API evals ONLY
```
`README.md`: one paragraph — what Athanor is, "public from day one", link to `docs/plans/`, and the devshell run line `nix develop -c cargo test --workspace`.

- [ ] **Step 6: Verify the empty workspace builds**

Run: `cd ~/athanor-app && nix develop -c cargo metadata --format-version 1 >/dev/null && echo OK`
Expected: `OK` (no members yet, or empty stub crates — metadata resolves cleanly).

- [ ] **Step 7: Create the public GitHub repo and push**

```bash
cd ~/athanor-app
git add -A
git commit -m "genesis: rmp-scaffolded workspace, public-from-day-one, secrets gitignored"
gh repo create gudnuf/athanor-app --public --source=. --remote=origin --push
```

- [ ] **Step 8: Secret-scan before trusting the push** (fail-closed)

Run: `git log -p | grep -nEi 'sk-ant|ANTHROPIC_API_KEY=.|BEGIN .*PRIVATE KEY|\.p12' || echo CLEAN`
Expected: `CLEAN`. If anything prints, STOP — do history surgery before continuing.

**Do NOT touch:** any real API key; the `spikes/` exclude must stay a prefix (`"spikes"`), never `"spikes/*"`.

---

## Task 2: Copy `crates/stt` in verbatim (the STT donor)

**Files:**
- Create (by copy): `crates/stt/**` from `~/murmur-rmp/crates/stt/` (Cargo.toml, README.md, src/{lib,bias,chunk,decoder,finalize,whisper}.rs, tests/stream_append_only.rs)
- Modify: `~/athanor-app/Cargo.toml` (ensure `crates/stt` is a member)

**Interfaces:**
- Produces: `stt::{SttStream, SttConfig, FinalizedSegment, Decoder, ScriptedDecoder}` and, behind `--features whisper`, `stt::WhisperDecoder`. Consumed later by the FFI/Bellows lane (Phase 4). Copy — do NOT add a dependency on the sitewalk repo (the Athanor↔sitewalk diff is the factory's graduation signal).

- [ ] **Step 1: Copy the crate**

```bash
cp -R ~/murmur-rmp/crates/stt ~/athanor-app/crates/stt
```

- [ ] **Step 2: Confirm `crates/stt/Cargo.toml` keeps whisper OFF by default**

The copied Cargo.toml must read `default = []` and `whisper = ["dep:whisper-rs"]` with `whisper-rs = { version = "=0.16.0", features = ["metal"], optional = true }`. Do not change these.

- [ ] **Step 3: Run the hermetic stt tests (no whisper feature)**

Run: `cd ~/athanor-app && nix develop -c cargo test -p stt`
Expected: PASS — `bias_prompt_is_passed_to_every_decode`, `poll_finalizes_incrementally_and_end_flushes_bounded_tail`, `poll_is_a_noop_until_a_window_is_ready`, `config_rejects_overlap_ge_chunk`, and `tests/stream_append_only.rs` all green, no native toolchain touched.

- [ ] **Step 4: Record the copy provenance in the grimoire**

Append to `docs/grimoire.md` a dated line: "stt copied from murmur-rmp @ <git rev of ~/murmur-rmp HEAD> — copy, not dependency; diff is the factory graduation signal." Get the rev with `git -C ~/murmur-rmp rev-parse --short HEAD`.

- [ ] **Step 5: Commit**

```bash
git add crates/stt Cargo.toml docs/grimoire.md
git commit -m "stt: copy whisper Bellows crate from murmur-rmp (hermetic tests green)"
```

**Do NOT touch:** the stt source logic (it is a verbatim donor); do NOT enable `whisper` in `default`.

**Worked example (PCM buffer sizes — grounds the copied API for the later Bellows lane):**
16 kHz mono f32. One 5.0 s decode window = `5.0 × 16000 = 80,000` samples; f32 is 4 bytes → `80,000 × 4 = 320,000` bytes = 312.5 KiB per window. Nine seconds = `9 × 16000 = 144,000` samples (exactly the `vec![0.0; 144_000]` in the copied test). With `chunk_secs=5, overlap_secs=1` the window stride is `chunk − overlap = 4 s = 64,000` samples: window 0 = samples `[0, 80000)`, window 1 = `[64000, 144000)` — both "ready" after 144,000 samples; window 2 would need start `128000` + `80000` = `208000 > 144000`, so it is not ready and `end()`'s flush finalizes the held tail. AVAudioEngine typically taps at 48 kHz, so the Swift shell must downsample 48k→16k (factor 3) before `push_pcm`; a 4096-frame 48 kHz tap buffer (85.33 ms) becomes 1365 samples at 16 kHz. This math is for the Phase-4 Bellows lane; it lives here because it validates the copied contract.

---

## Task 3: CI — hermetic tier + secret-scan

**Files:**
- Create: `.github/workflows/ci.yml`

**Interfaces:**
- Produces: a required status check that runs on every push/PR: fmt, clippy, `cargo test --workspace` (hermetic — no `whisper`, no `goose`, no real-API), and a secret-scan grep.

- [ ] **Step 1: Write `ci.yml`** (uses the Nix devshell so CI matches local exactly)

```yaml
name: ci
on:
  push: { branches: [main] }
  pull_request:
jobs:
  hermetic:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: DeterminateSystems/nix-installer-action@main
      - uses: DeterminateSystems/magic-nix-cache-action@main
      - name: secret-scan (fail closed)
        run: |
          if git grep -nEi 'sk-ant-[A-Za-z0-9]|ANTHROPIC_API_KEY=[^\s]|BEGIN [A-Z ]*PRIVATE KEY' -- . ':!*.md'; then
            echo "secret-like content found"; exit 1; fi
      - name: fmt
        run: nix develop -c cargo fmt --all -- --check
      - name: clippy (hermetic features only)
        run: nix develop -c cargo clippy --workspace --all-targets -- -D warnings
      - name: test (hermetic)
        run: nix develop -c cargo test --workspace
```

- [ ] **Step 2: Verify locally that the hermetic commands the CI runs are green**

Run: `cd ~/athanor-app && nix develop -c cargo fmt --all -- --check && nix develop -c cargo clippy --workspace --all-targets -- -D warnings && nix develop -c cargo test --workspace`
Expected: all PASS.

- [ ] **Step 3: Commit and confirm the check runs green on the remote**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: hermetic tier (fmt, clippy, test) + fail-closed secret scan"
git push
gh run watch --exit-status
```
Expected: the run finishes green.

**Do NOT touch:** never add `--features whisper`, `--features goose`, or any real-API env to CI. Those tiers run gated, locally/on-demand, and their reports are checked in as files — CI stays hermetic.

---

# PHASE 1 — The Two Spike Gates (before ANY product code)

> Both spikes live in `spikes/` (excluded from the workspace) so their heavy/native deps never touch the hermetic tier. Each spike produces a dated report in `docs/research/` with an explicit **GREEN/RED verdict** and, if RED, commits the pre-decided fallback in writing. **Neither spike's outcome blocks Phase 2's hermetic skeleton** — Phase 2 is written against a mock engine and a mock decoder regardless. What the spikes decide is *which real backend* Phase 4 wires in. Sequencing note: Task 4 (Goose) and Task 5 (Whisper) are independent and may run in parallel.

## Task 4: Spike 1a — Goose rlib on iOS (cross-compile + tool-call round-trip)

**Files:**
- Create: `spikes/goose-ios/Cargo.toml`, `spikes/goose-ios/src/main.rs` (host round-trip), `spikes/goose-ios/tests/roundtrip.rs`, `spikes/goose-ios/README.md`
- Create: `docs/research/2026-07-0X-goose-ios-spike.md` (the verdict)
- Update: `~/athanor/projects/athanor-app/STATE.md` (record the pinned tag)

**Interfaces:**
- Produces: a pinned Goose tag + a confirmed (or refuted) in-process embed path. Every fact learned resolves one or more VERIFY-AT-SPIKE items and is written into the report; Phase 2's `engine/goose.rs` (Task 8) consumes those facts.

**GREEN criteria (all four):**
1. The pinned `goose` rlib builds for the host with the portable feature set (`--no-default-features` + the minimal confirmed set) — no `system-keyring`/`nostr`/`local-inference`.
2. The same feature set **cross-compiles** for `aarch64-apple-ios` (`cargo build --target aarch64-apple-ios`) with only devshell cmake/clang — no host-only C dep leaks in.
3. A host integration test drives **one full round-trip**: construct the in-process agent with an Anthropic provider → send one prompt → receive a streamed response → the model calls one **custom in-process extension tool** → the tool result feeds back → the turn completes. (Runs against the real Anthropic API using a key sourced from `.env`; this test is `#[ignore]` by default so it never runs in the hermetic tier.)
4. The round-trip also runs on the **iOS simulator** (agent can do this alone — `aarch64-apple-ios-sim`).

**Operator gate — device run:** the on-device (`aarch64-apple-ios`, real iPhone) execution is **gudnuf-only**. The agent builds the device artifact, writes the run recipe into `spikes/goose-ios/README.md`, and STOPS. The device pass/fail is appended to the report by the operator.

**RED path (pre-decided, no re-litigation):** if criterion 2 or 3 cannot be met after honest effort, switch to **goosed-on-tailnet**: the same ACP shapes run against `goosed` on nous over the tailnet, phone as ACP/HTTP+SSE client, `athanor-core` exposing its data verbs as a client-supplied MCP server. This is a *deployment* change, not a redesign — Phase 2's ACP seam (Task 8) is identical either way. Record RED in the report and note that Task 8's `engine/goose.rs` becomes `engine/goosed_client.rs` with the same trait.

- [ ] **Step 1: Pin the tag and scaffold the spike crate**

Choose the tag (start from `v1.41.0` unless a newer stable exists at spike time). Write `spikes/goose-ios/Cargo.toml`:
```toml
[package]
name = "goose-ios-spike"
version = "0.1.0"
edition = "2021"
publish = false

[dependencies]
# Pin EXACTLY — record the rev in the report and STATE.md.
goose = { git = "https://github.com/aaif-goose/goose", tag = "v1.41.0", default-features = false, features = ["rustls-tls"] }
# Adjust the feature list to the confirmed minimal portable set (VERIFY #1).
agent-client-protocol = { version = "*", features = ["unstable"] }   # confirm version at spike (VERIFY #6)
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```
(README notes: `goosed`-on-tailnet is the RED fallback.)

- [ ] **Step 2: Prove the host + iOS cross-compile builds (criteria 1 & 2)**

Run:
```
cd ~/athanor-app/spikes/goose-ios
nix develop ~/athanor-app -c cargo build                                   # host
nix develop ~/athanor-app -c cargo build --target aarch64-apple-ios        # device slice
nix develop ~/athanor-app -c cargo build --target aarch64-apple-ios-sim    # simulator slice
```
Expected: all three link. If the iOS targets fail, iterate on the feature set (remove any feature dragging a host-only C dep); if still failing after honest effort → RED, go to the fallback step.

- [ ] **Step 3: Write the round-trip integration test (criterion 3)**

`tests/roundtrip.rs` — `#[ignore]` (real API; sourced key). Because the embed API is unverified, the test is written *against the API the spike discovers*; the report records the real constructor/registration/streaming calls. The test asserts, in order: (a) a streamed text delta arrives; (b) a tool-call for the registered custom tool `echo_probe` arrives with the expected argument; (c) the tool's returned value appears in the final assistant turn. Skeleton:
```rust
#[tokio::test]
#[ignore] // real Anthropic API; run with `--ignored`, key from .env
async fn prompt_stream_toolcall_roundtrip() {
    let key = std::env::var("ANTHROPIC_API_KEY").expect("source .env first");
    // 1. construct in-process agent + Anthropic provider (VERIFY #3)
    // 2. register custom in-process extension exposing tool `echo_probe(text) -> text` (VERIFY #4)
    // 3. send prompt: "Call echo_probe with the text 'salt'. Then tell me what it returned."
    // 4. collect streamed session/update notifications (VERIFY #5)
    // asserts:
    //   assert!(saw_text_delta);
    //   assert_eq!(tool_call_arg, "salt");
    //   assert!(final_text.contains("salt"));
}
```

- [ ] **Step 4: Run the round-trip on host and simulator**

Run: `set -a; . ~/athanor-app/.env; set +a; nix develop ~/athanor-app -c cargo test -p goose-ios-spike --test roundtrip -- --ignored`
Expected: PASS on host. Then build+run on the simulator per `README.md`. Device run → STOP (operator gate).

- [ ] **Step 5: Write the verdict report and pin the tag**

`docs/research/2026-07-0X-goose-ios-spike.md`: GREEN/RED verdict; the confirmed minimal feature set; the real embed/registration/streaming API (resolving VERIFY #1–#8); the pinned tag+rev. Update `~/athanor/projects/athanor-app/STATE.md` with the pinned tag. If RED, write the fallback decision explicitly.

- [ ] **Step 6: Commit (spike stays out of the workspace)**

```bash
cd ~/athanor-app
git add spikes/goose-ios docs/research
git commit -m "spike(goose-ios): <GREEN|RED> — pinned <tag>, embed round-trip verified on host+sim"
```

**Do NOT touch:** the hermetic workspace — this spike never becomes a workspace member. Do NOT commit any API key. Do NOT proceed to Task 8's real embed until this report exists.

---

## Task 5: Spike 1b — Whisper RTF (mac-Metal + simulator pre-check; device operator-gated)

**Files:**
- Create: `spikes/whisper-ios/Cargo.toml`, `spikes/whisper-ios/src/main.rs` (RTF bench harness), `spikes/whisper-ios/README.md`, `spikes/whisper-ios/RESULTS.md`
- Reference (read-only): `~/murmur-rmp/spikes/stt-whisper/RESULTS.md` (Mac tiers already measured), `~/athanor-app/crates/stt` (the pipeline under test)

**Interfaces:**
- Produces: a confirmed default model tier (`ggml-base.en-q5_1`) or a tier-drop / last-resort decision. Consumed by Phase 4 (Bellows model-provisioning) — NOT by the hermetic Phase 2/3.

**GREEN criteria:** on-device (real iPhone) RTF < 1.0 sustained over a 10-minute decode with no thermal kill, using `base.en`. The agent-runnable pre-check (mac-Metal + simulator) confirms the *pipeline* runs and re-measures the Mac numbers as a sanity baseline (RESULTS.md Table 1: `base.en` RTF 0.009, `small.en` 0.021 on M4 Max).

**RED path (pre-decided):** if device RTF ≥ 1.0 or thermal kill → drop tier `base → small`? No — smaller is faster; drop toward `tiny.en` (RTF 0.006 Mac) for speed, trading accuracy; if whisper is genuinely unusable on device at any tier → SFSpeechRecognizer returns as the last-resort Swift-shell fallback (product-spec original). Record which.

**Operator gate — device run:** the iPhone tier is gudnuf-only (needs the physical device; simulator has no Metal/ANE/thermal). The agent runs mac-Metal + simulator, writes the device build recipe into `README.md`, and STOPS.

- [ ] **Step 1: Scaffold the bench crate reusing the copied stt pipeline**

`spikes/whisper-ios/Cargo.toml` depends on the workspace `stt` crate with `features = ["whisper"]` and `hound` for WAV I/O. `src/main.rs` loads a WAV, feeds it through `SttStream::with_model` in 5 s / 1 s windows, and reports RTF = `decode_wall_time / audio_duration` on the *second* decode (first is Metal-shader JIT warm-up, per RESULTS.md).

- [ ] **Step 2: Fetch models (gitignored) and run the mac-Metal baseline**

```bash
cd ~/athanor-app/spikes/whisper-ios
# download ggml-base.en-q5_1.bin (~57MB) + ggml-small.en-q5_1.bin (~182MB) from
# huggingface.co/ggerganov/whisper.cpp into ./models/  (gitignored; ggml-org 401s)
set -a; nix develop ~/athanor-app -c cargo run --release -- ./models/ggml-base.en-q5_1.bin sample.wav
```
Expected: RTF printed, comfortably < 0.5 on Mac (baseline sanity vs RESULTS.md ~0.009). Then repeat on the simulator per README.

- [ ] **Step 3: Write the device recipe and RESULTS scaffold, then STOP at the device line**

`README.md`: the device build recipe (whisper.cpp's bundled `examples/whisper.swiftui`, or the stt xcframework path). `RESULTS.md`: copy the RESULTS.md table shape from the donor; fill the Mac/sim rows; leave the **iPhone tier row PENDING — operator-gated**.

- [ ] **Step 4: Commit**

```bash
cd ~/athanor-app
git add spikes/whisper-ios
git commit -m "spike(whisper-ios): mac-Metal + sim RTF baseline green; device tier pending operator"
```

**Do NOT touch:** the hermetic workspace; never commit model `.bin` blobs (gitignored). Do NOT claim device viability — that row is the operator's to fill.

**Worked example (RTF → real-time headroom):** RTF = decode wall-time ÷ audio duration. On Mac, `base.en` decodes 0.51 s of work for 59.8 s of audio → RTF = 0.51/59.8 = **0.0085 ≈ 0.009**. For streaming, each 5 s window costs `5 × RTF` seconds of decode; the window stride is 4 s (chunk − overlap), so to keep up with real time we need `5·RTF < 4` → `RTF < 0.8`. At a pessimistic 5–10× iPhone slowdown, device RTF ≈ 0.045–0.09, giving per-window decode ≈ 0.23–0.45 s ≪ 4 s — a wide margin. GREEN threshold `RTF < 1.0` is the coarse gate; the streaming-keep-up threshold `RTF < 0.8` is the one that actually matters, and both are expected to pass with large headroom (why the operator device run is expected-pass, not a coin flip).

---

# PHASE 2 — Core Skeleton (hermetic; mock engine + mock decoder)

> Everything here compiles and tests with `cargo test --workspace` — no goose, no whisper, no network. The real engine is behind `#[cfg(feature = "goose")]`; hermetic tests drive a `MockEngine` that replays scripted ACP responses (exactly the `MockProvider` pattern from the rmp harness). Tasks 6→11 are roughly sequential (later tasks consume earlier types) and each is one commit-on-green.

## Task 6: Tria prima Store + migrations (v1 schema)

**Files:**
- Create: `crates/athanor-core/Cargo.toml`, `src/lib.rs`, `src/error.rs`, `src/ids.rs`, `src/domain.rs`, `src/store/mod.rs`, `src/store/migrations.rs`, `src/store/{domains,threads,realizations,tending,profile,traces,kindling,correspondences}.rs`
- Modify: `~/athanor-app/Cargo.toml` (add `crates/athanor-core` member — already listed)
- Reference (read-only): `~/murmur-rmp/crates/murmur-core/src/store/{mod,migrations}.rs`

**Interfaces:**
- Produces: `athanor_core::Store` with `open(path, device_id)`, `open_in_memory(device_id)`, `with_clock(Clock)`, `now()`, and typed CRUD methods used by every later task. Domain types in `domain.rs`. The spiral-invariant enforcement lives in `realizations.rs::fix_salt`.
- Consumes: nothing (foundation).

- [ ] **Step 1: Write `Cargo.toml`, `error.rs`, `ids.rs`, and the `Store` shell**

`Cargo.toml`:
```toml
[package]
name = "athanor-core"
version = "0.1.0"
edition = "2021"

[dependencies]
rusqlite = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
uuid = { workspace = true }
async-trait = { workspace = true }

[features]
default = []
goose = []   # real embed pulled in Task 8; off keeps tests hermetic

[dev-dependencies]
tokio = { workspace = true }
```
`error.rs`: `#[derive(thiserror::Error)] pub enum CoreError { Sqlite(#[from] rusqlite::Error), NotFound(String), Immutable(String), BadState(String), Serde(#[from] serde_json::Error) }`.
`ids.rs`: `pub fn new_id() -> String { uuid::Uuid::now_v7().to_string() }`.
`store/mod.rs`: copy the rmp shell — `Store { conn, device_id, clock }`, `open`/`open_in_memory`/`from_connection` (sets `foreign_keys=ON`, runs `migrations::migrate`), `with_clock`, `now()`, `type Clock = Arc<dyn Fn() -> u64 + Send + Sync>`, `fn system_clock() -> u64` (epoch-seconds). Declare the eight submodules.

- [ ] **Step 2: Write the failing migration test**

`store/migrations.rs` (test first):
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    #[test]
    fn all_tables_exist_after_migrate() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        let count: i64 = conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN
             ('domains','pull_notes','threads','realizations','realization_domains',
              'tending','profile','traces','kindling','correspondences','sessions')",
            [], |r| r.get(0)).unwrap();
        assert_eq!(count, 11, "all v1 tables created");
    }
    #[test]
    fn failed_migration_rolls_back_cleanly() {
        let conn = Connection::open_in_memory().unwrap();
        let broken: &[&str] = &[MIGRATIONS[0], "CREATE TABLE broken (;"];
        assert!(migrate_with(&conn, broken).is_err());
        let v: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
        assert_eq!(v, 1, "v1 committed, broken v2 rolled back");
    }
}
```

- [ ] **Step 3: Run it — verify it fails**

Run: `nix develop -c cargo test -p athanor-core migrations`
Expected: FAIL (no `MIGRATIONS`, `migrate`, `migrate_with` yet).

- [ ] **Step 4: Write `MIGRATIONS` (v1) + the runner**

Copy the rmp `migrate`/`migrate_with` runner verbatim (append-only array, per-entry `BEGIN … PRAGMA user_version=N … COMMIT`, rollback-on-error). `MIGRATIONS[0]` = the v1 schema. All `*_at` columns are unix epoch-seconds; every row carries `created_at, updated_at, device_id`; deletes are tombstones (`deleted_at`) EXCEPT append-only/immutable tables noted below.
```sql
-- sulfur: domains + the desire-notes that seeded them
CREATE TABLE domains (
  id TEXT PRIMARY KEY, name TEXT NOT NULL,
  created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, device_id TEXT NOT NULL, deleted_at INTEGER);
CREATE TABLE pull_notes (
  id TEXT PRIMARY KEY, domain_id TEXT REFERENCES domains(id), text TEXT NOT NULL,
  created_at INTEGER NOT NULL, device_id TEXT NOT NULL);
-- mercury: threads (open questions). state in {volatile,condensing,fixed,evaporated}
CREATE TABLE threads (
  id TEXT PRIMARY KEY, prompt TEXT NOT NULL, domain_id TEXT REFERENCES domains(id),
  state TEXT NOT NULL, born INTEGER NOT NULL, last_worked INTEGER,
  parent_realization_id TEXT REFERENCES realizations(id),
  created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, device_id TEXT NOT NULL, deleted_at INTEGER);
-- salt: realizations (immutable once approved). child_thread_id is the spiral link.
CREATE TABLE realizations (
  id TEXT PRIMARY KEY, text TEXT NOT NULL, date INTEGER NOT NULL,
  thread_id TEXT NOT NULL REFERENCES threads(id), child_thread_id TEXT REFERENCES threads(id),
  created_at INTEGER NOT NULL, device_id TEXT NOT NULL);   -- no updated_at/deleted_at: immutable
CREATE TABLE realization_domains (
  realization_id TEXT NOT NULL REFERENCES realizations(id), domain_id TEXT NOT NULL REFERENCES domains(id),
  PRIMARY KEY (realization_id, domain_id));
-- fire: one row per day tended. append-only; wisdom = count(*)
CREATE TABLE tending (
  day TEXT PRIMARY KEY,           -- 'YYYY-MM-DD' (UTC) — one row per day
  minutes INTEGER NOT NULL, thread_ids TEXT NOT NULL DEFAULT '[]',   -- JSON array
  created_at INTEGER NOT NULL, device_id TEXT NOT NULL);
-- learner profile: one row per section (update_memory maintains these)
CREATE TABLE profile (
  section TEXT PRIMARY KEY,       -- 'domains'|'pulls'|'frictions'|'working_history'|'how_i_learn'
  content TEXT NOT NULL DEFAULT '', updated_at INTEGER NOT NULL);
-- one-line session traces future sessions read
CREATE TABLE traces (
  id TEXT PRIMARY KEY, session_id TEXT NOT NULL REFERENCES sessions(id), text TEXT NOT NULL,
  created_at INTEGER NOT NULL, device_id TEXT NOT NULL);
-- Tabula passage kindling (derived events; first-kindle wins)
CREATE TABLE kindling (
  passage_key TEXT PRIMARY KEY,   -- 'SALT','NIGREDO','SOLVE','CITRINITAS','AZOTH',...
  first_kindled_at INTEGER NOT NULL, source_id TEXT);   -- id of the datum that kindled it
-- Azoth's verb (mask deferred; schema ships now)
CREATE TABLE correspondences (
  id TEXT PRIMARY KEY, domain_a TEXT NOT NULL, domain_b TEXT NOT NULL, note TEXT NOT NULL,
  created_at INTEGER NOT NULL, device_id TEXT NOT NULL);
-- sessions: the dialogue container + its selected mask/mode
CREATE TABLE sessions (
  id TEXT PRIMARY KEY, thread_id TEXT REFERENCES threads(id),
  mask TEXT NOT NULL, mode TEXT NOT NULL, state TEXT NOT NULL,   -- open|closed|abandoned
  transcript TEXT NOT NULL DEFAULT '',
  started_at INTEGER NOT NULL, ended_at INTEGER,
  created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, device_id TEXT NOT NULL);
```
Note: `threads.parent_realization_id` and `realizations.child_thread_id` are a deliberate cycle — SQLite does not enforce FK order within a `CREATE`, and `foreign_keys=ON` checks at row-write time, so insert a thread first (child), then the realization referencing it, then set the child's `parent_realization_id` — all inside one transaction (Task 9).

- [ ] **Step 5: Run — verify migration tests pass**

Run: `nix develop -c cargo test -p athanor-core migrations`
Expected: PASS (11 tables; rollback clean).

- [ ] **Step 6: Write per-table CRUD with tests, one module at a time**

For each of `domains, threads, tending, profile, traces, kindling, correspondences` write the minimal typed methods the later tasks need, each behind a failing-test-first cycle. Required method signatures (later tasks depend on these exact names — this is the Produces contract):
```
// domains.rs
fn upsert_domain(&self, name: &str) -> Result<Domain, CoreError>;      // case-insensitive by name
fn add_pull_note(&self, domain_id: Option<&str>, text: &str) -> Result<PullNote, CoreError>;
fn list_domains(&self) -> Result<Vec<Domain>, CoreError>;
// threads.rs
fn open_thread(&self, prompt: &str, domain_id: Option<&str>, parent_realization_id: Option<&str>) -> Result<Thread, CoreError>;  // state='volatile', born=now
fn set_thread_state(&self, id: &str, state: ThreadState) -> Result<Thread, CoreError>;  // validates transition (Task 7)
fn evaporate_thread(&self, id: &str) -> Result<(), CoreError>;         // state='evaporated'
fn ripe_threads(&self, limit: usize) -> Result<Vec<Thread>, CoreError>; // volatile/condensing, oldest last_worked first
fn get_thread(&self, id: &str) -> Result<Thread, CoreError>;
// tending.rs  (append-only; one row per UTC day)
fn record_tending(&self, day: &str, minutes: u32, thread_ids: &[String]) -> Result<(), CoreError>; // upsert-add minutes for the day
fn wisdom_days(&self) -> Result<u64, CoreError>;                        // count(*) of tending
// profile.rs
fn get_profile_section(&self, section: &str) -> Result<String, CoreError>;  // "" if absent
fn set_profile_section(&self, section: &str, content: &str) -> Result<(), CoreError>;
// traces.rs
fn add_trace(&self, session_id: &str, text: &str) -> Result<(), CoreError>;
fn last_trace(&self) -> Result<Option<String>, CoreError>;
// kindling.rs
fn kindle_passage(&self, passage_key: &str, source_id: Option<&str>) -> Result<bool, CoreError>; // false if already kindled (first-wins)
fn kindled(&self) -> Result<Vec<String>, CoreError>;
// correspondences.rs
fn weave_domains(&self, a: &str, b: &str, note: &str) -> Result<Correspondence, CoreError>;
// sessions.rs (state machine methods land in Task 7; the row CRUD lands here)
fn create_session(&self, thread_id: Option<&str>, mask: &str, mode: &str) -> Result<Session, CoreError>; // state='open'
fn append_transcript(&self, session_id: &str, chunk: &str) -> Result<(), CoreError>;
```
Each method gets: a failing test asserting the row lands / reads back / enforces its rule, a run-to-fail, a minimal impl, a run-to-pass. Use the rmp `row_from` + `const *_COLS` pattern.

- [ ] **Step 7: Commit**

```bash
git add crates/athanor-core Cargo.toml
git commit -m "core: tria prima store + v1 migrations + typed CRUD (hermetic)"
```

**Do NOT touch:** the `realizations` immutability and spiral enforcement — those land in Task 9 (`fix_salt`), which is the only writer of `realizations`. Never add `updated_at`/`deleted_at` to `realizations` or `tending`.

**Worked example (migration runner arithmetic):** `migrate_with` reads `user_version` (starts 0), then `.iter().enumerate().skip(0)` runs entry 0, wrapping it as `BEGIN; <sql>; PRAGMA user_version = 1; COMMIT;` (the `i+1` = 0+1). A later v2 append would be entry index 1, skipped when `user_version ≥ 2`, wrapped with `user_version = 2`. So a fresh DB runs `[v1]` → version 1; a v1 DB with a new v2 appended runs only `[v2]` (skip 1) → version 2. The rollback test proves a broken entry N leaves `user_version = N−1` and no partial tables.

---

## Task 7: Session state machine (thread lifecycle + tending/wisdom)

**Files:**
- Create: `crates/athanor-core/src/session.rs`
- Modify: `crates/athanor-core/src/store/threads.rs` (transition validation), `src/store/sessions.rs` (close/abandon)
- Test: inline `#[cfg(test)]` in `session.rs`

**Interfaces:**
- Consumes: `Store` CRUD from Task 6.
- Produces: `ThreadState` transition rules; `Session::close(minutes)`, `Session::abandon()`; the invariant that closing a session records exactly one `tending` day-row and increments `wisdom_days` only on the first session of a new day.

- [ ] **Step 1: Write the failing transition test**

```rust
#[test]
fn thread_transitions_are_constrained() {
    // volatile -> condensing -> fixed  (legal); fixed -> * illegal; * -> evaporated legal
    assert!(ThreadState::Volatile.can_transition_to(ThreadState::Condensing));
    assert!(ThreadState::Condensing.can_transition_to(ThreadState::Fixed));
    assert!(!ThreadState::Fixed.can_transition_to(ThreadState::Volatile));
    assert!(ThreadState::Condensing.can_transition_to(ThreadState::Volatile)); // refusal returns it
    assert!(ThreadState::Volatile.can_transition_to(ThreadState::Evaporated));
}
#[test]
fn closing_first_session_of_day_adds_one_tending_row_and_bumps_wisdom() {
    let store = Store::open_in_memory("dev").unwrap().with_clock(fixed_day("2026-07-06", 0));
    let s = store.create_session(None, "philosophus", "explain").unwrap();
    assert_eq!(store.wisdom_days().unwrap(), 0);
    close_session(&store, &s.id, 7 /*minutes*/, &[]).unwrap();
    assert_eq!(store.wisdom_days().unwrap(), 1);
    // a SECOND session same day adds minutes but NOT a new wisdom day
    let s2 = store.create_session(None, "adamas", "challenge").unwrap();
    close_session(&store, &s2.id, 5, &[]).unwrap();
    assert_eq!(store.wisdom_days().unwrap(), 1, "wisdom counts days, not sessions");
}
```

- [ ] **Step 2: Run — verify it fails.** Run: `nix develop -c cargo test -p athanor-core session`. Expected: FAIL.

- [ ] **Step 3: Implement `ThreadState`, `can_transition_to`, `close_session`, `abandon_session`**

`ThreadState` enum with `parse`/`as_str`; `can_transition_to` encodes the DAG above. `set_thread_state` (Task 6) calls `can_transition_to` and returns `CoreError::BadState` on an illegal move. `close_session(store, id, minutes, thread_ids)`: in one transaction — set `sessions.state='closed'`, `ended_at=now`; `record_tending(today_utc, minutes, thread_ids)` (upsert-add minutes; `record_tending` is idempotent per day so wisdom = `count(*)` counts days). `abandon_session`: state='abandoned', return the session's thread (if any) to `volatile` (per product spec error-handling: an interrupted session returns its thread to volatile).

- [ ] **Step 4: Run — verify pass.** Run: `nix develop -c cargo test -p athanor-core session`. Expected: PASS.

- [ ] **Step 5: Commit.**
```bash
git add crates/athanor-core/src/session.rs crates/athanor-core/src/store
git commit -m "core: session state machine + thread-lifecycle DAG + tending/wisdom accounting"
```

**Do NOT touch:** `fix_salt` (Task 9). `record_tending` must stay append-only-per-day (never delete a day row).

**Worked example (wisdom vs sessions):** `wisdom_days = count(*) FROM tending`. Day 2026-07-06 with two sessions (7 min, 5 min): `record_tending('2026-07-06', 7, …)` inserts the row; `record_tending('2026-07-06', 5, …)` hits the `day` PK and does `minutes = minutes + 5` (→ 12) instead of inserting. So `count(*) = 1` day, `minutes = 12`. Next calendar day's first close inserts a second row → wisdom = 2. Wisdom never decreases and counts *days shown up*, exactly the product spec's single long-term score.

---

## Task 8: ACP-typed engine seam + MockEngine (hermetic) + real embed (gated)

**Files:**
- Create: `crates/athanor-core/src/engine/mod.rs`, `src/engine/acp.rs`, `src/engine/mock.rs`, `src/engine/goose.rs`
- Test: inline in `mock.rs`; a gated `#[ignore]` test in `goose.rs`
- Reference: the Task 4 spike report (the real embed API)

**Interfaces:**
- Produces: `trait MystagogueEngine` — the ONE seam every caller uses; `MockEngine` (hermetic, scripted); `GooseEngine` (`#[cfg(feature="goose")]`). ACP-shaped structs in `acp.rs` (`AcpPrompt`, `AcpUpdate`, `AcpToolCall`, `AcpToolResult`) that survive engine churn.
- Consumes: nothing at the type level; the real impl consumes the pinned goose crate.

- [ ] **Step 1: Define the ACP-shaped seam types (`acp.rs`)**

Our own structs (NOT the goose crate's) so engine upgrades can't reach our callers. Minimal set:
```rust
pub struct AcpPrompt { pub system: String, pub user_turns: Vec<String>, pub tools: Vec<AcpToolSpec> }
pub struct AcpToolSpec { pub name: String, pub json_schema: serde_json::Value }
pub enum AcpUpdate { TextDelta(String), ToolCall(AcpToolCall), TurnComplete }
pub struct AcpToolCall { pub id: String, pub name: String, pub args: serde_json::Value }
pub struct AcpToolResult { pub id: String, pub value: serde_json::Value }
```
These mirror ACP wire shapes but are ours; `engine/goose.rs` converts to/from the real `agent-client-protocol`/`goose-sdk-types` types at the boundary (VERIFY #6), and NOWHERE else in the codebase names a goose type.

- [ ] **Step 2: Define `trait MystagogueEngine`**
```rust
#[async_trait::async_trait]
pub trait MystagogueEngine: Send + Sync {
    /// Drive one prompt; stream AcpUpdates to `sink`; resolve tool calls via `tools`.
    async fn run_turn(
        &self,
        prompt: AcpPrompt,
        tools: &dyn ToolDispatch,
        sink: &mut dyn FnMut(AcpUpdate),
    ) -> Result<(), EngineError>;
}
#[async_trait::async_trait]
pub trait ToolDispatch: Send + Sync {
    async fn dispatch(&self, call: AcpToolCall) -> AcpToolResult;   // Task 9 impls this over the Store
}
```

- [ ] **Step 3: Write the failing MockEngine test**
```rust
#[tokio::test]
async fn mock_engine_streams_then_dispatches_a_tool_then_completes() {
    let engine = MockEngine::new(vec![
        AcpUpdate::TextDelta("Consider ".into()),
        AcpUpdate::ToolCall(AcpToolCall { id: "1".into(), name: "open_thread".into(),
            args: serde_json::json!({"question": "why?"}) }),
        AcpUpdate::TurnComplete,
    ]);
    let tools = RecordingDispatch::default();
    let mut got = Vec::new();
    engine.run_turn(demo_prompt(), &tools, &mut |u| got.push(format!("{u:?}"))).await.unwrap();
    assert!(got.iter().any(|g| g.contains("TextDelta")));
    assert_eq!(tools.calls(), vec!["open_thread"]);   // tool was dispatched
    assert!(got.iter().any(|g| g.contains("TurnComplete")));
}
```

- [ ] **Step 4: Run to fail, then implement `MockEngine`** — replays scripted `AcpUpdate`s in order (VecDeque, the `MockProvider` shape); on each `ToolCall` it calls `tools.dispatch(...)` and continues. Run to pass.

Run (fail then pass): `nix develop -c cargo test -p athanor-core engine`

- [ ] **Step 5: Stub `engine/goose.rs` behind the feature (do not implement real calls until the spike is GREEN)**
```rust
#[cfg(feature = "goose")]
pub struct GooseEngine { /* holds the in-process agent from the pinned tag */ }
#[cfg(feature = "goose")]
#[async_trait::async_trait]
impl MystagogueEngine for GooseEngine { /* convert AcpPrompt<->goose types; drive; stream */ }
```
Add a `#[cfg(all(feature="goose", test))] #[ignore]` real-API round-trip mirroring the Task-4 spike, so the embed is exercised behind the gate but never in the hermetic tier. If Spike 1a was RED, this file becomes `engine/goosed_client.rs` implementing the same `MystagogueEngine` trait over ACP/HTTP+SSE to `goosed` on the tailnet — callers are unchanged.

- [ ] **Step 6: Commit.**
```bash
git add crates/athanor-core/src/engine
git commit -m "core: ACP-typed engine seam + hermetic MockEngine + gated goose embed stub"
```

**Do NOT touch:** any goose type outside `engine/goose.rs`. Do NOT enable `goose` in `default` features. The hermetic tier must never compile the goose crate.

---

## Task 9: The Mystagogue extension — six tools over the Store

**Files:**
- Create: `crates/athanor-core/src/extension/mod.rs`
- Modify: `crates/athanor-core/src/store/realizations.rs` (`fix_salt` + spiral enforcement)
- Test: inline in `extension/mod.rs`

**Interfaces:**
- Consumes: `Store` (Task 6/7), `AcpToolCall`/`AcpToolResult`/`ToolDispatch` (Task 8).
- Produces: `Mystagogue` — a `ToolDispatch` impl whose six tools write to the store, and `Mystagogue::tool_specs() -> Vec<AcpToolSpec>` used by prompt assembly (Task 10). The spiral invariant is enforced here: `fix_salt` writes the realization AND its child thread in one transaction.

| Tool | Effect (store method) |
|---|---|
| `fix_salt(realization, thread_id, domains[])` | `Store::fix_salt` — write immutable realization + auto-create child thread; kindle SALT |
| `open_thread(question, domain?)` | `Store::open_thread` (state volatile) |
| `evaporate_thread(id)` | `Store::evaporate_thread` |
| `kindle_passage(term)` | `Store::kindle_passage` |
| `weave_domains(a, b, note)` | `Store::weave_domains`; kindle CITRINITAS+AZOTH |
| `update_memory(section, content)` | `Store::set_profile_section` |

- [ ] **Step 1: Write the failing spiral-invariant test** (the load-bearing one)
```rust
#[tokio::test]
async fn fix_salt_writes_immutable_realization_and_births_child_thread() {
    let store = Store::open_in_memory("dev").unwrap();
    let d = store.upsert_domain("thermodynamics").unwrap();
    let parent = store.open_thread("what is entropy?", Some(&d.id), None).unwrap();
    let myst = Mystagogue::new(store_arc(store));
    let res = myst.dispatch(AcpToolCall{ id:"1".into(), name:"fix_salt".into(),
        args: serde_json::json!({"realization":"entropy is lost ways-to-not-know",
                                 "thread_id": parent.id, "domains":["thermodynamics"]}) }).await;
    let rid = res.value["realization_id"].as_str().unwrap().to_string();
    // spiral: the realization has a child thread, and it exists and is volatile
    let child = myst.store().realization_child_thread(&rid).unwrap();
    assert_eq!(child.state, ThreadState::Volatile);
    assert_eq!(child.parent_realization_id.as_deref(), Some(rid.as_str()));
    // immutability: no update path exists / second write of same id rejected
    assert!(myst.store().try_mutate_realization(&rid, "tampered").is_err());
    // SALT passage kindled
    assert!(myst.store().kindled().unwrap().contains(&"SALT".to_string()));
}
```

- [ ] **Step 2: Run to fail.** Run: `nix develop -c cargo test -p athanor-core extension`. Expected: FAIL.

- [ ] **Step 3: Implement `Store::fix_salt` (the transaction) and the `Mystagogue` dispatcher**

`fix_salt(thread_id, text, domains, child_question)`: one `unchecked_transaction` — (a) insert the immutable `realizations` row; (b) `open_thread(child_question or a default "what does this open?", domain, parent_realization_id=rid)` to birth the child; (c) `UPDATE realizations SET child_thread_id=? WHERE id=?`; (d) `set_thread_state(parent, Fixed)`; (e) `kindle_passage("SALT", Some(rid))`; commit. If the model called `fix_salt` without a following child question, synthesize the child thread anyway (the invariant is structural, not optional). `Mystagogue::dispatch` matches on `call.name`, deserializes args, calls the store method, returns `AcpToolResult` with the new id(s). `realization_child_thread`, `try_mutate_realization` (always `Err(Immutable)`) are thin store helpers.

- [ ] **Step 4: Run to pass.** Then add one test per remaining tool (`open_thread`, `evaporate_thread`, `kindle_passage`, `weave_domains`, `update_memory`) — each a fail→impl→pass cycle asserting the store row lands. Run: `nix develop -c cargo test -p athanor-core extension`. Expected: all PASS.

- [ ] **Step 5: Commit.**
```bash
git add crates/athanor-core/src/extension crates/athanor-core/src/store/realizations.rs
git commit -m "core: Mystagogue extension — six tools; fix_salt enforces the spiral in one txn"
```

**Do NOT touch:** the streaming/engine loop (Task 8 owns it). `fix_salt` is the ONLY writer of `realizations`; there is no update/delete path — immutability is enforced by absence plus `try_mutate_realization` returning `Err`.

---

## Task 10: Prompt assembly (profile + ripe mercury + mode + mask)

**Files:**
- Create: `crates/athanor-core/src/prompt/mod.rs`, `src/prompt/assets.rs`, and the `crates/athanor-core/prompts/` asset dir (skeleton files — content is the separate prompt-smith lane; here we ship legible placeholders + the loader).
- Test: inline snapshot tests in `prompt/mod.rs`

**Interfaces:**
- Consumes: `Store` (profile, ripe threads, last trace), `Mystagogue::tool_specs()` (Task 9).
- Produces: `assemble_system_prompt(store, mask, mode, thread) -> String` and `SessionPlan { mask, mode, thread_id, system_prompt }`. Deterministic given the same store state + assets — so snapshot tests are stable.

- [ ] **Step 1: Create the prompts asset skeleton**

`prompts/identity.md`, `prompts/masks/{philosophus,adamas,solve}.md`, `prompts/modes/{trace,explain,predict,challenge,design}.md`, `prompts/condensation.md`, `prompts/initiation.md`, `prompts/judge.md`. Each holds a short legible placeholder marked `<!-- PROMPT-SMITH LANE: content iterates under evals -->` plus the one structural rule the eval harness checks (e.g. philosophus.md: "Ask only. Emit no declarative that asserts a domain fact."). `assets.rs` loads them with `include_str!` (compiled in — no runtime file IO on device) keyed by name.

- [ ] **Step 2: Write the failing snapshot test**
```rust
#[test]
fn assembled_prompt_is_deterministic_and_layers_all_parts() {
    let store = seeded_store();  // 1 domain, 1 ripe thread, profile.how_i_learn set, last trace set
    let plan = assemble("philosophus", "explain", Some(&thread_id(&store)), &store);
    let p = plan.system_prompt;
    assert!(p.contains("Ask only"));                 // mask asset present
    assert!(p.contains("explain"));                  // mode present
    assert!(p.contains("how_i_learn: dialogue"));    // profile injected
    assert!(p.contains("Ripe mercury:"));            // ripe thread injected
    assert!(p.contains("Last time:"));               // trace injected
    // deterministic: two assemblies of the same state are byte-identical
    assert_eq!(p, assemble("philosophus","explain",Some(&thread_id(&store)),&store).system_prompt);
}
```

- [ ] **Step 3: Run to fail, then implement `assemble`** — concatenate, in a fixed order: identity → mask asset → mode asset → `profile` sections (sorted by section key for determinism) → `ripe_threads(3)` rendered as "Ripe mercury: …" → `last_trace()` as "Last time: …" → the six tool specs' names as an availability line. No timestamps in the string (determinism). Run to pass.

- [ ] **Step 4: Commit.**
```bash
git add crates/athanor-core/src/prompt crates/athanor-core/prompts
git commit -m "core: prompt assembly (deterministic, layered) + versioned asset skeleton"
```

**Do NOT touch:** prompt *content* quality — that is the prompt-smith lane. Here we ship the loader, the layering order, and legible placeholders only. Keep `assemble` free of `now()`/random so snapshots stay stable.

---

## Task 11: Thin dev CLI over athanor-core

**Files:**
- Create: `crates/athanor-cli/Cargo.toml`, `src/main.rs`
- Modify: workspace members (already listed)

**Interfaces:**
- Consumes: all of `athanor-core` (Store, Mystagogue, assemble, MockEngine or, with `--features goose`, GooseEngine).
- Produces: a desktop binary for prompt iteration: `athanor-cli session --mask philosophus --mode explain [--goose]`. Default (hermetic) uses MockEngine reading a scripted JSON; `--features goose` drives the real engine against `ANTHROPIC_API_KEY` from the environment.

- [ ] **Step 1: Write `Cargo.toml`** — depends on `athanor-core` (with an optional `goose` feature forwarding to `athanor-core/goose`), `tokio`, `serde_json`. Default features hermetic.

- [ ] **Step 2: Write a smoke test (integration) that runs a scripted session end-to-end hermetically**
```rust
// tests/cli_smoke.rs — drives main's session runner with MockEngine + a scripted turn
#[tokio::test]
async fn scripted_session_fixes_salt_and_births_child_thread() {
    let store = Store::open_in_memory("dev").unwrap();
    // script: assistant asks, learner condenses, engine calls fix_salt
    run_scripted_session(&store, "philosophus", "explain", scripted_updates()).await.unwrap();
    assert_eq!(store.wisdom_days().unwrap(), 1);          // session closed -> a day tended
    assert_eq!(count_realizations(&store), 1);
    assert_eq!(count_threads_with_parent(&store), 1);     // the spiral child thread exists
}
```

- [ ] **Step 3: Run to fail, implement the session runner + `main` arg parsing, run to pass.** The runner: `create_session` → `assemble` the system prompt → `engine.run_turn(prompt, &mystagogue, &mut sink)` printing `TextDelta`s to stdout → on `TurnComplete` close the session (record minutes). Keep all logic in `athanor-core`; `main.rs` is glue only (the no-logic-in-shell invariant applies to the CLI too).

Run: `nix develop -c cargo test -p athanor-cli`

- [ ] **Step 4: Commit.**
```bash
git add crates/athanor-cli
git commit -m "cli: thin dev harness over athanor-core (hermetic MockEngine; goose behind a feature)"
```

**Do NOT touch:** business logic must not migrate into `main.rs`. The CLI is a shell like SwiftUI — it renders and dispatches only.

---

# PHASE 3 — Prompt Pack + Eval Harness Scaffolding

> The `evals` crate borrows the *shape* of the rmp evals machinery (deterministic graders, timestamp-free comparable JSON reports, hermetic + gated tiers) without depending on sitewalk. Prompt CONTENT iteration is a separate lane — here we build the harness, the personas skeleton, and the initial pack structure, and wire the hermetic tier into CI.

## Task 12: `evals` crate scaffold + normalize + report shape

**Files:**
- Create: `crates/evals/Cargo.toml`, `src/lib.rs`, `src/normalize.rs`, `src/report.rs`
- Reference (read-only): `~/murmur-rmp/crates/evals/src/{normalize,report}.rs`

**Interfaces:**
- Produces: `evals::normalize::{token_set, dice}` (copied — used by salt-refusal grading), `evals::report::{SuiteReport, ScenarioReport, render_table}` (timestamp-free JSON + human table).
- Consumes: `athanor-core` public API (personas replay through the engine seam).

- [ ] **Step 1: Write `Cargo.toml`** (dev-side; `publish = false`; depends on `athanor-core`, `serde`, `serde_json`; dev-dep `tokio`).

- [ ] **Step 2: Copy `normalize.rs` verbatim** from the rmp evals crate (the `token_set`/`dice` pair, with its unit tests). Run `nix develop -c cargo test -p evals normalize` → PASS.

- [ ] **Step 3: Write `report.rs`** — a `SuiteReport { pack_version, scenarios: Vec<ScenarioReport>, aggregate }` where each `ScenarioReport { id, checks: Vec<CheckResult>, passed: bool }` and `CheckResult { name, passed, detail }`. Serialize to pretty JSON with NO timestamps (comparability across runs, exactly the rmp property). Include `render_table`. TDD: one test asserts `serde_json` round-trips and the JSON contains `pack_version` but no time field.

- [ ] **Step 4: Commit.**
```bash
git add crates/evals
git commit -m "evals: crate scaffold + copied normalize + timestamp-free report shape"
```

**Do NOT touch:** any dependency on the sitewalk/murmur repo — copy the shape, never `path = "../../murmur-rmp/..."`.

---

## Task 13: Deterministic graders (spiral, salt-refusal, mask fidelity)

**Files:**
- Create: `crates/evals/src/grade.rs`
- Test: inline in `grade.rs`

**Interfaces:**
- Consumes: `normalize::dice` (Task 12), the tool-call trace of a scripted session.
- Produces: `grade_spiral`, `grade_salt_refusal`, `grade_mask_fidelity`, each `(&SessionTrace) -> CheckResult`. A `SessionTrace { turns: Vec<Turn> }` where `Turn = Assistant(String) | ToolCall{name,args} | LearnerText(String)`.

- [ ] **Step 1: Write the failing grader tests**
```rust
#[test]
fn spiral_check_requires_open_thread_after_every_fix_salt() {
    let ok = trace(&[tool("fix_salt","A"), tool("open_thread","q1"),
                     tool("fix_salt","B"), tool("open_thread","q2")]);
    assert!(grade_spiral(&ok).passed);
    let bad = trace(&[tool("fix_salt","A"), tool("fix_salt","B"), tool("open_thread","q1")]);
    assert!(!grade_spiral(&bad).passed, "fix_salt A had no open_thread before the next fix_salt");
}
#[test]
fn salt_refusal_rejects_parroted_condensation() {
    // learner echoes the assistant's phrasing (dice >= 0.7) then a fix_salt fires -> FAIL
    let parrot = trace(&[assistant("entropy is disorder"),
                         learner("entropy is disorder"),
                         tool("fix_salt","entropy is disorder")]);
    assert!(!grade_salt_refusal(&parrot).passed);
    // learner's OWN words (low overlap) -> fix_salt allowed -> PASS
    let own = trace(&[assistant("entropy is disorder"),
                      learner("its the count of ways i cant tell apart"),
                      tool("fix_salt","ways i cant tell apart")]);
    assert!(grade_salt_refusal(&own).passed);
}
#[test]
fn philosophus_mask_emits_no_declaratives() {
    let ok = trace_mask("philosophus", &[assistant("what would happen if you doubled it?")]);
    assert!(grade_mask_fidelity(&ok).passed);
    let bad = trace_mask("philosophus", &[assistant("Entropy always increases.")]);
    assert!(!grade_mask_fidelity(&bad).passed, "a bare declarative in Philosophus is a violation");
}
```

- [ ] **Step 2: Run to fail, then implement the graders.**
- `grade_spiral`: scan turns; for each `fix_salt` at index i, require an `open_thread` after i and before the next `fix_salt`. (Structural — deterministic; the machine-checkable half of the spiral invariant.)
- `grade_salt_refusal`: for each `fix_salt(text)`, find the nearest preceding `LearnerText`; if that learner text has `dice(token_set(learner), token_set(nearest_prior_assistant)) >= 0.7`, the salt is parroted → the check FAILS unless the engine refused (no realization committed). Use the Task-12 `dice`.
- `grade_mask_fidelity` (Philosophus, hermetic proxy): every `Assistant` turn under mask `philosophus` must contain a `?` and must not contain a sentence with no `?` that reads as a declarative (proxy: split on sentence punctuation; any non-empty segment ending in `.` and not containing `?` fails). This is a coarse structural proxy; deeper fidelity is the gated LLM-judge tier (Task 15). Document this scoping in the module docs.

Run: `nix develop -c cargo test -p evals grade`. Expected: PASS.

- [ ] **Step 3: Commit.**
```bash
git add crates/evals/src/grade.rs
git commit -m "evals: deterministic graders — spiral, salt-refusal (dice), Philosophus mask proxy"
```

**Do NOT touch:** the LLM-judge (gated tier, Task 15). Keep every grader here pure and deterministic — no engine, no network, no clock.

**Worked example (salt-refusal via Dice):** `token_set("entropy is disorder") = {entropy,is,disorder}` (3 tokens). Learner parrots "entropy is disorder" → identical set → `dice = 2·|A∩B| / (|A|+|B|) = 2·3 / (3+3) = 1.0 ≥ 0.7` → parroted → must refuse. Own words "ways i cant tell apart" vs assistant "entropy is disorder": intersection `{}` → `dice = 0 / 6 = 0.0 < 0.7` → genuine salt → allowed. Threshold 0.7 (higher than the grader's item-match 0.5) is deliberately strict: we only flag *near-verbatim* echoes as parroting, so paraphrase in the learner's own frame still passes.

**Worked example (spiral scan):** trace `[fix_salt A, fix_salt B, open_thread q1]`. Iterate: `fix_salt A` at i=0 → look for `open_thread` in indices (0, next_fix=1) → none → violation recorded → `passed=false`. Correct trace `[fix_salt A, open_thread q1, fix_salt B, open_thread q2]`: `fix_salt A` (i=0)→ open_thread at i=1 before next_fix=2 ✓; `fix_salt B` (i=2)→ open_thread at i=3 ✓ → `passed=true`.

---

## Task 14: Prompt-pack skeleton + assembled-prompt snapshot tests

**Files:**
- Modify: `crates/athanor-core/prompts/*` (fill the skeleton with the structural rules each grader/asset needs — still placeholder *content*, real *structure*)
- Create: `crates/evals/src/snapshot.rs`, `crates/evals/snapshots/` (checked-in expected assembled prompts)
- Test: inline in `snapshot.rs`

**Interfaces:**
- Consumes: `athanor_core::prompt::assemble` (Task 10).
- Produces: snapshot tests asserting the assembled prompt for each `(profile, thread, mode, mask)` combination matches a checked-in expected file — so any prompt-assembly change is visible in a diff (product-spec testing requirement).

- [ ] **Step 1: Enumerate the snapshot matrix** — the 3 v1 masks × 5 modes against a fixed seeded store fixture (one domain, one ripe thread, a fixed profile), plus the initiation prompt (cold-start, empty store). 3×5 + 1 = **16 snapshots**.

- [ ] **Step 2: Write the failing snapshot test** that, for each combination, calls `assemble` and compares to `snapshots/<mask>__<mode>.txt` (and `snapshots/initiation.txt`), with an `UPDATE_SNAPSHOTS=1` env to regenerate.

- [ ] **Step 3: Run to fail (no snapshot files), generate them once with `UPDATE_SNAPSHOTS=1`, eyeball each for sanity, run again to pass.**

Run: `nix develop -c cargo test -p evals snapshot` (after `UPDATE_SNAPSHOTS=1 nix develop -c cargo test -p evals snapshot` to seed). Expected: PASS, 16 snapshots checked in.

- [ ] **Step 4: Commit.**
```bash
git add crates/evals/src/snapshot.rs crates/evals/snapshots crates/athanor-core/prompts
git commit -m "evals: 16 assembled-prompt snapshots (3 masks x 5 modes + initiation)"
```

**Do NOT touch:** prompt content polish (prompt-smith lane). Snapshots lock *structure/assembly*, not literary quality.

**Worked example (matrix count):** masks {philosophus, adamas, solve} = 3; modes {trace, explain, predict, challenge, design} = 5; composable → 3 × 5 = 15 session prompts. Initiation is a separate cold-start prompt (empty store, no mask/mode selection yet) → +1 = 16 checked-in snapshot files.

---

## Task 15: Scripted personas + hermetic runner wired into CI

**Files:**
- Create: `crates/evals/src/personas.rs`, `crates/evals/src/run.rs`, `crates/evals/examples/eval.rs`, `crates/evals/fixtures/*.json`
- Modify: `.github/workflows/ci.yml` (add the hermetic eval run)

**Interfaces:**
- Consumes: `MockEngine` (Task 8), `Mystagogue` (Task 9), the graders (Task 13), the report (Task 12).
- Produces: four scripted personas replayed through the MockEngine, each graded, emitting a `SuiteReport` JSON; the hermetic subset runs in CI on every change. The gated real-API tier (LLM-judge) is scaffolded but `#[ignore]`.

- [ ] **Step 1: Encode the four personas as scripted `MockEngine` update-scripts + fixture JSON**
- **eager parroter** — condenses back the assistant's phrasing (tests salt-refusal grader).
- **the stuck one** — repeated "I don't know" (tests Solve's entrance; hermetic checks the mask/mode selection, not LLM quality).
- **tangent-chaser** — opens many threads without fixing salt (tests thread discipline).
- **the silent one** — empty/minimal turns (tests pacing/patience — hermetic checks the session still lands/closes).

- [ ] **Step 2: Write the failing runner test** — `run_suite(personas) -> SuiteReport` where each persona's scripted session is driven through `MockEngine` + `Mystagogue` over an in-memory store, then graded by all applicable Task-13 graders; assert the parroter FAILS salt-refusal and the well-behaved control PASSES the spiral check.

- [ ] **Step 3: Run to fail, implement `run.rs` + `personas.rs`, run to pass.** `examples/eval.rs` prints `render_table` and writes `report.json`.

Run: `nix develop -c cargo test -p evals` then `nix develop -c cargo run -p evals --example eval`

- [ ] **Step 4: Add the hermetic eval step to CI**
```yaml
      - name: evals (hermetic)
        run: nix develop -c cargo test -p evals
```
Scaffold (but leave `#[ignore]`) the gated real-API LLM-judge runner in `run.rs` behind an env check — it reads `ANTHROPIC_API_KEY`, replays personas through the real engine, and asks `prompts/judge.md`; its report lands in `docs/research/` when run on demand, never in CI.

- [ ] **Step 5: Commit and confirm CI green.**
```bash
git add crates/evals .github/workflows/ci.yml
git commit -m "evals: four scripted personas + hermetic runner in CI; gated LLM-judge scaffolded"
git push && gh run watch --exit-status
```

**Do NOT touch:** the gated tier must never run in CI (no key in CI; determinism only). Persona LLM-judgment is on-demand and its reports are checked-in files.

---

## What Comes Next (Phases 4–5 — NOT planned in task detail here)

- **Phase 4 — First shippable, voice-first.** Fill the `apps/ios` SwiftUI shell (Furnace, Initiation, Session, Grimoire, Mercury). Wire the `ffi` crate (UniFFI) exposing the engine + store verbs; stream `AcpUpdate` → UniFFI callback → SwiftUI token-by-token (the rmp `WalkEventListener` `with_foreign` pattern). Build the Bellows: Swift `AVAudioEngine` captures PCM → downsample 48k→16k → `stt::SttStream::push_pcm/poll/end`; add the two named deltas vs the copied stt (energy/VAD endpointing to auto-send; live-partial mercury-shimmer rendering). Shell downloads `ggml-base.en-q5_1` on first launch; API key in Keychain. The real `GooseEngine` (or `goosed_client`) turns on here per the Spike-1a verdict.
- **Phase 5 — Daily tending.** gudnuf dogfoods; vault export (markdown + YAML) and the Tabula surface land while real sessions accumulate; frictions feed the meta-process (mercury → salt in `docs/grimoire.md`). The gated LLM-judge is spot-checked against real session traces.

---

## Self-Review Notes (author's pass against both specs)

- **Product-spec coverage:** tria prima data model → Task 6; spiral invariant → Tasks 9 & 13; session/masks/modes → Tasks 7/10; condensation refusal → Task 13; Tabula kindling → Task 6/9; vault export + screens → deferred to Phase 4 (out of scope here, flagged above). Error-handling (session interrupt → thread back to volatile) → Task 7 `abandon_session`.
- **Goose-spec coverage:** embedded Goose behind ACP seams → Tasks 8; Mystagogue six-tool extension → Task 9; masks/modes/prompts as assets → Tasks 10/14; spike gates with pre-decided red paths → Tasks 4/5; eval harness (hermetic + gated, deterministic graders, timestamp-free reports, personas, snapshots, judge-as-prompt) → Tasks 12–15; dev CLI → Task 11.
- **Underspecified / decided-in-plan:** (a) exact tria prima SQL schema — designed here, following the rmp migration pattern; (b) Philosophus mask-fidelity as a *coarse deterministic proxy* in the hermetic tier with deep fidelity deferred to the gated LLM-judge — a scoping decision the specs left open; (c) the `threads`↔`realizations` FK cycle resolved by insert-order-within-one-transaction; (d) child-thread synthesis when the model omits the child question (invariant made structural, not model-dependent).
