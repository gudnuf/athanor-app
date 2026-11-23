# Plan Review: Bellows on Apple Speech

**Reviewer:** plan-review lane (opus)
**Date:** 2026-11-23
**Target:** `docs/plans/2026-11-22-bellows-apple-speech.md` (479 lines, 12 tasks)
**Method:** I checked every named file, type, signature and line against the tree at `aa37e5f` (`crates/stt/src/{endpoint,lib}.rs`, `crates/ffi/src/{lib,bellows}.rs`, `crates/ffi/Cargo.toml`, all of `apps/ios/Sources/Engine/*.swift`, `SessionScreen.swift`, `InitiationScreen.swift`, `FurnaceShell.swift`, `AppModel.swift`, `project.yml`, `project-real.yml`, `Info.plist`, `build-ffi.sh`, `generate.sh`, `justfile`, `.github/workflows/*`, `docs/{invariants,gestures,agent-brief}.md`). I walked the committer and turn-machine scripts by hand against the real `Endpointer` window maths. I ran `xcrun simctl help privacy` and checked which tools are on PATH locally. Apple API claims come from my own knowledge; anything I could not confirm is marked **UNVERIFIED**.

## Overall verdict: **APPROVE WITH FIXES.**

The architecture is sound. Putting the latch and the commit in Rust is the right reading of invariant #1, and reusing `stt::Endpointer` gives both engines the same latch. The window maths in every worked example is exact. The issues are in the spec, not the design. Before dispatch, fix **3 blockers**:

- **B1:** the turn machine has no rule for a final or error on the *Active* request, so the Bellows can go deaf.
- **B2:** the committer's reset rule loses words silently.
- **B3:** Tasks 6 and 7 cannot build green in the order written.

There are also 14 should-fix items, mostly facts a sonnet builder cannot get from the plan alone.

---

## 1. API, path and line verification

