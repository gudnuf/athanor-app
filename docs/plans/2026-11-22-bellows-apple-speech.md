# Bellows on Apple Speech: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task by task. Steps use checkbox (`- [ ]`) syntax for tracking. Write each test first, watch it fail, then implement.

## Revision 2026-11-23 (after `2026-11-22-bellows-apple-speech.REVIEW.md`)

**Blockers:**
- **B1:** a final or error on the *Active* request now commits, closes and cycles without ending the turn (new turn tests 12 and 13).
- **B2:** the committer's reset rule now:
  - flushes the held-back tail;
  - detects a reset by "hypothesis shrank and no committed slot matches" (this also works with 1 committed word);
  - de-duplicates by suffix/prefix overlap, so a final that drops filler words is not re-emitted.

  Committer tests went from 6 to 8.
- **B3:** the snapshot fields, the overlay's engine chip row and the `gestures.md` triple-tap row move into Task 6, so Tasks 6 and 7 build as written. Task 9 shrinks to the Apple rows.

**Should-fix items:**
- **S1:** `Endpointer::in_utterance()` (any non-Idle state). Soft rotation only fires when Idle, pickup is armed only on the Idle→non-Idle edge, and new turn test 15 covers this.
- **S2:** a turn end with no Active request marks the newest open request (test 14).
- **S3:** a separate control queue, and preview computed at drain time.
- **S4:** Swift rotation swaps the new request in *before* calling `endAudio` on the old one, and keeps the (task, request) pair.
- **S5:** the gaps in Task 7 are spelled out.
- **S6:** fixtures use the filter helper consistently.
- **S7:** the clippy dead-code trap is covered.
- **S8:** the `verify` signature is spelled out, and the diagnosis is corrected: the SHA-256 runs on main once after each fresh download, not at every launch.
- **S9:** the grep pattern is fixed.
- **S10:** the smoke run uses `screen=session` plus `-athanor.hasCompletedInitiation YES`.
- **S11:** `say` writes the WAV directly, the injector pads 2 s of silence and builds its converter from the file format, and the idb client is planned up front.
- **S12:** curly apostrophes are normalised.
- **S13:** the pass check adds `timeouts == 0` plus "request 1 final arrived", and learner text is logged `.public` only under DEBUG.
- **S14:** concurrent recognition tasks are now an explicit UNVERIFIED dependency, with a fallback (F5).

**Other changes:**
- Every Apple fact I could not verify is marked **UNVERIFIED**, with the device step that settles it.
- Nits N1–N9, N11 and N12 are folded in.
- N10 (the denied-mic wording) is left out of scope.
- Test counts: committer 8, turn 15, `text_bellows` 23, ffi 3.

**Re-review fold-ins (same day, no further review round):**
- **R1:** when `on_final` detects a revision, it now aligns by the overlap `k` instead of by position, so a filler-dropping final that does not shrink keeps every word (committer tests 9 and 10: the R1 case, plus the `k == 0` fallback that keeps a plain slot-0 revision from duplicating).
- **R4:** after 3 consecutive self-closes with no words, Rust stops cycling and emits a terminal `Unavailable`. Swift maps it to `.permissionDenied` (typed fallback) and stops capture (turn test 16).
- **R2 and R3** are documented as trade-offs in Risk 6, and Task 12 records `revisions_dropped` next to `resets`.
- **Nits:** tests 12 and 13 drain after the committing partials; `tier`, `currentMetrics()` and `rms` are `nonisolated`.
- New test counts: committer 10, turn 16, `text_bellows` 26, ffi 3.

---

**Goal:** Make Apple's on-device speech recognition (`SFSpeechRecognizer`, iOS 17 baseline) the default Bellows engine, because the operator found whisper too slow in daily use. Whisper stays selectable from the dev overlay and stays compiled in (`--features goose,whisper`). Its model download becomes lazy, so a first launch on Apple needs no download.

**Architecture (the decision):**
- **Rust owns endpointing and finalization.** A new pure `stt::TextBellows` takes the same 16 kHz PCM the whisper path already builds.
  - It drives the existing, tested `stt::Endpointer` for the silence latch.
  - It takes recognizer text events tagged with a request id.
  - From these it decides: the append-only commit (what cools into the transcript); when to cycle a request (at a turn end, at a rotation, or when the recognizer closes the request itself); and when the utterance has ended.
- **Swift only forwards.** `AppleBellowsController` captures audio, runs the recognizer, forwards PCM and text, and obeys `CycleRequest` events.

*Why Rust, in two sentences:* the turn boundary and the committed text decide what the learner said and when it goes to the Mystagogue, so invariant #1 puts them in Rust. Reusing `stt::Endpointer` gives both engines the same latch semantics (threshold, 800 ms latch, latency metric), and scripted text-plus-PCM tests cover all of it hermetically, which Swift recognizer code cannot.

This refines the 2026-07-04 product spec's sketch ("SFSpeechRecognizer in the Swift shell only; the Rust core sees text and utterance boundaries"). The Rust core still never sees the recognizer. It now *decides* the boundary instead of being told it.

**Tech stack:** Rust (stt + ffi crates, UniFFI 0.31 proc-macro), Swift (language mode 5.9 on the 6.2 toolchain), SwiftUI, the `Speech` framework, AVAudioEngine, xcodegen.

## Global constraints

