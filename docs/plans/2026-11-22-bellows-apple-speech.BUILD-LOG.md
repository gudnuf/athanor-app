# Bellows on Apple Speech — BUILD LOG

Working on `main` directly (lead instruction: one commit per task on main, linear, push once at the end); plan Task 0's `git switch -c bellows-apple-speech` was skipped, and the plan docs were already tracked. Baseline `cargo test --workspace`: every `test result:` ok (180 core).

## Task 1 — Endpointer::in_utterance
- Red: `cargo test -p stt in_utterance` → `error[E0599]: no method named in_utterance found for struct Endpointer`.
- Green: `test endpoint::tests::in_utterance_spans_trailing_silence_until_latch_or_reset ... ok`. (The filter also matches two older `*_min_utterance_*` tests, so the line reads `3 passed`, not the plan's `1 passed`.)
- Gate: fmt=0; clippy `Finished`; workspace all `test result: ok` (stt 31 passed).

## Task 2 — committer
- Test-first slip: impl and tests written in one pass; the only red state observed was clippy (below). Tests are the plan's, verbatim.
- `cargo test -p stt text_bellows::committer`: normalize_strips_case_and_punctuation, local_agreement_commits_stable_prefix_with_holdback, final_commits_only_the_uncommitted_remainder, revised_committed_word_is_never_rewritten, reset_flushes_the_held_back_tail, reset_detected_with_one_committed_word, filler_dropping_final_is_not_duplicated, filler_dropping_longer_final_keeps_every_word, slot_zero_revision_final_without_overlap_stays_positional, empty_and_whitespace_partials_are_ignored — all `ok`; `test result: ok. 10 passed; 0 failed`.
- Deviation: the plan's `#![cfg_attr(not(test), allow(dead_code))]` still fails `clippy --all-targets -D warnings` (the lib-test target flags last_hypothesis/preview/has_seen_words as unused). Used an unconditional `#![allow(dead_code)]`; Task 3 removes it.
- Environment: ANOTHER LANE (cold-room, v1.0.8) edits ~/athanor-app concurrently: uncommitted crates/evals/*, docs/plans/2026-11-24-cold-room.md, and commit 61e9258 landed on main mid-run. I commit explicit paths only. While `crates/evals` is dirty its tests fail (pack v1 rules WIP), so the gate excludes evals (`--workspace --exclude evals`); evals are re-run at the end if the other lane has committed.
- Gate (evals excluded): fmt=0; clippy `Finished`; every test result ok.

## Task 3 — TextBellows turn machine
- Test-first slip again: impl written before the turn tests; tests are the plan's 16 (test 1 verbatim, the rest from the plan's prose asserts). All passed on first run; no red state observed.
- `cargo test -p stt text_bellows`: 10 committer + 16 turn_tests, all `ok` (latch_cycles_request_then_final_ends_utterance, final_timeout_commits_last_partial_and_ignores_late_final, send_now_cycles_without_silence_and_zeroes_latch, next_request_words_wait_behind_closing_request, soft_rotation_only_when_idle_and_does_not_end_utterance, hard_rotation_while_in_utterance, stale_request_events_are_ignored, reset_endpoint_clears_pending_latch, pickup_latency_measures_onset_to_first_partial, contextual_strings_cap_dedupe_trim, preview_concatenates_closing_tail_held_words_and_active, active_request_final_cycles_without_ending_turn, active_request_error_cycles_to_fresh_request, latch_with_no_active_request_marks_newest_closing, pickup_not_rearmed_by_interword_dip, self_close_storm_halts_to_unavailable); `test result: ok. 26 passed; 0 failed` (workspace stt line: 57 passed).
- Dead-code allowances removed from committer.rs and mod.rs; clippy `-D warnings` Finished.
- Pickup onset arm: `!was && (in_utterance || fired)` (covers a single large chunk that goes Idle→fire).
- Gate (evals excluded, other lane dirty): fmt=0; clippy Finished; all ok.

## Task 4 — FFI TextBellowsHandle
- Tests (written with the file, same slip): `cargo test -p ffi text_bellows`: handle_round_trips_latch_cycle_final ... ok, ffi_event_projects_core_event ... ok, metrics_project ... ok; `test result: ok. 3 passed; 0 failed`.
- Gate: fmt=0; clippy Finished; no FAILED anywhere in the workspace test run.
- `cargo check -p ffi --features whisper` → `Finished` (one pre-existing warning: `DEFAULT_SILENCE_LATCH_MS` unused under whisper-without-test, bellows.rs:26; not from this lane).

## Task 5 — bindings
- `zsh -l -c 'cd ~/athanor-app && nice -n 10 apps/ios/build-ffi.sh'` → exit 0, last line `==> done. Run 'cd apps/ios && ./generate.sh' next.`
- `grep -c "TextBellowsHandle|FfiTextBellowsEvent" ffi.swift` → 33. `git status` showed only ffi.swift modified from this task (xcframework gitignored).

## Task 6 — engine selection, lazy download, off-main whisper open + hash
- `generate.sh` + `xcodebuild … iPhone 17 … build` → `** BUILD SUCCEEDED **` (real); `xcodegen generate` + same → `** BUILD SUCCEEDED **` (demo); `./generate.sh` re-run to restore the real project.
- `git grep "modelTier|ModelTier.allCases|switchTier" apps/ios/Sources` → no output. `git grep -c "BellowsHandle\.open(modelPath"` → exactly 1 (BellowsController.swift, inside `Task.detached`).
- Deviation: BellowsController.swift needed `#if canImport(AthanorCoreFFI) import AthanorCoreFFI #endif` at the top (the factory body now names `AthanorCoreFFI.BellowsHandle`; first build failed with "cannot find type 'AthanorCoreFFI'"). Declarations stay FFI-free.
- Deviation (small): `bellows-engine=` override also suppresses persisting, via a static check in the `didSet`. `ModelDownloader.verify` is `nonisolated static` with the plan's signature; both call sites go through a `verifyOffMain` helper that reads the main-actor inputs then `Task.detached(priority: .utility)`.
- Overlay for non-Apple snapshots: `engine` row first, whisper rows as before; Apple snapshot shows `engine` + `send latch` only (Task 9 adds the rest). gestures.md triple-tap row updated in the same commit.

## Task 8 — speech permission string
- `./generate.sh` ok; `grep -c NSSpeechRecognitionUsageDescription apps/ios/project.yml apps/ios/Sources/App/Info.plist` → `project.yml:1`, `Info.plist:1`.
- Commit touches project.yml + Info.plist only. (Done before Task 7: the plan allows Task 8 any time, and Task 10 needs both.)

## Task 7 — AppleBellowsController
- Real build → `** BUILD SUCCEEDED **`; demo build → `** BUILD SUCCEEDED **` (project restored with generate.sh).
- Invariant #1 grep `grep -nE "hasPrefix|dropFirst|split\(|components\(" AppleBellowsController.swift` → empty (after changing the release-build word count from `split` to counting spaces).
- Shape as planned: `ingest(_:)` is the single audio entry (tap + later DEBUG injector), converter built/cached from the buffer's own format, rotation swaps the new request in before `endAudio` on the old, `.unavailable` → `.permissionDenied` + `stop()`. Refusal paths log `AppleBellows refused: …` / `on-device recognition unavailable`.
- No deviations from the plan's shape. Not yet run on a simulator (Task 10).

## Task 9 — Apple rows in the overlay
- Apple snapshot shows engine / pickup / latch / finalize / felt send / requests / timeouts (— for zero); whisper rows unchanged. gestures.md: triple-tap row extended, bed-tap row corrected to "Bank: mute the Bellows (tap again to resume)".
- Real build → `** BUILD SUCCEEDED **`.

## Task 10 — simulator smoke (focus-free)
- Injector: DEBUG-only `AppleBellowsFileInjector.swift` (extension; kept out of the controller file so the invariant-#1 grep stays empty). Real build → `** BUILD SUCCEEDED **`.
- Fixture: `say -o bellows.wav --data-format=LEF32@16000 "The salt is fixed and the fire is lit"`; `afinfo` → `1 ch, 16000 Hz, Float32`, 2.608 s.
- Run (iPhone 17 sim, booted with simctl only; no osascript, no Simulator.app): `simctl privacy grant microphone`, wav copied to app `tmp/`, `simctl launch … -athanor.hasCompletedInitiation YES screen=session bellows-engine=apple stt-overlay=1 bellows-audio-file=<tmp>/bellows.wav`.
- Speech permission: the "Athanor would like to access Speech Recognition" dialog appeared (screenshot 1). `idb ui describe-all` → `Allow {x:205,y:598.7,w:140,h:48}`; `idb ui tap 275 622` granted it; relaunched. (idb companion started with `idb_companion --udid`, client `~/Library/Python/3.9/bin/idb`.)
- **The on-device recognizer DID produce text on the simulator.** os_log, in order: `AppleBellows started on_device=true contextual=0` (seed store has no bias terms) / `bellows-audio-file: injecting … frames=41728` / `finalized: The` `salt` `is` `fixed` `and` `the` `fire` / `cycle from=1` / `finalized: is lit` / `utterance ended finalize_ms=41 latch_ms=810 timeouts=0 pickup_ms=824 requests=2 self_closes=0 resets=0 revisions_dropped=0` / `injection complete`.
- Pass criteria: on_device=true ✓; finalized line containing `salt` ✓; exactly one `utterance ended` with `timeouts=0` ✓ — request 1's final arrived while request 2 was live, so concurrent recognition tasks work on the simulator (settles the UNVERIFIED row there, NOT on device); screenshot 2 shows the learner turn "The salt is fixed and the fire is lit" and the overlay: engine apple, pickup 824 ms, latch 810 ms, finalize 41 ms, felt send 851 ms, requests 2, timeouts —.
- Caveat: pickup_ms 824 includes the injector's realtime pacing of a synthesized voice (first partial arrives ~0.8 s into speech); finalize 41 ms is a simulator number, not a device number. Task 12 remains the real measurement. `resets=0 self_closes=0 revisions_dropped=0` on this single short utterance proves nothing about the UNVERIFIED reset/cap behaviours.
- Whisper smoke: `bellows-engine=whisper-base stt-overlay=1`. The model was already in the simulator's Application Support (60 MB, from the v1.0.7 lane), so no download progress was exercised (the lazy-download code path itself is the factory's `ensureModel`, unchanged logic). Log: `Bellows opened: tier=base latchMs=800`, `decode pass=1 ms=939 window_ms=5000 rtf=0.19 gpu=false` … `pass=5 ms=944`. Overlay shows `engine whisper-base` + whisper rows (screenshot 3). Five `idb ui swipe` calls + screenshots at 4 s intervals during/after the open all succeeded (UI responsive; not a rigorous main-thread measurement).
- Screenshots: apps/ios/recordings/bellows-apple/{1-speech-permission-dialog,2-apple-turn-overlay,3-whisper-base-overlay}.png.

## Task 11 — docs
- agent-brief.md:8 now says "an on-device Bellows (Apple speech by default, whisper selectable in the dev overlay)"; grimoire.md has a dated 2026-11-23 paragraph on why the latch stays in Rust. Commit 3235f73.

## Final gate (clean detached worktree at HEAD 3235f73, so the other lane's uncommitted WIP is excluded)
- fmt=0; clippy `-D warnings` Finished; `cargo test --workspace` including evals: every `test result: ok` (core 180, stt 57, ffi 22+3+2, evals 66 passed / 1 ignored).
- Task 12 (device measurement) deliberately not done and its research template not written (lead: operator's step).

## Commits (linear on main, interleaved with the cold-room lane's `prompts:` commits)
1 712b001 · 2 52efdda · 3 4f7554d · 4 0bb275e · 5 5ba3116 · 6 f4996b7 · 8 c950681 · 7 d9ddb03 · 9 07e97cc · 10 b7c76cb · 11 3235f73 · log = last commit.
Task 0's branch was skipped (lead: commit on main).

## Smells
- Two lanes in one working tree; evals tests fail mid-edit for the other lane. I committed explicit paths only.
- The plan's Step-4 expectations of `1 passed` for the Task 1 filter was wrong (filter also matches two older tests).
- Test-first was observed red only for Task 1 (E0599) and clippy for Task 2; Tasks 2–4's tests were written with the implementation in one pass.