| Plan claim | Reality | Verdict |
|---|---|---|
| `stt::Endpointer`, `EndpointConfig { silence_ms, ..Default::default() }` | `endpoint.rs:61/10`. Defaults: `energy_threshold 0.02, silence_ms 1200, min_utterance_ms 300, window_ms 30`. `Endpointer::new(sample_rate: u32, cfg)`, `push(&[f32]) -> bool`, `reset()`, `last_latency_ms()`. `State::{Idle, Speaking{speech_ms}, TrailingSilence{..}}` is private, so `in_speech` must live in `endpoint.rs` as Task 1 says | ✅ |
| `sine` helper in endpoint tests | `endpoint.rs:216` `sine(n_samples, freq_hz, amplitude)`, plus `silence(n)` | ✅ reuse it |
| `lib.rs`: `mod` + `pub use` | `mod endpoint;` is private and re-exported (`lib.rs:6-15`). Adding `pub mod text_bellows;` plus the `pub use` works | ✅ |
| "same converted buffers the whisper path already builds" | `RealBellowsController.swift:171-203`: 4096-frame tap, `AVAudioConverter` to 16 kHz mono f32, `pushPcm`, `rms*5` amplitude | ✅ |
| `crates/ffi/src/lib.rs` add `mod text_bellows;` | `mod bellows;` is private at `lib.rs:12`, same pattern | ✅ |
| `BellowsHandle.open(modelPath:biasTerms:tier:silenceLatchMs:)` | `bellows.rs:164-177`, `#[cfg(feature="whisper")]`, returns `Result<Arc<Self>, BellowsError>` | ✅ |
| `AthanorCoreFFI.BellowsTier .smallEn / .baseEn` | `bellows.rs:84-89` `BaseEn, SmallEn` | ✅ |
| `RealBellowsController.defaultSilenceLatchMs` | `RealBellowsController.swift:48`, `static let … UInt64 = 800`, inside `#if canImport(AthanorCoreFFI)` | ✅ (only usable inside the same guard, which the plan does) |
| `RealBellowsController.init(modelPath:tier:biasTerms:) throws` | `:68` | ✅ |
| `BellowsFactory.makeRealController` is `@MainActor` | `BellowsController.swift:103-117` | ✅ |
| `SttMetricsSnapshot` fields | `lastDecodeMs, lastWindowMs, realtimeFactor, decodePasses, gpuRequested, utteranceEndLatencyMs`. **No `engine`/`pickupMs`/`latchMs`/`timeouts` yet** (Task 9 adds them, but Task 7 uses them, see B3) | ⚠️ |
| `BellowsBias.terms(engine:)` | `BellowsController.swift:88` | ✅ |
| `model.modelTier`, `"athanor.modelTier"` | `AppModel.swift:22,33`. Other refs: `SessionScreen.swift:128,502,512,517` and `InitiationScreen.swift:235` | ✅ |
| `ModelDownloader.ensureModel(tier:)`, `readyPath`, `verify(fileAt:tier:)` | `:54`, `:29`, `:155`. `verify` is a **private instance** method that reads `self.usingFixture` and `self.cachedSHA256`, and calls instance `sha256(of:)` and `sanityCheckMagicBytes(url:)` | ⚠️ S8 |
| `SessionScreen.openBellows(engine:)` / `switchTier(to:)` | Real names: `openBellows(tier: ModelTier)` `:527`, `switchTier(to:)` `:511`, `beginRealBellows()` `:501`. The 40 × 250 ms loops are at `:540-546` and `InitiationScreen.swift:225-231` | ✅ (rename as planned) |
| `SttOverlay` `ModelTier.allCases` row | `SessionScreen.swift:692-760`, `ForEach(ModelTier.allCases)` `:738`, props `tier:`, `onSwitchTier:` | ✅ |
| `RealFallbackInput` / `WarmingLine` render nothing for `.idle` | `WarmingLine` `InitiationScreen.swift:281` `case .idle, .ready: EmptyView()` | ✅ |
| `project.yml` `NSMicrophoneUsageDescription`, tracked `Info.plist` | `project.yml:51` (under `info.properties`); `Info.plist:31` is tracked and xcodegen rewrites it. `project-real.yml` includes `project.yml`, so `generate.sh` picks the key up | ✅ |
| `ffi.swift` path | `apps/ios/Packages/AthanorCoreFFI/Sources/AthanorCoreFFI/ffi.swift` exists; `Frameworks/` is gitignored (`apps/ios/.gitignore:13`) | ✅ |
| `build-ffi.sh` builds `--features goose,whisper`, refuses inside nix | `:` `build_target … --features goose,whisper`; `IN_NIX_SHELL` guard | ✅ |
| Launch arg that lands in a session, "e.g. `autonav=1`; check `FurnaceShell.swift:88`" | `autonav=1` (`:88`) **turns through surfaces on a timer and never opens a session**. The session arg is **`screen=session`** (`FurnaceShell.swift:37-39`). A fresh install also routes to **Initiation** first (`AthanorApp.swift:43`), and Initiation opens the Bellows too | ❌ S10 |
| `stt-overlay=1` | `SessionScreen.swift:71` | ✅ |
| `docs/agent-brief.md` "What this is" line 1 | `agent-brief.md:8` "a whisper-STT Bellows" (first line of the section) | ✅ |
| `docs/gestures.md` triple-tap row | `gestures.md:12` | ✅ |
| `docs/grimoire.md`, `docs/research/` | both exist | ✅ |
| Donor `~/murmur-rmp/.../TranscriptSource.swift` `SpeechSource` | exists, `:67`. Permissions at `:83-89` (`withCheckedContinuation` around `requestAuthorization`) are the part worth copying. It has no rotation and no error handling | ✅ |
| `~/athanor/forge/jefe-review/speech/diagnosis.md` | exists (about jefe, not athanor; the plan uses it as an analogy, which is fair) | ✅ |
| `justfile` | `bindgen-swift` writes to `…/AthanorCoreFFI/Generated` from a **debug host dylib**, the wrong place and the wrong library. The plan rightly uses `build-ffi.sh`; tell builders not to use `just bindgen-swift` | nit N1 |
| "Swift 5.9" | `SWIFT_VERSION: "5.9"` is the language mode; the toolchain is Swift 6.2.3. `nonisolated(unsafe)` is already in use | ✅ |

## 2. Hand-computed examples

**Window maths:** `window_ms 30` × 16 kHz gives **480 samples**, so one `sine(480,…)` push is one window. Trailing silence counts 30, 60, … and fires at the first `30k ≥ 800`, which is **k = 27 → 810 ms**.

### Committer (agree_n = 2, holdback = 1)

| Step | history | LCP | upto | emit | preview | Plan | ✓ |
|---|---|---|---|---|---|---|---|
| "the" | [the] | n/a (len < 2) | n/a | None | "the" | (None,"the") | ✅ |
| "the salt" | [the],[the salt] | 1 | 0 | None | "the salt" | same | ✅ |
| "the salt is" | [the salt],[the salt is] | 2 | 1 | "the" | "salt is" | same | ✅ |
| "the salt is fixed" | [..is],[..is fixed] | 3 | 2 | "salt" | "is fixed" | same | ✅ |
| final "The salt is fixed." | n/a | n/a | n/a | words[2..] = **"is fixed."** | n/a | same | ✅ (no revision: norm(The) = the) |
| final again | n/a | n/a | n/a | None (done) | n/a | same | ✅ |
| revised: after the 4 partials, "a salt is" | [the salt is fixed],[a salt is] | 0 | 0 | None | words[2..] = **"is"**; slot 0 a≠the, so `revisions_dropped = 1` | same | ✅ |
| reset: commit 3 (e.g. "we tend the fire","we tend the fire now" → LCP 4, upto 3), then "fire" | committed 3 > 0, len 1 < 3, fire≠we → **reset**, resets = 1 | n/a | n/a | None | "fire" | same | ✅ as specified, **but see B2** |
| final "fire burns" | lineage 0, no reset check | n/a | n/a | **"fire burns"** | n/a | same | ✅ |
| `norm("Fixed.")="fixed"`, `norm("don't")="don't"`, `norm("—")=""` | n/a | n/a | n/a | n/a | n/a | same | ✅ (but see S12, curly ’) |