- **Repo hygiene:** the repo is public. Never read `.env`, `apps/ios/project.local.yml` or `~/secrets`. No `Co-Authored-By` footers. Rebase and fast-forward; commit on green only, one commit per task.
- **Hermetic by default (invariant #3):** every new Rust type compiles and tests with no `whisper` feature, no model and no network.
- **Cargo:** every cargo command runs as `nix develop -c cargo ...` from `~/athanor-app`.
- **Rust gate:** after each Rust task, all three must pass:
  - `nix develop -c cargo fmt --all -- --check` prints nothing.
  - `nix develop -c cargo clippy --workspace --all-targets -- -D warnings` ends with `Finished`.
  - `nix develop -c cargo test --workspace` shows every `test result:` line `ok`.
- **Bindings:** `apps/ios/build-ffi.sh` runs from a login shell, never inside `nix develop` (the script refuses). It is the **only** way to regenerate bindings; do **not** use `just bindgen-swift` (wrong output directory, debug host dylib). It runs two release cross-builds, so launch it in the background (`run_in_background`) and wait for the notification.
- **Simulator:** focus-free only (launch args, `xcrun simctl`, the `idb` client). **osascript keystrokes are banned.**
- **Felt gestures stay identical (docs/gestures.md):**
  - The ember bed flares with RMS amplitude computed from the same buffers.
  - Partials render mercury-italic (`previewTail`).
  - Finals cool into the transcript (`finalizedAppend`).
  - Sustained silence sends (`utteranceEnded`).
  - Tapping the bed mutes/banks (`setMuted`).
  - Tapping settled text sends now (`sendNow`).

  The `BellowsController` protocol and the `BellowsEvent` enum keep their shape.
- **No new visible UI (invariant #8):** the engine choice lives in the triple-tap overlay, and `gestures.md` changes in the same commit as the overlay.
- **Operator gate:** Task 12 (device measurement over TestFlight) belongs to the operator.

## Sequencing

1. Task 0.
2. Tasks 1 and 2 (parallel worktrees allowed).
3. Task 3, then Task 4, then Task 5.
4. Task 6 can run in parallel with Tasks 1–5 (it uses no new FFI type). Tasks 8 and 11 can run any time.
5. Task 7 needs Tasks 5 and 6.
6. Task 9 needs Task 7.
7. Task 10 needs Tasks 7, 8 and 9.
8. Task 12 is the operator gate.

## File map

```
crates/stt/src/endpoint.rs                 MODIFY  add Endpointer::in_utterance()
crates/stt/src/text_bellows/mod.rs         CREATE  TextBellows turn machine, config, events, metrics, turn_tests
crates/stt/src/text_bellows/committer.rs   CREATE  append-only LocalAgreement committer and its tests
crates/stt/src/lib.rs                      MODIFY  pub mod + pub use
crates/ffi/src/text_bellows.rs             CREATE  TextBellowsHandle (uniffi::Object, NOT whisper-gated)
crates/ffi/src/lib.rs                      MODIFY  mod text_bellows
apps/ios/Packages/AthanorCoreFFI/Sources/AthanorCoreFFI/ffi.swift   REGENERATE (build-ffi.sh)
apps/ios/Sources/Engine/BellowsEngine.swift          CREATE  enum apple | whisperBase | whisperSmall
apps/ios/Sources/Engine/BellowsController.swift      MODIFY  snapshot fields, async factory
apps/ios/Sources/Engine/RealBellowsController.swift  MODIFY  init(handle:tier:), snapshot fill
apps/ios/Sources/Engine/AppleBellowsController.swift CREATE  recognizer + capture + forwarding (+ DEBUG file injector)
apps/ios/Sources/Engine/ModelDownloader.swift        MODIFY  verify/sha256/magic -> nonisolated static, run detached
apps/ios/Sources/App/AppModel.swift                  MODIFY  bellowsEngine, lazy download
apps/ios/Sources/Screens/SessionScreen.swift         MODIFY  openBellows(engine:) via factory; overlay engine row + Apple rows
apps/ios/Sources/Screens/InitiationScreen.swift      MODIFY  beginRealBellows via factory
apps/ios/project.yml + apps/ios/Sources/App/Info.plist   MODIFY  NSSpeechRecognitionUsageDescription
docs/gestures.md, docs/agent-brief.md, docs/grimoire.md  MODIFY
docs/research/2026-11-bellows-apple-latency.md       CREATE  measurement protocol + results table (Task 12)
```

---

## Apple facts this plan relies on

| Fact | Status | Settled by |
|---|---|---|
| `requiresOnDeviceRecognition`, `supportsOnDeviceRecognition`, `isAvailable` (iOS 13+); a request fails instead of falling back to the server | verified | n/a |
| `addsPunctuation` (16+), `taskHint`, `shouldReportPartialResults` | verified | n/a |
| `requestAuthorization` is callback-only; a missing `NSSpeechRecognitionUsageDescription` crashes on request | verified | n/a |
| `endAudio()` stops input; the final arrives after it; `cancel()` gives an error callback | verified | n/a |
| `simctl privacy` has no speech-recognition service | verified locally | n/a |
| A ~60 s per-request cap for **on-device live** recognition (documented for server recognition) | **UNVERIFIED** | Task 12 step 4 (`requests` rises during the monologue; any self-close shows in `self_closes`) |
| `contextualStrings` affects **on-device** results; 100 is a hard cap | **UNVERIFIED** (the cap is harmless either way) | Task 12 step 5 (jargon A/B against whisper) |
| The cumulative hypothesis **resets mid-request after a pause** on iOS 17 | **UNVERIFIED** | Task 12 steps 3–4 (`resets` metric) |
| The recognizer ends a request by itself while it is still fed (unprompted `isFinal`, or an error such as "no speech detected"); exact error codes | **UNVERIFIED** (handled either way by B1) | Task 12 (`self_closes` metric) |
| **Two concurrent `SFSpeechRecognitionTask`s** on one recognizer (the old one finishing after `endAudio` while the new one streams) | **UNVERIFIED** (the overlap design depends on it) | Task 10 step 6 + Task 12 (`timeouts == 0` across turns, lossless rotation seam); fallback F5 |
| On-device recognition works on the iOS Simulator | **UNVERIFIED** | Task 10 step 7 (honest-gap rule) |

---

## Shared definitions (used by Tasks 2 to 4; copy exactly)

```rust
// crates/stt/src/text_bellows/mod.rs
#[derive(Clone, Debug)]
pub struct TextBellowsConfig {
    pub sample_rate: u32,              // 16_000, same as SttConfig
    pub endpoint: EndpointConfig,      // EndpointConfig { silence_ms: 800, ..Default::default() }
    pub final_timeout_ms: u64,         // 600: max wait for a final after a cycle
    pub rotate_soft_ms: u64,           // 40_000: rotate if the request is this old AND the endpointer is Idle
    pub rotate_hard_ms: u64,           // 55_000: rotate regardless (hedge against an UNVERIFIED ~60 s cap)
    pub agree_n: usize,                // 2: LocalAgreement-2 over consecutive partials (must be >= 2)
    pub holdback_words: usize,         // 1: never commit the newest agreed word from a partial
    pub max_contextual_strings: usize, // 100
}

#[derive(Clone, Debug, PartialEq)]
pub enum TextBellowsEvent {
    Preview(String),                   // emitted only when changed; "" clears it
    Finalized(String),                 // append-only words, space-joined
    UtteranceEnded,
    CycleRequest { request_id: u64 },  // Swift: start a new request, swap it in, then endAudio() this one
    Unavailable,                       // terminal: recognizer keeps self-closing with no words (R4); Swift -> .permissionDenied
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextBellowsMetrics {
    pub pickup_ms: u64,         // wall ms: utterance onset (Idle -> non-Idle) -> first non-empty partial
    pub latch_ms: u64,          // audio ms: Endpointer::last_latency_ms at the last latched turn end (0 for send_now)
    pub finalize_ms: u64,       // wall ms: turn-ending cycle issued -> final committed (or timeout)
    pub requests: u64,          // begin_request count
    pub timeouts: u64,          // closes forced by final_timeout_ms
    pub self_closes: u64,       // final/error arriving on the ACTIVE request (the recognizer ended it itself)
    pub revisions_dropped: u64, // recognizer revised an already-committed word (committed form kept)
    pub resets: u64,            // hypothesis restarted mid-request (see the committer)
}
```

Every method that needs time takes `now_ms: u64` (a monotonic wall clock from the caller). The FFI handle supplies it from `Instant`, and tests pass literal numbers.

---

## Task 0: Branch

- [ ] Run `git switch -c bellows-apple-speech`. Then `git add docs/plans/2026-11-22-bellows-apple-speech*.md && git commit -m "docs: Bellows Apple-speech plan + review"`, so that later `git status` checks are clean (N7).
- [ ] Baseline: `nix develop -c cargo test --workspace`. Every `test result:` line must be `ok`.

## Task 1: `Endpointer::in_utterance()`

**Files:** modify `crates/stt/src/endpoint.rs`.

**Semantics:** `in_utterance()` is `!matches!(self.state, State::Idle)`. It is true in `Speaking` and in `TrailingSilence`. A dip of 30 ms or more between words is still the same utterance. It becomes false only after a latch fire, after a too-short blip is abandoned (both return to Idle once `silence_ms` of silence has passed), or after `reset()`.

- [ ] **Step 1: test first.** Add this in `endpoint.rs`'s `mod tests`, using the existing `sine(n_samples, freq_hz, amplitude)` helper:

```rust
#[test]
fn in_utterance_spans_trailing_silence_until_latch_or_reset() {
    let mut ep = Endpointer::new(16_000, EndpointConfig { silence_ms: 800, ..Default::default() });
    assert!(!ep.in_utterance(), "fresh endpointer is idle");
    for _ in 0..12 { ep.push(&sine(480, 440.0, 0.5)); }
    assert!(ep.in_utterance(), "voiced -> in utterance");
    for _ in 0..3 { ep.push(&vec![0.0; 480]); }
    assert!(ep.in_utterance(), "an inter-word dip is still the same utterance");
    let mut fired = false;
    for _ in 0..24 { fired |= ep.push(&vec![0.0; 480]); }   // 27 silent windows total = 810 ms
    assert!(fired, "latch fires at 810 ms");
    assert!(!ep.in_utterance(), "latch returns to idle");
    for _ in 0..12 { ep.push(&sine(480, 440.0, 0.5)); }
    ep.reset();
    assert!(!ep.in_utterance(), "reset returns to idle");
}
```
- [ ] **Step 2:** `nix develop -c cargo test -p stt in_utterance`. Expect `no method named in_utterance`.
- [ ] **Step 3:** implement it as a `pub fn`.
- [ ] **Step 4:** same command. Expect a line beginning `test result: ok. 1 passed; 0 failed`.
- [ ] **Step 5:** Rust gate. Commit `stt: Endpointer::in_utterance for text-bellows rotation and pickup`.

## Task 2: The append-only committer

**Files:**
- Create `crates/stt/src/text_bellows/committer.rs`.
- Create `crates/stt/src/text_bellows/mod.rs` containing: `mod committer;`, `pub(crate) use committer::Committer;`, the Shared definitions, and a stub `pub struct TextBellows { committer: Committer }` with a `#[allow(dead_code)]` on the field.
- At the top of `committer.rs`, add `#![cfg_attr(not(test), allow(dead_code))]` (S7: without it, clippy `-D warnings` fails on the non-test lib target). Task 3 removes both allowances.
- In `lib.rs`, add `pub mod text_bellows;` and `pub use text_bellows::{TextBellows, TextBellowsConfig, TextBellowsEvent, TextBellowsMetrics};`.

**Behaviour.** On-device SFSpeech delivers *cumulative* best-transcription strings per request, and partials can revise earlier words. The committer turns this into an append-only stream.

**Words and `norm`:**
- Words come from splitting on whitespace.
- `norm(w)` maps `’` (U+2019) to `'`, lowercases, and keeps only alphanumerics and `'`.
- All comparisons use `norm`. Committed text uses the surface form of the hypothesis current at commit time.

**Per-request state (N8):**
- `committed: Vec<String>`: norm'd words of the **current lineage** already emitted. `lineage_committed == committed.len()`.
- `lineage_text: Vec<String>`: norm'd words of the current lineage that have been emitted *or* flushed. Used for the overlap check after a reset.
- `history: VecDeque<Vec<String>>`: the last `agree_n` hypotheses, as surface words.
- `prev: Vec<String>`: the last hypothesis.
- Counters `revisions_dropped` and `resets`, plus `done: bool`.

**Reset detection** (shared by partial and final):
- With `new` the incoming words, a reset is: `lineage_committed > 0` **and** `new.len() < prev.len()` **and** no index `i < min(new.len(), lineage_committed)` has `norm(new[i]) == committed[i]`.
  - **Pause reset:** the recognizer dropped its earlier text, so the hypothesis shrank and nothing lines up. This works when exactly 1 word is committed (B2.2).
  - **First-word revision:** "the salt" → "a salt" keeps its length, so it is a *revision*, not a reset.
- **On reset:**
  1. **Flush the held tail.** Let `tail = prev[lineage_committed..]`. If it is non-empty, emit it and append its norm'd words to `lineage_text` (B2.1).
  2. **Overlap.** `k` is the largest k with `norm(new[..k]) == lineage_text[len-k..]`. This catches a final that drops filler and re-states already-emitted words (B2.3).
  3. **Restart the lineage.** Set `committed = norm(new[..k])`, `lineage_text = committed.clone()`, `history = [new]`, and `resets += 1`.
  4. **Preview** is `new[k..]`.
- **Accepted limit (Risk 6):** alignment is positional. An *insertion* before the commit point ("the salt is" → "the sea salt is") counts as a revision at slot 1, and the next commit re-emits "salt", which duplicates a word.

**`on_partial(words) -> (Option<String> /*finalized*/, String /*preview*/)`:**
1. If `done`, or the hypothesis is empty or whitespace-only, return `(None, current preview)`.
2. Check for a reset (above). If it fires, return `(flushed tail if any, preview)`. With `agree_n >= 2`, no commit can follow in the same call.
3. Otherwise, if any `i < min(len, lineage_committed)` has `norm(words[i]) != committed[i]`, increment `revisions_dropped` once for this hypothesis.
4. Push the hypothesis to `history` (cap `agree_n`). If `history.len() == agree_n`: `stable` is the longest common norm prefix over the history, and `upto = stable.saturating_sub(holdback_words)`. If `upto > lineage_committed`, emit `words[lineage_committed..upto]` and extend `committed` and `lineage_text`.
5. Set `prev = words`. The preview is `words[min(lineage_committed, len)..]` joined with spaces.

**`on_final(words) -> Option<String>`:**
1. If `done`, return `None`.
2. Check for a reset. On a reset, the emitted text is the flushed tail plus `words[k..]` (space-joined, `None` if empty).
3. Otherwise check for a revision as in step 3 above.
   - **No revision:** emit `words[min(lineage_committed, len)..]`, by position.
   - **Revision (R1):** count it, then compute the same overlap `k` against `lineage_text` that the reset path uses. If `k > 0`, emit `words[k..]`. If `k == 0`, fall back to the positional emit. This covers "um so" + final "So the fire is.", which has `k = 1` and emits "the fire is.". A plain slot-0 revision like "a salt is fixed" has `k = 0` and keeps the positional "is fixed", so it never duplicates.
4. Set `done = true`.

Add `pub(crate) fn last_hypothesis(&self) -> &[String]` (used by `ingest_error` and the timeout path), `revisions_dropped()`, `resets()` and `preview()`.

- [ ] **Step 1: tests first.** Put all of these in `committer.rs`'s `mod tests`, with `fn w(s: &str) -> Vec<String>` splitting on whitespace and `Committer::new(2, 1)`.

```rust
#[test]
fn normalize_strips_case_and_punctuation() {
    assert_eq!(norm("Fixed."), "fixed"); assert_eq!(norm("don't"), "don't");
    assert_eq!(norm("don’t"), "don't"); assert_eq!(norm("—"), "");
}
#[test]
fn local_agreement_commits_stable_prefix_with_holdback() {
    let mut c = Committer::new(2, 1);
    assert_eq!(c.on_partial(&w("the")),               (None, "the".into()));
    assert_eq!(c.on_partial(&w("the salt")),          (None, "the salt".into()));             // stable 1, upto 0
    assert_eq!(c.on_partial(&w("the salt is")),       (Some("the".into()), "salt is".into())); // stable 2, upto 1
    assert_eq!(c.on_partial(&w("the salt is fixed")), (Some("salt".into()), "is fixed".into()));// stable 3, upto 2
}
#[test]
fn final_commits_only_the_uncommitted_remainder() {
    let mut c = Committer::new(2, 1);
    for p in ["the", "the salt", "the salt is", "the salt is fixed"] { c.on_partial(&w(p)); }
    assert_eq!(c.on_final(&w("The salt is fixed.")), Some("is fixed.".into()));
    assert_eq!(c.on_final(&w("The salt is fixed.")), None, "done after final");
}
#[test]
fn revised_committed_word_is_never_rewritten() {
    let mut c = Committer::new(2, 1);
    for p in ["the", "the salt", "the salt is", "the salt is fixed"] { c.on_partial(&w(p)); }   // committed: the salt
    assert_eq!(c.on_partial(&w("a salt is fixed")), (None, "is fixed".into()), "same length: revision, not reset");
    assert_eq!(c.revisions_dropped(), 1);
    assert_eq!(c.resets(), 0);
}
#[test]
fn reset_flushes_the_held_back_tail() {
    let mut c = Committer::new(2, 1);
    assert_eq!(c.on_partial(&w("we tend the")),          (None, "we tend the".into()));
    assert_eq!(c.on_partial(&w("we tend the fire")),     (Some("we tend".into()), "the fire".into()));
    assert_eq!(c.on_partial(&w("we tend the fire now")), (Some("the".into()), "fire now".into()));
    assert_eq!(c.on_partial(&w("burns")),                (Some("fire now".into()), "burns".into()), "tail flushed on reset");
    assert_eq!(c.resets(), 1);
    assert_eq!(c.on_final(&w("burns brightly")), Some("burns brightly".into()));
}
#[test]
fn reset_detected_with_one_committed_word() {
    let mut c = Committer::new(2, 1);
    c.on_partial(&w("the salt"));
    assert_eq!(c.on_partial(&w("the salt is")), (Some("the".into()), "salt is".into()));      // 1 committed
    assert_eq!(c.on_partial(&w("burns")), (Some("salt is".into()), "burns".into()));
    assert_eq!((c.resets(), c.revisions_dropped()), (1, 0));
}
#[test]
fn filler_dropping_final_is_not_duplicated() {
    let mut c = Committer::new(2, 1);
    c.on_partial(&w("um so the"));
    assert_eq!(c.on_partial(&w("um so the fire")), (Some("um so".into()), "the fire".into()));
    assert_eq!(c.on_partial(&w("um so the fire")), (Some("the".into()), "fire".into()));
    // final drops "um": shrank 4 -> 3, no slot matches -> reset; flush "fire"; overlap k = 3 -> nothing new
    assert_eq!(c.on_final(&w("So the fire.")), Some("fire".into()));
}
#[test]
fn filler_dropping_longer_final_keeps_every_word() {
    let mut c = Committer::new(2, 1);
    c.on_partial(&w("um so the"));
    assert_eq!(c.on_partial(&w("um so the")), (Some("um so".into()), "the".into()));   // committed [um, so]
    // final 4 >= prev 3: no reset; slot 0 so != um: revision -> overlap k = 1 ([so]) -> words[1..]
    assert_eq!(c.on_final(&w("So the fire is.")), Some("the fire is.".into()), "\"the\" is not lost");
    assert_eq!((c.revisions_dropped(), c.resets()), (1, 0));
}
#[test]
fn slot_zero_revision_final_without_overlap_stays_positional() {
    let mut c = Committer::new(2, 1);
    for p in ["the", "the salt", "the salt is", "the salt is fixed"] { c.on_partial(&w(p)); }   // committed: the salt
    assert_eq!(c.on_final(&w("A salt is fixed.")), Some("is fixed.".into()), "k = 0 -> positional, no duplicate \"salt\"");
}
#[test]
fn empty_and_whitespace_partials_are_ignored() {
    let mut c = Committer::new(2, 1);
    c.on_partial(&w("the salt"));
    assert_eq!(c.on_partial(&w("")),    (None, "the salt".into()));
    assert_eq!(c.on_partial(&w("   ")), (None, "the salt".into()));
}
```
- [ ] **Step 2:** `nix develop -c cargo test -p stt text_bellows::committer`. Expect a compile failure.
- [ ] **Step 3:** implement `Committer` and `norm`.
- [ ] **Step 4:** same command. Expect a line beginning `test result: ok. 10 passed; 0 failed`.
- [ ] **Step 5:** Rust gate (clippy must pass with the stub). Commit `stt: append-only LocalAgreement committer for cumulative recognizer text`.

## Task 3: `TextBellows` turn machine

**Files:** `crates/stt/src/text_bellows/mod.rs` (replace the stub, remove both dead-code allowances, add `#[cfg(test)] mod turn_tests`).

**API (exact):**
```rust
impl TextBellows {
    pub fn new(cfg: TextBellowsConfig, bias_terms: &[String]) -> Self;   // asserts cfg.agree_n >= 2
    pub fn default_config() -> TextBellowsConfig;                         // values in Shared definitions
    pub fn contextual_strings(&self) -> Vec<String>;   // trim, drop empty, dedupe (first wins), cap max_contextual_strings
    pub fn begin_request(&mut self, now_ms: u64) -> u64;   // ids start at 1, monotonic; new Active request
    pub fn push_pcm(&mut self, pcm: &[f32], now_ms: u64);
    pub fn ingest_partial(&mut self, request_id: u64, text: &str, now_ms: u64);
    pub fn ingest_final(&mut self, request_id: u64, text: &str, now_ms: u64);
    pub fn ingest_error(&mut self, request_id: u64, now_ms: u64);   // = final with that request's last hypothesis
    pub fn send_now(&mut self, now_ms: u64);
    pub fn reset_endpoint(&mut self);                               // mute/bank: Endpointer::reset only
    pub fn tick(&mut self, now_ms: u64);                            // final timeouts + rotation
    pub fn drain(&mut self) -> Vec<TextBellowsEvent>;
    pub fn metrics(&self) -> TextBellowsMetrics;
}
```

**State:** each request owns a `Committer`, an `outbox: Vec<TextBellowsEvent>` (only `Finalized` and `UtteranceEnded`), `started_ms`, and a status:
- `Active`: at most one at a time.
- `Closing { since_ms, ends_turn: bool }`
- `Closed`

A separate `control: Vec<TextBellowsEvent>` holds `CycleRequest`s (S3).

**Rules:**
1. **Turn end.** A turn ends when `push_pcm` makes the Endpointer fire (`latch_ms = endpointer.last_latency_ms()`), or when `send_now` is called (`latch_ms = 0`, then `Endpointer::reset`).
   - **With an Active request:** it becomes `Closing { since_ms: now, ends_turn: true }`, and `CycleRequest { id }` goes onto `control`.
   - **With no Active request** (S2: the gap between a cycle and Swift's `begin_request`):
     - If the newest request is `Closing`, set its `ends_turn = true` and `since_ms = now`.
     - If it is `Closed`, push `UtteranceEnded` into its outbox.
     - If there are no requests, push `UtteranceEnded` into a pre-request outbox that drains first.
2. **Rotation (`tick`).**
   - Soft: the Active request's `now - started_ms >= rotate_soft_ms` **and** `!endpointer.in_utterance()`.
   - Hard: `now - started_ms >= rotate_hard_ms`.
   - Either way the request becomes `Closing { ends_turn: false }`, and `CycleRequest` goes onto `control`.
3. **Self-close (B1).** `ingest_final` or `ingest_error` on the **Active** request:
   - Commit its remainder (`on_final`, using `last_hypothesis()` for an error) into its outbox.
   - Mark it `Closed`, increment `self_closes`, and push `CycleRequest { id }` onto `control`, so Swift starts a fresh request.
   - **Storm backoff (R4).** Keep a counter `empty_self_closes`. Any non-empty partial or final on any request resets it to 0. A self-close of a request whose committer never saw a non-empty hypothesis increments it.
     - When it reaches **3**, do **not** queue the `CycleRequest`. Push `Unavailable` onto `control` and set `halted = true`.
     - Once halted, `begin_request` returns 0 and every input is ignored (no further events).
     - This prevents a ~20 requests/s churn with a live mic, e.g. missing on-device assets or speech revoked mid-session.
     - Only self-closes count. Rotation and turn-end closes never do.
   - The turn does **not** end; the learner may still be mid-thought.
4. **Close of a `Closing` request.**
   - **Triggers:** `ingest_final` for its id; `ingest_error` (uses the last hypothesis); or `tick` once `now - since_ms >= final_timeout_ms` (timeout: commit the remainder of `last_hypothesis()` and increment `timeouts`).
   - **Commit:** push `Finalized(rem)` if non-empty.
   - **If `ends_turn`:** push `UtteranceEnded` and set `finalize_ms = now - since_ms`.
   - Mark it `Closed`.
5. **Partials.**
   - For the Active request, or for a `Closing` request (its tail can still change before the final), feed the request's committer. Any `Finalized` goes into that request's outbox.
   - Pickup: when `push_pcm` takes `in_utterance()` from false to true, record `onset_ms = now`. The first non-empty partial after that sets `pickup_ms = now - onset_ms` and clears `onset_ms`. Dips inside an utterance therefore never re-arm it (S1).
6. **Stale events.** A partial, final or error for a `Closed` or unknown id is ignored.
7. **`drain` order:**
   1. All of `control`.
   2. The pre-request outbox.
   3. Outboxes in id order. A request's outbox drains even while it is `Closing`, but iteration **stops after the first request that is not `Closed`** (Active counts as not Closed). So request N+1's words never come before request N's remainder or its `UtteranceEnded`.
   4. Finally the preview. It is computed as the concatenation of: the preview of the oldest non-Closed request; then, for each later request, its *held* outbox `Finalized` words (N9) followed by its committer preview. Words are joined with spaces, and empty parts are skipped. If it differs from the last emitted preview, append `Preview(s)`. After an `ends_turn` close with nothing pending, the preview is "" and is emitted once.

- [ ] **Step 1: tests first** in `turn_tests`. Helpers (N3):
  - `turn_tests` gets its own private `sine` (endpoint's helper is private to its own test module).
  - `voiced(b, n, &mut now)` does `now += 30; b.push_pcm(&sine(480, 440.0, 0.5), now)` n times.
  - `silent(b, n, &mut now)` does the same with `&[0.0; 480]`.
  - `drain_no_preview(b)` drains and filters out `Preview(_)`.

  Worked example, test 1:

```rust
use TextBellowsEvent::*;
#[test]
fn latch_cycles_request_then_final_ends_utterance() {
    let mut b = TextBellows::new(TextBellows::default_config(), &[]);
    let mut now = 0;
    assert_eq!(b.begin_request(now), 1);
    voiced(&mut b, 12, &mut now);                       // 360 ms speech, now = 360
    b.ingest_partial(1, "the salt", now);
    b.ingest_partial(1, "the salt is", now);            // commits "the"
    assert_eq!(drain_no_preview(&mut b), vec![Finalized("the".into())]);
    silent(&mut b, 26, &mut now);                       // now = 1140, 780 ms: no latch
    assert_eq!(drain_no_preview(&mut b), vec![]);
    silent(&mut b, 1, &mut now);                        // now = 1170, 810 ms: latch
    assert_eq!(b.drain(), vec![CycleRequest { request_id: 1 }], "preview unchanged, not re-emitted");
    assert_eq!(b.begin_request(now), 2);
    b.ingest_final(1, "The salt is fixed.", now + 180);
    assert_eq!(b.drain(), vec![Finalized("salt is fixed.".into()), UtteranceEnded, Preview("".into())]);
    let m = b.metrics();
    assert_eq!((m.latch_ms, m.finalize_ms, m.requests, m.pickup_ms), (810, 180, 2, 330));
}
```
  Remaining tests. Each has exact asserts in the same style, and every test except 1 and 11 uses `drain_no_preview`:
  2. `final_timeout_commits_last_partial_and_ignores_late_final`: after the latch at `since`, there is no final. `tick(since+599)` gives `[]`. `tick(since+600)` gives `[Finalized("salt is"), UtteranceEnded]`. `timeouts == 1`. A later `ingest_final(1, ..)` gives `[]`.
  3. `send_now_cycles_without_silence_and_zeroes_latch` (N5): first complete one latched turn (so `latch_ms == 810`). Then on request 2, speak, partial, and call `send_now`: this gives `[CycleRequest{2}]`. The final gives `UtteranceEnded`, and now `latch_ms == 0`.
  4. `next_request_words_wait_behind_closing_request`: after the cycle of request 1, request 2 commits "next words" (via partials "next words now" twice) before request 1's final. The drain has no `Finalized("next words")`. After request 1's final: `[Finalized("salt is fixed."), UtteranceEnded, Finalized("next words")]`.
  5. `soft_rotation_only_when_idle_and_does_not_end_utterance`: `begin_request(0)`; 3 voiced windows (90 ms blip, under the 300 ms minimum); 27 silent windows (blip abandoned, back to Idle); partial "hm". `tick(39_999)` gives `[]`. `tick(40_000)` gives `[CycleRequest{1}]`. Final "hm" gives `[Finalized("hm")]`, with no `UtteranceEnded`.
  6. `hard_rotation_while_in_utterance`: `begin_request(0)`, 12 voiced, 3 silent (TrailingSilence, still in the utterance). `tick(40_000)` gives `[]`. `tick(55_000)` gives `[CycleRequest{1}]`.
  7. `stale_request_events_are_ignored`: a partial and a final for id 99, and for a request that is already Closed, both give `[]`.
  8. `reset_endpoint_clears_pending_latch`: 12 voiced, 20 silent, `reset_endpoint()`, 10 silent gives no `CycleRequest`. (Without the reset, the 27th silent window would fire.)
  9. `pickup_latency_measures_onset_to_first_partial`: the first voiced window is at now = 30, the partial "the" at 250, so `pickup_ms == 220`.
  10. `contextual_strings_cap_dedupe_trim`: 150 inputs (including duplicates, `"  "` and `" salt "`) give 100 unique, trimmed strings in input order, and `"salt"` appears once.
  11. `preview_concatenates_closing_tail_held_words_and_active`: request 1 is closing with tail "is fixed"; request 2 holds `Finalized("next")` and previews "words". The last `Preview` in the drain is `"is fixed next words"`.
  12. `active_request_final_cycles_without_ending_turn` (B1): during speech, `ingest_final(1, "the salt is", now)` gives `[CycleRequest{1}, Finalized("salt is")]`. **Drain after the committing partials first**, so `Finalized("the")` is already consumed, with no `UtteranceEnded`, and `self_closes == 1`. After `begin_request` gives 2, a latch later ends the turn on request 2.
  13. `active_request_error_cycles_to_fresh_request` (B1): partials "we tend the fire" twice (committing "we tend the"), **drain**, then `ingest_error(1)`. This gives `[CycleRequest{1}, Finalized("fire")]`. `begin_request` gives 2, and request 2's partials flow normally.
  14. `latch_with_no_active_request_marks_newest_closing` (S2): a soft rotation of request 1 leaves no Active request; the latch fires before `begin_request`. The final for request 1 gives `[Finalized(..), UtteranceEnded]`.
  15. `pickup_not_rearmed_by_interword_dip` (S1): onset at 30 and partial at 250 give `pickup_ms == 220`. Then 3 silent windows, voiced again, and a partial at +100 ms: `pickup_ms` stays 220.
  16. `self_close_storm_halts_to_unavailable` (R4):
     - `begin_request` → 1, `ingest_error(1)` → `[CycleRequest{1}]`.
     - `begin_request` → 2, `ingest_error(2)` → `[CycleRequest{2}]`.
     - `begin_request` → 3, `ingest_error(3)` → `[Unavailable]` (no `CycleRequest`). `self_closes == 3`.
     - `begin_request` → 0, and a later partial gives `[]`.
     - Second half, on a fresh `TextBellows`: two empty self-closes, then request 3 gets the partial "hm" and self-closes. The streak resets, so the drain is `[CycleRequest{3}, Finalized("hm")]` and there is no `Unavailable`.

- [ ] **Step 2:** `nix develop -c cargo test -p stt text_bellows::turn_tests`. Expect compile errors.
- [ ] **Step 3:** implement.
- [ ] **Step 4:** same command. Expect a line beginning `test result: ok. 16 passed; 0 failed`. Then `nix develop -c cargo test -p stt text_bellows` reports `26 passed`.
- [ ] **Step 5:** Rust gate. Commit `stt: TextBellows turn machine (Rust-owned latch, cycle, self-close, append-only finalize)`.

## Task 4: FFI `TextBellowsHandle`

**Files:** create `crates/ffi/src/text_bellows.rs` and add `mod text_bellows;` to `crates/ffi/src/lib.rs`. No `#[cfg(feature = "whisper")]` anywhere.

```rust
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum FfiTextBellowsEvent {
    Preview { text: String }, Finalized { text: String }, UtteranceEnded, CycleRequest { request_id: u64 }, Unavailable,
}
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct FfiTextBellowsMetrics { pub pickup_ms: u64, pub latch_ms: u64, pub finalize_ms: u64,
    pub requests: u64, pub timeouts: u64, pub self_closes: u64, pub revisions_dropped: u64, pub resets: u64 }

#[derive(uniffi::Object)]
pub struct TextBellowsHandle {
    inner: std::sync::Mutex<stt::TextBellows>,
    clock: Box<dyn Fn() -> u64 + Send + Sync>,
}
impl TextBellowsHandle {
    // Not exported. `new` delegates here, so it is live code on the non-test target (S7).
    fn with_clock(bias_terms: Vec<String>, silence_latch_ms: u64, clock: Box<dyn Fn() -> u64 + Send + Sync>) -> Self;
}
#[uniffi::export]
impl TextBellowsHandle {
    #[uniffi::constructor]
    pub fn new(bias_terms: Vec<String>, silence_latch_ms: u64) -> std::sync::Arc<Self>; // Instant::now() at open; clock = elapsed ms
    pub fn contextual_strings(&self) -> Vec<String>;
    pub fn begin_request(&self) -> u64;
    pub fn push_pcm(&self, pcm: Vec<f32>);
    pub fn ingest_partial(&self, request_id: u64, text: String);
    pub fn ingest_final(&self, request_id: u64, text: String);
    pub fn ingest_error(&self, request_id: u64);
    pub fn send_now(&self);
    pub fn reset_endpoint(&self);
    pub fn poll(&self) -> Vec<FfiTextBellowsEvent>;   // tick(now) then drain()
    pub fn metrics(&self) -> FfiTextBellowsMetrics;
}
```
`new` uses `TextBellows::default_config()` with `endpoint.silence_ms = silence_latch_ms`. The file stays a pure projection, following the `bellows.rs` discipline.

- [ ] **Step 1: tests first.**
  1. `handle_round_trips_latch_cycle_final`: a fake clock from `Arc<AtomicU64>`, driven through the handle by the Task 3 test 1 script. With a `poll_no_preview` filter helper:
     - after the partials: `[Finalized{"the"}]`
     - after the latch: `[CycleRequest{request_id:1}]`
     - after the final: `[Finalized{"salt is fixed."}, UtteranceEnded]`
  2. `ffi_event_projects_core_event`: all five variants (including `Unavailable`).
  3. `metrics_project`: field by field.
- [ ] **Step 2:** `nix develop -c cargo test -p ffi text_bellows`. Expect a compile failure.
- [ ] **Step 3:** implement.
- [ ] **Step 4:** same command. Expect a line beginning `test result: ok. 3 passed; 0 failed`.
- [ ] **Step 5:** Rust gate, plus `nix develop -c cargo check -p ffi --features whisper`, which must end with `Finished` (N2: whisper still compiles on the host). Commit `ffi: TextBellowsHandle bridge (ungated, hermetic)`.

## Task 5: Regenerate Swift bindings

- [ ] **Step 1:** from a login shell, in the background: `cd ~/athanor-app && ./apps/ios/build-ffi.sh > "$SCRATCH/build-ffi.log" 2>&1`. Expect exit 0. It still builds `--features goose,whisper`; that is deliberate.
- [ ] **Step 2:** `grep -c "TextBellowsHandle\|FfiTextBellowsEvent" apps/ios/Packages/AthanorCoreFFI/Sources/AthanorCoreFFI/ffi.swift` gives a number ≥ 2.
- [ ] **Step 3:** `git status --short` shows only ` M apps/ios/Packages/AthanorCoreFFI/Sources/AthanorCoreFFI/ffi.swift`. Commit `ios: regenerate bindings for TextBellowsHandle`.

## Task 6: Engine selection, snapshot fields, overlay engine row, lazy whisper download, whisper off main

**Diagnosis check** (analogy: `~/athanor/forge/jefe-review/speech/diagnosis.md`):
- **`BellowsHandle.open` runs on the main thread (CONFIRMED).** `BellowsFactory.makeRealController` is `@MainActor` (`BellowsController.swift:103`). It calls the synchronous `RealBellowsController.init`, which calls `BellowsHandle.open` (`RealBellowsController.swift:70`): the whisper model read plus Metal init, with a first-run shader compile. Athanor has no shared `stt_warm` mutex, so the result is a multi-second jank on first open, not a deadlock.
- **The hash on main is narrower than first claimed (corrected).** `ModelDownloader` is `@MainActor` and `verify` is synchronous. But the SHA-256 path only runs when `cachedSHA256[tier]` is set, and that in-memory cache is filled only by `fetchExpectedSHA256` inside `download()`. So the full 57/182 MB hash runs on main **once after each fresh download** (and on a same-process switch back to a tier downloaded in that process). At launch with a model already on disk, `verify` does only the cheap size and magic-byte check.

Both still move off main while whisper stays selectable.

**Files:** create `BellowsEngine.swift`. Modify `BellowsController.swift`, `RealBellowsController.swift`, `ModelDownloader.swift`, `AppModel.swift`, `SessionScreen.swift`, `InitiationScreen.swift`, `docs/gestures.md`.

- [ ] **Step 1: `BellowsEngine`:**
```swift
enum BellowsEngine: String, CaseIterable, Identifiable, Codable {
    case apple, whisperBase = "whisper-base", whisperSmall = "whisper-small"
    var id: String { rawValue }
    var whisperTier: ModelTier? { switch self { case .apple: nil; case .whisperBase: .base; case .whisperSmall: .small } }
    var displayName: String { switch self { case .apple: "Apple"; case .whisperBase: "W·Base"; case .whisperSmall: "W·Small" } }
}
```
- [ ] **Step 2: snapshot fields (moved here from the old Task 9; B3).** Add `engine: String`, `pickupMs: UInt64`, `latchMs: UInt64` and `timeouts: UInt64` to `SttMetricsSnapshot`. `RealBellowsController.currentMetrics()` fills `engine: "whisper-\(tier.rawValue)"`, `latchMs: m.utteranceEndLatencyMs`, `pickupMs: 0`, `timeouts: 0`. Store it as `nonisolated let tier: ModelTier` (`ModelTier` is Sendable), so the existing `nonisolated func currentMetrics()` can read it.
- [ ] **Step 3: AppModel.**
  - Replace `modelTier` with `var bellowsEngine: BellowsEngine`, persisted under `"athanor.bellowsEngine"`. The default is `.apple`, and the old `athanor.modelTier` key is ignored.
  - The QA launch arg `bellows-engine=<rawValue>` overrides it for the process without persisting.
  - In `init`, call `ensureModel` **only if** `bellowsEngine.whisperTier != nil`.
- [ ] **Step 4: ModelDownloader (S8).**
  - Turn `verify(fileAt:tier:)` into `nonisolated static func verify(fileAt url: URL, approxBytes: Int64, expectedSHA: String?, usingFixture: Bool) -> Bool`.
  - Make `sha256(of:)` and `sanityCheckMagicBytes(url:)` `nonisolated static` as well.
  - At **both** call sites (`ensureModel` ~`:60` and `download` ~`:136`), first read `cachedSHA256[tier]`, `usingFixture` and `tier.approxByteCount` on main. Then call `await Task.detached(priority: .utility) { Self.verify(fileAt:..., ...) }.value`.
  - Keep the existing size-band logic unchanged and pass `approxBytes` through for it.
- [ ] **Step 5: RealBellowsController.** `init(modelPath:tier:biasTerms:) throws` becomes `init(handle: AthanorCoreFFI.BellowsHandle, tier: ModelTier)`. The open moves into the factory.
- [ ] **Step 6: one async factory** replaces `makeRealController`:
```swift
enum BellowsFactory {
    @MainActor
    static func make(engine: BellowsEngine, downloader: ModelDownloader, biasTerms: [String]) async -> (any BellowsController)? {
        #if canImport(AthanorCoreFFI)
        switch engine.whisperTier {
        case nil:
            return nil   // Task 7 replaces this with AppleBellowsController(biasTerms:)
        case let tier?:
            await downloader.ensureModel(tier: tier)             // lazy: only when whisper is chosen
            guard let path = downloader.readyPath else { return nil }
            let latch = RealBellowsController.defaultSilenceLatchMs
            let ffiTier: AthanorCoreFFI.BellowsTier = tier == .small ? .smallEn : .baseEn
            do {
                let handle = try await Task.detached(priority: .userInitiated) {
                    try AthanorCoreFFI.BellowsHandle.open(modelPath: path, biasTerms: biasTerms,
                                                          tier: ffiTier, silenceLatchMs: latch)
                }.value                                          // model load OFF the main actor
                return RealBellowsController(handle: handle, tier: tier)
            } catch { NSLog("[Athanor] whisper open failed: \(error)"); return nil }
        }
        #else
        return nil
        #endif
    }
}
```
  Until Task 7, Apple returns `nil`, which means the typed fallback, so this commit is coherent.
- [ ] **Step 7: screens.**
  - `SessionScreen.openBellows(engine:)` and `InitiationScreen.beginRealBellows()` call `BellowsFactory.make(engine: model.bellowsEngine, downloader: model.modelDownloader, biasTerms: BellowsBias.terms(engine: model.engine))`. Their existing `for await event` switch stays unchanged.
  - **Keep** SessionScreen's `guard Self.debugTurnText == nil else { return }` at the top of `openBellows` (N6).
  - Delete both 40 × 250 ms `readyPath` poll loops (`SessionScreen.swift:540-546`, `InitiationScreen.swift:225-231`).
  - `switchTier(to:)` becomes `switchEngine(to: BellowsEngine)`, with the same stop → nil → set → reopen sequence (`switchingTier` becomes `switchingEngine`).
- [ ] **Step 8: overlay engine row (B3).**
  - In `SttOverlay`, replace the props `tier: ModelTier` and `onSwitchTier:` with `engine: BellowsEngine` and `onSwitchEngine:`.
  - Replace the `ForEach(ModelTier.allCases)` row with `ForEach(BellowsEngine.allCases)` (label `engine`, same chip styling).
  - The existing whisper rows stay. For an Apple snapshot this interim version shows `engine` plus `send latch` only; Task 9 adds the Apple rows.
  - In `docs/gestures.md`, change the triple-tap row to `STT dev overlay (engine picker Apple / Whisper Base / Whisper Small; latency numbers)`. Same commit (invariant #8).
- [ ] **Step 9: verify.**
  - `grep -rn "modelTier\|ModelTier.allCases\|switchTier" apps/ios/Sources` is empty.
  - `grep -rn "BellowsHandle\.open(modelPath" apps/ios/Sources` gives exactly **1** hit, inside `Task.detached` (S9).
  - Real build: `cd apps/ios && ./generate.sh && xcodebuild -project Athanor.xcodeproj -scheme Athanor -destination "platform=iOS Simulator,id=$(xcrun simctl list devices available | grep -m1 iPhone | grep -oE '[0-9A-F-]{36}')" -derivedDataPath /tmp/athanor-dd build 2>&1 | tail -1` gives `** BUILD SUCCEEDED **`.
  - Demo build: `cd apps/ios && xcodegen generate && xcodebuild` (same arguments) `build 2>&1 | tail -1` gives `** BUILD SUCCEEDED **`. Then run `./generate.sh` again to restore the real project.
- [ ] **Step 10:** commit `ios: Bellows engine selection, lazy whisper download, whisper open + hash off main`.

## Task 7: `AppleBellowsController`

**Files:** create `apps/ios/Sources/Engine/AppleBellowsController.swift` (the whole body inside `#if canImport(AthanorCoreFFI)`). Modify the `.apple` arm of the factory to `return AppleBellowsController(biasTerms: biasTerms)`.

Donor: `~/murmur-rmp/apps/ios/Sources/Engine/TranscriptSource.swift` `SpeechSource`. Take only its permission wrapper (`:83-89`) and its request setup. Diffing, rotation and error handling are not taken from it; Rust owns them.

Shape (render and capture only):
```swift
@MainActor
final class AppleBellowsController: BellowsController {
    nonisolated private let handle: AthanorCoreFFI.TextBellowsHandle
    private let recognizer = SFSpeechRecognizer(locale: Locale(identifier: "en-US"))
    private let engine = AVAudioEngine()
    private let contextual: [String]
    // Tap thread appends to the current request; rotation swaps it. The request isn't Sendable:
    nonisolated private let current = OSAllocatedUnfairLock<SFSpeechAudioBufferRecognitionRequest?>(uncheckedState: nil)
    private var live: [UInt64: (task: SFSpeechRecognitionTask, request: SFSpeechAudioBufferRecognitionRequest)] = [:]
    nonisolated(unsafe) private var muted = false
    nonisolated(unsafe) private var converter: AVAudioConverter?   // built from the incoming buffer format
    nonisolated private static let log = Logger(subsystem: "com.damsac.athanor", category: "bellows")
    let events: AsyncStream<BellowsEvent>
    nonisolated private let continuation: AsyncStream<BellowsEvent>.Continuation
    init(biasTerms: [String]) {
        handle = TextBellowsHandle(biasTerms: biasTerms, silenceLatchMs: RealBellowsController.defaultSilenceLatchMs)
        contextual = handle.contextualStrings()          // the cap and dedupe are decided in Rust
        (events, continuation) = AsyncStream<BellowsEvent>.makeStream()
    }
}
```

Behaviour:
1. **`start()`:**
   - `await AVAudioApplication.requestRecordPermission()`.
   - Then speech authorization, via the continuation wrapper (S5):
     `await withCheckedContinuation { c in SFSpeechRecognizer.requestAuthorization { c.resume(returning: $0 == .authorized) } }`.
   - **Refuse** when the mic or speech permission is refused, `recognizer == nil`, `supportsOnDeviceRecognition == false`, or `!recognizer.isAvailable`. On refusal, log the reason (e.g. `on-device recognition unavailable`) and yield `.permissionDenied`, which means the typed fallback with no nagging. Never fall back to server recognition, and never silently fall back to whisper.
   - Otherwise: set the audio session (`.record` / `.measurement`, as whisper does), install the tap, `engine.start()`, call **`startRequest()` once** (S5), start the poll loop, and log `AppleBellows started on_device=true contextual=\(n)`.
2. **The tap** (4096 frames at the input's native format) calls `ingest(buffer)`:
```swift
nonisolated private func ingest(_ buffer: AVAudioPCMBuffer) {
    guard !muted else { return }                       // muted: skip BOTH the append and pushPcm (S5)
    current.withLockUnchecked { $0?.append(buffer) }
    // 16 kHz mono f32: same conversion code as RealBellowsController.swift:171-193, but build/cache
    // `converter` from `buffer.format` (so the DEBUG file injector, with its own format, works; S11)
    handle.pushPcm(pcm: samples)
    let amplitude = Double(min(rms(samples) * 5, 1.0))  // same rms, same buffers: identical ember bed
    continuation.yield(.amplitude(amplitude))
}
```
3. **`startRequest()`:**
   - `let id = handle.beginRequest()`.
   - Build an `SFSpeechAudioBufferRecognitionRequest` with `shouldReportPartialResults = true`, `requiresOnDeviceRecognition = true`, `contextualStrings = contextual`, `addsPunctuation = true`, `taskHint = .dictation`.
   - Swap it into `current`, start `recognizer.recognitionTask(with:)`, and store `live[id] = (task, request)`.
   - **Result callback:** if `isFinal`, call `handle.ingestFinal(requestId: id, text:)`; otherwise call `handle.ingestPartial(requestId: id, text:)`, passing `bestTranscription.formattedString`.
   - **Error callback:** `handle.ingestError(requestId: id)`.
   - After a final or an error, hop to MainActor and remove `live[id]`. The callback never touches the UI.
4. **Poll loop** (`Task.detached`, every 50 ms; cheap because there is no decode). For each event of `handle.poll()`:
   - `.preview` → `.previewTail`
   - `.finalized` → `.finalizedAppend`
   - `.utteranceEnded` → `.utteranceEnded`
   - `.cycleRequest(id)` → hop to MainActor and **rotate in this order** (S4): `startRequest()` first (it swaps the new request into `current`), **then** `live[id]?.request.endAudio()`. The old task stays alive so its final can arrive.
   - `.unavailable` (R4) → log `recognizer unavailable: self-close storm`, yield `.permissionDenied` (typed fallback, no nagging), then call `stop()`. This makes no decision in Swift; it only maps Rust's verdict.

   Log each cycle (`cycle from=\(id)`). Log each utterance end with counts only: `utterance ended finalize_ms=… latch_ms=… timeouts=…`. Log finalized text only under `#if DEBUG` with `privacy: .public` (S13). Release builds log the word count only.
5. **`setMuted(m)`:** set `muted`; if `m`, call `handle.resetEndpoint()`.
   **`sendNow()`:** call `handle.sendNow()`; the poll loop then delivers the cycle, the final and `utteranceEnded`.
   **`stop()`:** cancel the poll task, remove the tap, stop the engine, and for every `live` entry call `request.endAudio()` and `task.cancel()`. Then deactivate the session and call `continuation.finish()`.
6. **`nonisolated func currentMetrics()`** (and a `nonisolated static func rms`) maps `handle.metrics()` to `SttMetricsSnapshot`:
   - `engine: "apple"`
   - `utteranceEndLatencyMs = finalizeMs`, `latchMs = latchMs`, `pickupMs = pickupMs`, `timeouts = timeouts`
   - `decodePasses = requests` (S5: the overlay's "requests" row reads `decodePasses`)
   - `realtimeFactor = 0`, `gpuRequested = false`, `lastDecodeMs = 0`, `lastWindowMs = 0`

   Log `selfCloses`, `resets` and `revisionsDropped` at each utterance end. They feed Task 12.

- [ ] **Step 1:** write the file and switch the factory's `.apple` arm.
- [ ] **Step 2:** real and demo builds as in Task 6 Step 9. Expect `** BUILD SUCCEEDED **` twice.
- [ ] **Step 3:** `grep -nE "hasPrefix|dropFirst|split\(|components\(" apps/ios/Sources/Engine/AppleBellowsController.swift` is empty. This is the invariant #1 check: no text diffing in Swift.
- [ ] **Step 4:** commit `ios: AppleBellowsController over SFSpeechRecognizer (on-device), Rust-owned turns`.

## Task 8: Speech permission string

**Files:** `apps/ios/project.yml` and the tracked `apps/ios/Sources/App/Info.plist`.

- [ ] **Step 1:** in `project.yml`, under `NSMicrophoneUsageDescription` (`:51`), add:
  `NSSpeechRecognitionUsageDescription: "Athanor turns your Bellows into words on this iPhone, so the Mystagogue can follow what you tend."`
  This is accurate: recognition is on-device, but the resulting text goes to the model, so do not claim "never leaves the device".
- [ ] **Step 2:** `cd apps/ios && ./generate.sh`. xcodegen rewrites Info.plist.
- [ ] **Step 3:** `grep -c NSSpeechRecognitionUsageDescription apps/ios/project.yml apps/ios/Sources/App/Info.plist` gives `:1` for each file.
- [ ] **Step 4:** commit `ios: speech recognition usage description`.

Without this key the app crashes at `requestAuthorization`, so Task 10 needs Tasks 7 and 8 both landed.

## Task 9: Apple latency rows in the overlay

**Files:** `SessionScreen.swift` (`SttOverlay`), `docs/gestures.md`.

- [ ] **Step 1:** when `m.engine == "apple"`, render these rows instead of the whisper ones:
  - `pickup` (`pickupMs`)
  - `latch` (`latchMs`)
  - `finalize` (`utteranceEndLatencyMs`)
  - `felt send` (`latchMs + utteranceEndLatencyMs`)
  - `requests` (`decodePasses`)
  - `timeouts`

  Show `—` for zero. The whisper rows stay as they are. The downloading label still shows while switching to an undownloaded whisper engine.
- [ ] **Step 2:** in `docs/gestures.md`, extend the triple-tap row with `; Apple: pickup / latch / finalize / felt-send`. Also fix the existing drift on the bed-tap row (N11): `:13` says "sends what was heard", but the code mutes/banks (`SessionScreen.swift:1090`). It should read `Bank: mute the Bellows (tap again to resume)`.
- [ ] **Step 3:** real build gives `** BUILD SUCCEEDED **`. Commit `ios: dev overlay Apple latency rows; gestures.md bed-tap fix`.

## Task 10: Simulator smoke (focus-free)

**Files:** `AppleBellowsController.swift` (a `#if DEBUG` audio-file injector).

The recognizer cannot be unit-tested on the simulator, so drive it end to end with a deterministic file through the *same* `ingest(_:)` path.

- [ ] **Step 1:** under `#if DEBUG`, when the launch arg `bellows-audio-file=<path>` is present, skip `installTap`. Instead, read the file with `AVAudioFile` and feed 4096-frame buffers to `ingest(_:)` at realtime pace (`Task.sleep` for each buffer's duration). After EOF, feed **2 s of zero buffers** in the same format, so the latch fires and no padding tool is needed (S11).
- [ ] **Step 2: fixture,** generated rather than committed:
  `say -o "$SCRATCH/bellows.wav" --data-format=LEF32@16000 "The salt is fixed and the fire is lit"`.
  Check it with `afinfo "$SCRATCH/bellows.wav" | grep -i "data format"`, which should show 16000 Hz, 1 ch, Float32.
- [ ] **Step 3: idb client, up front.** Check `command -v idb`. If it is missing, run `pipx install fb-idb` (pairs with the installed `idb_companion`). If that is not allowed or fails, plan for the operator to tap Allow once (Step 5).
- [ ] **Step 4: run it.** This bypasses Initiation, which also opens the Bellows and would consume the injected audio (S10):
```sh
UDID=$(xcrun simctl list devices available | grep -m1 iPhone | grep -oE '[0-9A-F-]{36}')
xcrun simctl boot "$UDID" 2>/dev/null; xcrun simctl install "$UDID" /tmp/athanor-dd/Build/Products/Debug-iphonesimulator/Athanor.app
xcrun simctl privacy "$UDID" grant microphone com.damsac.athanor
DATA=$(xcrun simctl get_app_container "$UDID" com.damsac.athanor data); cp "$SCRATCH/bellows.wav" "$DATA/tmp/"
xcrun simctl spawn "$UDID" log stream --predicate 'subsystem == "com.damsac.athanor"' > "$SCRATCH/bellows.log" &
xcrun simctl launch "$UDID" com.damsac.athanor -athanor.hasCompletedInitiation YES screen=session \
  bellows-engine=apple stt-overlay=1 "bellows-audio-file=$DATA/tmp/bellows.wav"
```
- [ ] **Step 5: speech permission.** `simctl privacy` has no speech service. On the first run, take a screenshot (`xcrun simctl io "$UDID" screenshot "$SCRATCH/perm.png"`), locate Allow with `idb ui describe-all --udid "$UDID"`, tap it with `idb ui tap --udid "$UDID" X Y`, and relaunch with the Step 4 command. If there is no idb client, **STOP** and ask the operator to tap Allow once on that simulator. The grant persists for the install.
- [ ] **Step 6: pass criteria** (from `bellows.log` plus one screenshot, saved to `apps/ios/recordings/bellows-apple/`):
  - `AppleBellows started on_device=true`.
  - A DEBUG `finalized` line containing `salt`.
  - Exactly one `utterance ended` line, showing **`timeouts=0`**. This proves request 1's final arrived while request 2 was live, which also settles concurrent tasks on the simulator.
  - The screenshot shows the learner turn "The salt is fixed and the fire is lit" (or a near transcription), with the overlay's Apple rows filled in.
- [ ] **Step 7: honest-gap rule.** If the simulator reports `supportsOnDeviceRecognition == false`, the expected result is the typed fallback plus the log line `on-device recognition unavailable`. Record that as the simulator outcome, check that the fallback renders, and leave recognizer proof to Task 12. Do **not** loosen `requiresOnDeviceRecognition`. If it logs `timeouts=1` (the final never arrived while the next request ran), record it as evidence against overlap and flag F5.
- [ ] **Step 8:** smoke `bellows-engine=whisper-base` once. The overlay should show download progress, then the whisper rows, and the UI should stay responsive during the open (`idb ui swipe` over the transcript, or a screenshot during the open). Commit `ios: DEBUG audio-file injector for focus-free Bellows smoke` together with the screenshots.

## Task 11: Docs

- [ ] In `docs/agent-brief.md:8`, change "a whisper-STT Bellows" to "an on-device Bellows (Apple speech by default, whisper selectable in the dev overlay)". Add a dated entry to `docs/grimoire.md`: one paragraph on why the latch stayed in Rust for a Swift recognizer. Commit `docs: Bellows engine switch`.

## Task 12: Device measurement (OPERATOR GATE, TestFlight)

**Files:** create `docs/research/2026-11-bellows-apple-latency.md`. An agent writes the template (these steps plus an empty results table); the operator fills it in.

Protocol (operator, a real iPhone, a TestFlight build from `main` after merge):
1. **Fresh install.** The first launch shows **no** download line. The two permission prompts (mic, then speech) appear in **Initiation**, the first screen that opens the Bellows.
2. Open a session and triple-tap the mask name. The engine reads Apple.
3. **Ten utterances.** Speak 10 utterances of 5–15 words each, pausing naturally after each. Record `pickup`, `latch`, `finalize` and `felt send` for each. *Settles:* finalize latency; mid-request resets (record `resets` **and** `revisions_dropped` from the os_log, via Console.app over a cable or a sysdiagnose. A high `revisions_dropped` on short utterances is the R2 signature).
4. **Rotation.** Talk for 70 seconds continuously **without any pause of 800 ms or more** (N12); a longer pause auto-sends. Expect `requests` to rise by at least 1, with no auto-send and no lost words at the seam (around 55 s). *Settles:* the ~60 s cap, concurrent tasks, and self-close behaviour (`self_closes`).
5. **Bias terms.** Say three bias terms (an active domain name, a recent salt phrase), then say them again on W·Base. *Settles* whether `contextualStrings` helps on-device (an A/B against whisper's prompt biasing).
6. Switch to W·Base and repeat step 3 for 5 utterances. Note whether the UI froze during the open; it should not after Task 6.
7. Deny speech in Settings and relaunch. Expect the typed fallback, with no re-prompt.

**What good looks like:**
- `finalize`: p50 ≤ 300 ms, p90 ≤ **490 ms**. The measured latch is 810 ms, not 800, because of 30 ms windows (N4).
- `pickup` ≤ 500 ms.
- `felt send` ≈ latch + finalize ≤ 1.3 s.
- `timeouts` is 0 across the 10 utterances, and the rotation seam is lossless.

If `finalize` is fine but sending still feels slow, the lever is the latch (`defaultSilenceLatchMs`), not the engine. Record that and stop.

---

## Follow-ups (not on this plan's critical path)

- **F1: SpeechAnalyzer (iOS 26).** `SpeechAnalyzer` + `SpeechTranscriber` removes the request-cap question and the cumulative-string diffing: its volatile and finalized results map natively onto `previewTail`/`finalizedAppend`, and Apple claims lower latency on long-form audio. The open question is biasing. Earlier research recorded custom vocabulary as dropped. The SDK exposes an `AnalysisContext` with contextual strings, but whether `SpeechTranscriber` honours them is **UNVERIFIED**, so check that first. Task shape: a `SpeechAnalyzerBellowsController` behind `if #available(iOS 26, *)`, a fourth `BellowsEngine` case, and the same `TextBellowsHandle` with finals passed straight through. Measure it with Task 12's protocol.
- **F2: drop `whisper` from `build-ffi.sh` and `release.yml`** after a dogfood week on Apple. Wins: a smaller binary (no ggml/whisper archives or Metal shader library), faster CI cross-compiles, and no 57 MB download path. This is a separate operator decision.
- **F3: engine-agnostic LLM vocab correction.** Give the condensation pass the bias terms ("fix obvious mis-hearings"). This pays off whichever engine wins.
- **F4: audio-session interruptions** (calls, Siri, route changes). Neither engine handles them today. Use `AVAudioSession.interruptionNotification` to restart capture and cycle the request.
- **F5: sequential-cycle fallback** (only if concurrent tasks fail on device, i.e. `timeouts` on every turn). Add a `TextBellowsConfig.overlap_requests: bool`. When it is false, Rust emits `CycleRequest` as "end this one" and a later `StartRequest` once the old request closes (bounded by the timeout), accepting an audio gap at turn ends only.

## Risks

1. **The request cap and hypothesis resets are UNVERIFIED** on iOS 17 on-device. Rotation (soft at 40 s when Idle, hard at 55 s) and the reset rule are only tested against scripted text. Task 12 steps 3–4 are the real check, with `resets`, `self_closes` and `revisions_dropped` making any drift visible.
2. **Finalize latency after `endAudio()`** is the headline number and is unknown until the device run. The 600 ms timeout bounds it by committing the last hypothesis; if timeouts are common, the Rust config knob can be changed.
3. **Two permission prompts** on a fresh install. A denied speech permission gives the typed fallback even though the mic was granted. This is by design; the "no mic access" wording is out of scope (N10).
4. **Recognizer coverage on the simulator is UNVERIFIED** (Task 10 step 7), so device proof may be the only proof.
5. **Endpointing stays energy-based** for both engines. Noise above 0.02 RMS delays the latch exactly as it does today, and it is now the main felt cost.
6. **Positional commit alignment.** A later re-casing or punctuation change of a committed word is dropped (cosmetic). An *insertion* before the commit point ("the salt is" → "the sea salt is") re-emits the shifted word, which duplicates it. Both are counted in `revisions_dropped`. A smarter aligner is future work if the device logs show it matters.

   Accepted trade-offs from the re-review:
   - **R2 (drop):** a pause reset whose first post-pause partial is *not* shorter than the previous hypothesis ("the salt" → "burns bright") reads as a slot-0 revision. The held "salt" is never flushed and "burns" is dropped. Detecting resets without the shrink test would instead turn every real equal-length slot-0 revision ("the salt" → "a salt") into a duplicate. That is dup versus drop, and the rule stays until device data says otherwise. Its signature is a high `revisions_dropped` on short utterances, so Task 12 records it next to `resets`.
   - **R3 (swallow):** the overlap check treats a word restated across a reset or a revised final as already said. A genuine repeat ("…and I" then "I think…") loses its second "I". This is the right default, because restating is far likelier than true repetition.
7. **Concurrent recognition tasks are UNVERIFIED.** The design overlaps request N's final with request N+1's audio. If the device does not support this, every turn ends by timeout (still bounded at about 650 ms, but committing the last partial instead of the final). Task 10 step 6 and Task 12 detect it; F5 is the fallback.