Test counts: committer **6** ✅, turn_tests **11** ✅, `text_bellows` total **17** ✅, ffi **3** ✅, Task 1 filter `in_speech` matches **1** test ✅ (no existing test name contains it).

### Turn machine test 1

This assumes the helper does `now += 30` and then pushes. Only that order gives "onset at 30" in test 9; state it (N3).

- 12 voiced → now = **360**, state Speaking(360 ≥ min 300). Onset = 30.
- 26 silent → now = **1140**, silence 780 < 800, no latch ✅. The 27th → now = **1170**, fires ✅.
- `latch_ms = elapsed − last_voiced = 1170 − 360 = **810**` ✅.
- `begin_request` → 2; `requests` = **2** ✅.
- final at 1350: remainder after committed "the" is **"salt is fixed."** ✅. `finalize_ms = 1350 − 1170 = **180**` ✅.
- Pickup (not asserted) = 360 − 30 = 330. Test 9: 250 − 30 = **220** ✅.

### Other timing tests

- **Final timeout:** with `now − since ≥ 600`, `since+599` gives nothing and `since+600` fires ✅. The `tick` cadence is the 50 ms poll, so a real timeout lands at 600 to 650 ms.
- **Rotation:** 40 000 soft and 55 000 hard ✅ as arithmetic. With 50 ms poll and a main-hop slack, `endAudio` lands around 55.05 s, under 60 s.
- **Test 8:** 12 voiced + 20 silent = 600 ms trailing; reset → Idle; 10 more silent stays Idle → no cycle ✅. Without the reset, the 27th silent window would fire, so the test is meaningful.
- **Task 12 "felt send ≈ latch + finalize ≤ 1.3 s":** the measured latch is **810**, not 800, because of window granularity. So ≤ 1.3 s needs finalize p90 ≤ **490 ms**, not 500 (N4).
- **Task 10 "finalize_ms under 1000":** vacuous. A timeout caps `finalize_ms` at about 650, so it passes even if the recognizer never finals (S13).

## 3. Apple API facts

| Fact | Status |
|---|---|
| `SFSpeechRecognitionRequest.requiresOnDeviceRecognition` (iOS 13+); a request fails rather than going to the server when on-device is unavailable | ✅ |
| `SFSpeechRecognizer.supportsOnDeviceRecognition` (iOS 13+), `isAvailable` | ✅ |
| `addsPunctuation` (iOS 16+), `taskHint = .dictation`, `shouldReportPartialResults` | ✅ |
| `contextualStrings` "cap 100" | **UNVERIFIED as a hard cap.** Apple's docs advise keeping the list short; 100 is folklore. Capping is harmless. Whether contextualStrings affects **on-device** recognition at all is also **UNVERIFIED** (iOS 17's on-device lever is `customizedLanguageModel`/`SFSpeechLanguageModel`). Task 12 step 5 is the check; say so. |
| ~1-minute per-request cap for **on-device live** recognition | **UNVERIFIED.** The 1-minute limit is documented for server-based recognition. For on-device it is reported anecdotally either way. The rotation is a correct hedge, but the plan states it as fact ("under Apple's ~60 s live cap"). Reword it as a hedge. |
| `bestTranscription.formattedString` is cumulative per request; partials revise earlier words | ✅ for normal behaviour. The **mid-request reset after a pause** (the reason for the reset heuristic) is reported on recent on-device runtimes but is **UNVERIFIED** on iOS 17 specifically. |
| `isFinal` true once, after `endAudio()`/`finish()` or when the recognizer ends the task itself | ✅ for after `endAudio`. **Unprompted `isFinal` or an error on a still-fed request** (silence timeout, "no speech detected", recognizer hiccup) happens in practice, which is why B1 matters. Error codes **UNVERIFIED**. |
| `endAudio()` stops input; the recognizer finishes the buffered audio and then delivers the final; `cancel()` discards and yields an error callback | ✅ |
| Two prompts: `AVAudioApplication.requestRecordPermission()` (async, iOS 17+) and `SFSpeechRecognizer.requestAuthorization` (**callback-only**: wrap it in `withCheckedContinuation` as the donor does at `:86`); a missing `NSSpeechRecognitionUsageDescription` crashes on request | ✅ |
| Two concurrent `SFSpeechRecognitionTask`s on one recognizer (old one finishing after `endAudio` while the new one streams), on-device | **UNVERIFIED.** The whole overlap design depends on it. Add it as Risk 7 and as an explicit Task 10/12 pass item: "request N's final arrives after request N+1 has started". |
| On-device recognition on the iOS Simulator | **UNVERIFIED.** It varies by runtime and host dictation settings. The plan's honest-gap rule (Task 10 step 6) is the right handling. |
| `simctl privacy` has no speech-recognition service | ✅ **verified locally** (`xcrun simctl help privacy`: calendar, contacts, location, photos, media-library, microphone, motion, reminders, siri). Whether `grant … all` covers speech is UNVERIFIED (probably not). |

## 4. Invariants and whisper selectability

- **#1 (no business logic in Swift): holds.** Commit, latch, rotation, timeout and the contextual-string cap live in Rust. The Swift residue is capability gating and engine persistence, which is config/view state and acceptable. The Task 7 Step 3 grep is a good mechanical check. B1 must be fixed **in Rust**; do not let a builder patch it in Swift with "restart on error".
- **#3 (hermetic): holds.** `TextBellows` and `TextBellowsHandle` are ungated, and the tests use literal `now_ms` and a fake clock. No model, network or key.
- **#8 (hidden depth, gestures.md in the same commit): holds only for Task 9.** B3 forces an overlay change into Task 6, and that commit must update `gestures.md` too. No new visible UI.
- **Whisper selectable: yes.** It stays in the overlay and the `bellows-engine=whisper-*` launch arg. `BellowsHandle.open` is untouched. `build-ffi.sh` and `release.yml:221` still build `goose,whisper`. **`whisper` feature compiles:** nothing new is whisper-gated, and Task 5's `build-ffi.sh` run is the real compile proof. CI only checks hermetic features (`ci.yml:20`), so there is no host gate on `--features whisper`. Optional nit N2: add `nix develop -c cargo check -p ffi --features whisper` to the Task 4 gate.

## 5. The two diagnosis claims (Task 6)

- **`BellowsHandle.open` runs synchronously on `@MainActor`: CONFIRMED.** `BellowsFactory.makeRealController` is `@MainActor` (`BellowsController.swift:103`). It calls the synchronous `RealBellowsController.init` on an `@MainActor` class, which calls `BellowsHandle.open` (`RealBellowsController.swift:70`). Both screens call the factory from their `.task`.
- **"ModelDownloader hashes the model on main at every launch": PARTLY WRONG.** The class is `@MainActor` and `verify` is synchronous, so any hash *does* run on main. But the SHA path runs only when `cachedSHA256[tier]` is set, and that cache is in memory and filled only by `fetchExpectedSHA256` inside `download()` (`:112`). At launch with a model already on disk, the cache is empty, so `verify` does the **size band + 4-byte magic check only, which is cheap**. The real main-thread hash is **once after each fresh download** (57 or 182 MB), plus a same-process tier switch back to a tier downloaded in that process. Correct the diagnosis text. Moving it off main is still worth doing, at lower priority (S8).

## 6. Findings

### Blockers

- **B1. No rule for a final or error on the *Active* request (Task 3).**
  - *Failure:* the rules only close `Closing` requests. When the on-device recognizer ends a request by itself (unprompted `isFinal`, or an error after silence or a hiccup), `ingest_final`/`ingest_error` on the active id is unspecified. The likely builder reading is to ignore it or commit it without cycling. Either way no new request is started, so every later word goes to a dead request and **the Bellows goes deaf** with no fallback. It also happens while muted (no audio appended, so a silence error is likely).
  - *Fix:* add a rule. A final or error on the Active request commits its remainder, closes it, and queues `CycleRequest { request_id }` with `ends_turn: false`, so Swift starts a fresh request. Add turn test 12, `active_request_error_cycles_to_fresh_request` (or `final_` too), and update the 11/17 counts.
- **B2. The committer's reset heuristic loses learner words silently (Task 2).** Invariant #4/#5 spirit: "what the learner said". Three paths:
  1. **Held-back tail dropped.** A reset means the recognizer has dropped the earlier text from its cumulative string. The old lineage's uncommitted tail (at least the holdback word) is never emitted. **Fix:** on reset, first commit the previous hypothesis's `words[lineage_committed..]`. Then rewrite the reset test fixture explicitly, e.g. partials "we tend the", "we tend the fire", "we tend the fire now" (commits "we tend the"), then "burns". Expected: `Finalized("fire now")` is emitted on reset and the preview is "burns".
  2. **Undetectable with 1 committed word.** `words.len() < lineage_committed` can never be true when `lineage_committed == 1`, because empty hypotheses are ignored. The new first word is then counted as a *revision* of slot 0 and **dropped**. The same happens whenever the reset hypothesis is at least as long as the committed count. **Fix:** use `len <= lineage_committed`. Better, detect a reset when `norm(words[0]) != committed[0]` **and** the new hypothesis shares no `norm` prefix with the previous hypothesis.
  3. **False reset on final duplicates text.** Finals often drop disfluencies. Committed "um so the" followed by final "So the." meets both reset conditions, so "So the." is emitted again. **Fix:** on `on_final`, if a reset is detected, emit only the part not already covered by the committed suffix. Or skip reset detection on finals and accept revision drops. Either way, write it down and test it.

  Also record the positional-alignment limit in Risk 6. An *insertion* before the commit point ("the salt is…" becoming "the sea salt is…") re-emits "salt", giving duplicated words, not just cosmetic drift.
- **B3. Tasks 6 and 7 cannot build green as sequenced.**
  - Task 6 Step 2 deletes `modelTier` (with "grep must end empty"), but `SttOverlay` is still fed `tier: model.modelTier` and `onSwitchTier: switchTier`, and Step 6 renames `switchTier` to `switchEngine`. So the Task 6 build fails until Task 9 rewrites the overlay.
  - Task 7 Step 6 builds `SttMetricsSnapshot(engine:pickupMs:latchMs:timeouts:…)`, fields Task 9 adds, so the Task 7 build fails too.
  - *Fix:* move Task 9 Step 1 (snapshot fields, whisper fills) into Task 6. Either move the overlay engine row (Task 9 Step 2's chip row) into Task 6 together with the `gestures.md` line (invariant #8, same commit), or give Task 6 an explicit interim mapping (whisper tiers only). Task 9 then shrinks to the Apple rows.

### Should-fix

- **S1. `in_speech()` semantics break soft rotation and the pickup metric (Tasks 1 and 3).** `in_speech` is true only in `Speaking`, so any single 30 ms dip (`TrailingSilence`) between words reads as "not in speech".
  - Soft rotation at ≥ 40 s then fires **mid-utterance on any inter-word dip**, so the 55 s hard rule almost never matters, and seams land inside phrases.
  - Pickup re-arms on every dip-to-voice transition. The next partial (within ~100 ms) overwrites `pickup_ms` with a meaningless small number, and the **"pickup ≤ 500 ms" success criterion passes spuriously**.
  - *Fix:* define `in_utterance() = !Idle`. Arm pickup only on an Idle-to-non-Idle edge, and soft-rotate only when Idle. Update the Task 1 test: trailing silence counts as in-utterance until the latch or the blip abandon.
- **S2. A turn end with no Active request is unspecified.** Between a rotation's `CycleRequest` and Swift's `begin_request` (one poll plus a main hop), there is no active request. A latch or `send_now` there has nothing to mark `ends_turn`, so **`UtteranceEnded` is lost and the turn never sends**. *Fix:* in that case set `ends_turn = true` on the newest `Closing` request. Add a test.
- **S3. Where does `CycleRequest` sit relative to the ordering rule?** "drain stops at the first Closing request" would hold back a `CycleRequest` queued in that request's outbox. Test 1 needs it immediately. *Fix:* say control events go on a separate queue released first, and a Closing request's already-queued events release up to the close.
- **S4. Swift rotation order loses seam audio (Task 7 step 4).** "endAudio, then startRequest" leaves a window where the tap appends to an ended request. *Fix:* build and `beginRequest` the new request, swap `current`, *then* `endAudio()` the old one. Also, `tasks` maps id to `SFSpeechRecognitionTask`, but `endAudio()` is on the **request**. Keep `[UInt64: (task, request)]`, or call `task.finish()`.
- **S5. Task 7 state and API gaps a builder will hit.**
  - `start()` must call `startRequest()` once after permissions; it is only implied.
  - `requestAuthorization` needs the continuation wrapper.
  - On mute, skip **both** `append` and `pushPcm`, matching `RealBellowsController.swift:195`; the plan only says the append.
  - `OSAllocatedUnfairLock` over a non-Sendable request needs `init(uncheckedState:)`/`withLockUnchecked`.
  - The overlay "requests" row must read `decodePasses`, since Task 7 maps `requests` into it. Say so.
- **S6. Turn-test fixtures disagree with the prose.** The Task 3 code block uses `b.drain()` for the cycle assert, while the prose says to use `drain_no_preview` there. The FFI test 1 expectation ("CycleRequest then Finalized, UtteranceEnded") omits `Finalized("the")` and the Preview events that `poll()` will return. *Fix:* write both asserts with the filter helper. State that `Preview` is emitted only on change, and that the closing `Preview("")` is not duplicated by the concatenation rule.
- **S7. Clippy `-D warnings` will fail Task 2 and maybe Task 4.**
  - In Task 2, `Committer`, `norm` and `pub(crate) use committer::Committer` are used only by tests, so the non-test lib target (`--all-targets` builds it) warns `dead_code`/`unused_imports`. *Fix:* add `#[cfg_attr(not(test), allow(dead_code))]` (removed in Task 3), or have the stub `TextBellows` hold a `Committer`.
  - In Task 4, a test-only `with_clock` is dead code. Have `new` delegate to `with_clock`.
- **S8. `verify` cannot simply become `nonisolated static`.** It reads `cachedSHA256` and `usingFixture`, and calls two instance helpers. *Fix:* spell it out as `nonisolated static func verify(fileAt:tier:expectedSHA: String?, usingFixture: Bool) -> Bool`, make `sha256(of:)` and `sanityCheckMagicBytes(url:)` `nonisolated static`, read the cache on main first, and use `await Task.detached { … }.value` at **both** call sites (`:60` and `:136`). Also correct the diagnosis (§5).
- **S9. The Task 6 Step 7 grep cannot return "exactly one hit".** `grep -rn "BellowsHandle.open" apps/ios/Sources` already finds comments (`ModelDownloader.swift:28`, `BellowsController.swift:84`, `RealBellowsController.swift:58`), and the plan's own factory keeps `NSLog("[Athanor] BellowsHandle.open failed…")`. *Fix:* use `grep -rn "BellowsHandle\.open(modelPath" apps/ios/Sources` and expect 1.
- **S10. The Task 10 launch lands in the wrong place.** `autonav=1` never opens a session (§1). A fresh simulator install shows Initiation, which also opens the Bellows and would eat the injected audio. *Fix:* use `screen=session` plus `-athanor.hasCompletedInitiation YES`; UserDefaults reads the argument domain, so no code is needed. Also note that the permission prompts first appear in **Initiation** on a fresh device install (Task 12 step 1 says "first session").
- **S11. The Task 10 fixture tools are not available.** `ffmpeg` and `sox` are on neither the host PATH nor the devshell (checked), and only `idb_companion` is installed, **no `idb` client**. So Step 2 dead-ends and Step 4 will always STOP. *Fix:* generate with `say -o bellows.wav --data-format=LEF32@16000 "…"`, and have the DEBUG injector feed 2 s of zeros after EOF (no padding tool needed). Plan the operator tap or `pipx install fb-idb` up front. The injector also **cannot reuse the tap's converter**, which is built for `inputNode`'s format. `ingest(_:)` must build its converter from `buffer.format`.
- **S12. `norm` and Apple typography.** `formattedString` uses curly ’ (U+2019). As written, `norm("don’t")` gives "dont" while "don't" gives "don't", which creates false revisions. *Fix:* map ’ to ' before filtering, and add that assert.
- **S13. Task 10 pass criteria are too weak, and the logs leak in release.** `finalize_ms < 1000` is always true (§2). Add **`timeouts == 0`** and "request 1's final arrived". The `finalized` log line holds learner text. `Logger` redacts strings as `<private>` unless `privacy: .public`, so the grep for `salt` may fail. Making it public in release puts learner speech in device logs. *Fix:* log text `.public` under `#if DEBUG` only, and log counts and ms in release.
- **S14. Concurrent recognition tasks are an unstated dependency.** See §3. Add it to Risks and to the Task 10/12 pass list, with a fallback: if overlap fails, wait for the old final (bounded by the 600 ms timeout) before starting the next request, and accept the audio gap at turn ends only.

### Nits

- **N1.** Don't use `just bindgen-swift` (wrong output directory, debug host dylib). `build-ffi.sh` is the only regen path. Run it with `run_in_background`, because two release cross-builds can exceed a 10-minute Bash call.
- **N2.** Add `nix develop -c cargo check -p ffi --features whisper` (host) to the Task 4 gate as a fast whisper-still-compiles check before the long Task 5.
- **N3.** State the helper order (`now += 30` then `push_pcm(…, now)`), and give `turn_tests` its own `sine` (endpoint's is private to its test module).
- **N4.** The latch measures **810**, not 800. Make the Task 12 budget "finalize p90 ≤ 490" or "felt send ≤ 1.31 s".
- **N5.** Test 3 asserts `latch_ms == 0`, but with no prior latch it is 0 anyway. Latch once first so the assert proves `send_now` zeroes it.
- **N6.** The Task 6 Step 6 rewrite must keep the `debugTurnText` guard at `SessionScreen.swift:533`.
- **N7.** Task 5 Step 3: `git status` will also list the untracked plan and REVIEW files unless they are committed on the branch at Task 1.
- **N8.** The per-request state list omits `committed: Vec<String>` (the norm'd committed words), which steps 2 and 4 read. List it.
- **N9.** During a closing request, request N+1's held `Finalized` words drop out of the shimmer until request N closes, so they briefly flicker. Consider including them in the concatenated preview.
- **N10.** `micDenied` renders "no mic access — type instead" (`SessionScreen.swift:1097`) for speech-denied and unsupported devices as well. Wording only; out of scope.
- **N11.** Pre-existing doc drift: `gestures.md:13` says the bed tap "sends what was heard", but the code mutes (`SessionScreen.swift:1090`). Fix it while touching the file in Task 9.
- **N12.** Task 12 step 4: a 70 s monologue with any pause of 800 ms or more auto-sends. Ask the operator to talk through without pausing. With S1 fixed, rotation will usually fire at the 55 s hard bound.

## 7. Sequencing and builder safety

- **Order:**
  1. **Tasks 1–5 are serial** (1 and 2 could run in parallel worktrees, since they touch only `endpoint.rs` and `text_bellows/`+`lib.rs`; 3 needs both; 4 needs 3; 5 needs 4).
  2. **Task 6 can start in parallel with 1–5**, because it uses no new FFI type, provided B3's fields and overlay work move into it.
  3. **Task 8 and Task 11 can run any time, in parallel.**
  4. **Task 7 needs 5 and 6.**
  5. **Task 9's remainder needs 7.**
  6. **Task 10 needs 7, 8 and 9.**
  7. **Task 12 is the operator gate.**
- **xcframework regen placement: correct.** Task 5 (`build-ffi.sh` from a login shell, which regenerates `ffi.swift`) comes before Task 7, the first Swift use of `TextBellowsHandle`/`FfiTextBellowsEvent`. Task 6 needs no regen.
- **Facts a sonnet builder can't get from the plan:**
  - B1–B3, S2 and S3 (spec holes).
  - S8 (the real `verify` dependencies).
  - S10 and S11 (launch arg, initiation bypass, missing tools).
  - S7 (the clippy trap).
  - The full partial sequences for the committer tests marked `// ...`: B2 changes the reset one; the others derive from the worked example.

  With those fixes folded in, every task is executable from the plan alone.

---

## Re-review 2026-11-23 (revised plan, 755 lines)

Scope: B1, B2, B3 and the `in_utterance` change (S1), as the lead asked. I hand-walked everything again against the same tree (`aa37e5f`).

### B1: self-close on the Active request. **RESOLVED.**

Rule 3 commits the remainder, marks the request `Closed`, increments `self_closes`, and queues `CycleRequest` without ending the turn. Swift's S4 order (`startRequest()`, then `live[id]?.request.endAudio()`) is a safe no-op when the recognizer already ended the request itself.

- **Test 12:** "the" is committed first. `final "the salt is"` gives lc 1, len 3 = prev 3, so no reset, and it emits `words[1..]` = **"salt is"**. Drain order is control first, so the drain is `[CycleRequest{1}, Finalized("salt is")]` ✅.
- **Test 13:** "we tend the fire" twice gives LCP 4, upto 3, committing "we tend the". The error runs `on_final(last_hypothesis)`, and the remainder is **"fire"**, giving `[CycleRequest{1}, Finalized("fire")]` ✅.
  - *nit:* both tests must drain after the committing partials. Otherwise the drain also holds the earlier `Finalized`. Say "drain after the partials".
- **New should-fix R4: no backoff on a self-close storm.** If every new request errors at once (missing on-device assets, speech revoked mid-session, recognizer unavailable), each poll gives error, then `CycleRequest`, then a new request. That is a 20 Hz request churn with a live mic and no fallback.
  - *Fix (Rust):* if N (say 3) consecutive requests self-close with no non-empty partial within 1 s of `begin_request`, stop cycling. Emit a new terminal event (or reuse an error path) that Swift maps to `.permissionDenied` (typed fallback).
  - Add one turn test for it.

### B2: committer reset. **RESOLVED for the three reported paths.**

All 8 committer cases, walked with `Committer::new(2,1)`:

| # | Case | Walk | Expected | Plan | ✓ |
|---|---|---|---|---|---|
| 1 | normalize | ’ is mapped to ', then lowercase and filter | "fixed", "don't", "don't", "" | same | ✅ |
| 2 | local agreement | LCP 1, 2, 3 → upto 0, 1, 2 | (None,"the"), (None,"the salt"), (Some "the","salt is"), (Some "salt","is fixed") | same | ✅ |
| 3 | final remainder | lc 2, len 4 = prev 4, no reset, no revision | Some "is fixed.", then None | same | ✅ |
| 4 | revision | "a salt is fixed": len 4 = prev 4, so no reset; slot 0 a≠the gives revisions 1; LCP 0, no commit | (None,"is fixed"), revisions 1, resets 0 | same | ✅ |
| 5 | flush held tail | "we tend the" → None; "…fire" LCP 3, upto 2 → "we tend"; "…fire now" LCP 4, upto 3 → "the"; "burns": 1 < 5, burns≠we, so reset, flush `prev[3..]` = "fire now", overlap k 0; final with lc 0 → "burns brightly" | as walked | same | ✅ |
| 6 | 1 committed word | "the salt is" → "the" (lc 1); "burns": 1 < 3, slot 0 burns≠the, so reset, flush "salt is", k 0 | (Some "salt is","burns"), (1,0) | same | ✅ |
| 7 | filler-dropping final | "um so the fire" twice → "um so", then "the" (lc 3); final "So the fire.": 3 < 4, slots so/um, the/so, fire/the all mismatch, so reset, flush "fire"; lineage_text [um so the fire]; k 3 ([so the fire]); emit "fire" + "" | Some "fire" | same | ✅ (*nit:* the final's punctuation "fire." is lost, because the flushed partial surface form wins. Cosmetic, Risk 6.) |
| 8 | empty/whitespace | `w("")` and `w("   ")` are empty, so ignored | (None,"the salt") ×2 | same | ✅ |

Counts: committer **8**, turn **15**, `text_bellows` **23** (8 + 15), ffi **3** ✅. The Task 1 filter `in_utterance` matches 1 test ✅.

**Remaining drop and repeat paths:**

- **R1 should-fix: a filler-dropping final that does *not* shrink still drops a word.**
  - *Example:* committed `[um, so]`, prev "um so the" (3), final "So the fire is." (4, not < 3), so there is no reset. Slot 0 mismatches (revision), and the positional emit is `words[2..]` = "fire is.", so **"the" is lost**. The case is realistic, because a final usually also covers the last ~100 ms of audio after the last partial.
  - *Fix (one sentence plus one test):* in `on_final`, when the revision check fires, align by the same overlap `k` against `lineage_text` instead of by `lineage_committed`. Here `k` = 1 ([so]), so it emits "the fire is.". Case 3 is unchanged (k = 2 gives "is fixed.").
  - Add `filler_dropping_longer_final_keeps_every_word`.
- **R2 (nit, document in Risk 6): a pause reset whose first partial is not shorter than prev.**
  - *Example:* prev "the salt" (lc 1), and after the pause the first partial is "burns bright" (2, not < 2). That reads as a slot-0 revision, so the held "salt" is never flushed and "burns" is dropped.
  - Detecting it without shrink would turn a real equal-length slot-0 revision ("the salt" → "a salt") into a duplicate "salt". That is a dup-versus-drop trade-off, and I would not change the rule without device data.
  - Record it as an accepted limit. Have Task 12 note `revisions_dropped` next to `resets`, since a high revisions count on short utterances is the signature.
- **R3 (nit, document): overlap can swallow a genuine repeat.** Lineage "…and I" followed by new speech "I think…" gives k 1, so the second "I" is treated as restated. This is the right default (restating is far likelier), but say so in Risk 6.
- No other path found. Insertion before the commit point (duplicate) was already accepted in Risk 6.

### B3: build order. **RESOLVED.**

Every call site that Task 6 touches is listed in Task 6: `AppModel.swift:22-40`, `SessionScreen.swift:74/126-131/502/511-521/549/697/740`, `InitiationScreen.swift:234-235`, `BellowsController.swift:106`, and the only `SttMetricsSnapshot(` constructor at `RealBellowsController.swift:83`.

- **Task 6:** adds the four snapshot fields, the overlay `engine` props and row, and `gestures.md` (same commit, #8). Its verify greps (`modelTier|ModelTier.allCases|switchTier`, `BellowsHandle\.open(modelPath` = 1) will hold.
- **Task 7:** constructs the snapshot with fields that now exist, and needs only the Task 5 bindings.
- **Task 9:** is pure rendering plus docs.

So 6 → 7 → 9 builds green as written. Two small notes:

- *nit:* `currentMetrics()` is `nonisolated` on both controllers. Declare the stored `tier` as `nonisolated let`; `ModelTier` is Sendable. Also make `AppleBellowsController.currentMetrics()` and the `rms` helper `nonisolated`.
- The demo build is unaffected, because `BellowsEngine` and the overlay are FFI-free.

### S1: `in_utterance`, tests 14 and 15. **RESOLVED.**

- **Task 1 test:** 12 voiced, then 3 silent (TrailingSilence, still in the utterance), then 24 more silent. The 27th window is 810 ≥ 800 with speech 360 ≥ 300, so it fires and returns to Idle. Then 12 voiced and `reset()` gives Idle ✅.
- **Test 5:** a 3-window blip (90 < 300) plus 27 silent abandons the blip and returns to Idle. `tick(39_999)` gives `[]` and `tick(40_000)` gives `CycleRequest{1}`. The final gives `[Finalized("hm")]` with no `UtteranceEnded` ✅.
- **Test 6:** TrailingSilence at 40 000 blocks the soft rule. The hard rule fires at 55 000 ✅.
- **Test 14:** a soft rotation leaves request 1 `Closing{ends_turn:false}`. The latch before `begin_request` sets `ends_turn = true`, `since = now`, with no second `CycleRequest`. The final gives `[Finalized, UtteranceEnded]` ✅.
- **Test 15:** onset is armed only on the Idle→non-Idle edge. Onset 30, partial 250 gives 220. A dip stays non-Idle, so there is no re-arm and the value stays 220 ✅.
- **Test 1:** pickup 360 − 30 = **330** ✅. The drain after the latch emits no Preview, because the oldest non-Closed request is still request 1 with an unchanged "salt is" ✅.
- **Test 11:** "is fixed" + held "next" + "words" ✅.
- **Test 4:** iteration stops after the first non-Closed request, so request 2's held "next words" waits, then drains after request 1's `UtteranceEnded` ✅.

### Re-review verdict: **APPROVE.**

All three blockers and S1 are resolved, and the arithmetic in every revised test checks out. Fold in two should-fix items, each a one-line plan edit plus one test, without another review round:

- **R1** (overlap alignment in `on_final` on revision), in Task 2.
- **R4** (self-close storm backoff to the typed fallback), in Task 3, with the Swift mapping in Task 7.

R2, R3 and the test-drain nit are documentation only.
