# Invariants

Machine-checkable and hand-checkable rules that hold across every change to
this repo. Violating one of these is a bug even if the tests pass.

## Carried from the rmp pattern (murmur-rmp)

1. **No business logic in Swift.** The SwiftUI shell renders state and
   dispatches actions; it owns none of the domain's meaning. `athanor-core`
   (via the `ffi` UniFFI bridge) is the only place session state, storage,
   prompt assembly, and tool dispatch live. A Swift file that computes a
   domain decision — not just view state — is a defect.
2. **`athanor-core` stays UniFFI-free.** Every binding-generator dependency
   lives in `crates/ffi`. Engine/binding upgrades never touch the domain
   crate.
3. **Workspace tests stay hermetic by default.** `cargo test --workspace`
   requires no network, no API key, no native model file. The `whisper`
   feature (crates/stt) and any real-API tier are feature/env-gated and
   excluded from the default CI run.

## Athanor-specific (from the product + build design specs)

4. **Salt is immutable once fixed.** A realization committed via `fix_salt`
   in the learner's own words is never edited or deleted by the system —
   only ever appended to, superseded, or (if genuinely wrong) explicitly
   retracted through a visible, logged action. Silent mutation of a fixed
   salt is a defect.
5. **Fire is append-only.** The grimoire/session-trace log never rewrites
   history. New entries only.
6. **The spiral invariant: every `fix_salt` spawns a child thread.** Each
   realization must be followed, at minimum, by one `open_thread` call. This
   is also a deterministic eval check (see the eval harness, once it lands) —
   a prompt-pack change that breaks this is a regression, not a style choice.
7. **No plausible hallucination stands in for an honest gap.** Anything the
   Mystagogue didn't actually establish with the learner renders as
   acknowledged uncertainty, never a confident invented fact — same spirit as
   the sitewalk "honest gaps" rule, applied to pedagogy instead of paperwork.

## Secrets

8. **No secret is ever read into agent context, committed, or logged.**
   `.env` is shell-sourced only. Keys, certs, `.p12`/`.pem`/`.mobileprovision`
   files are gitignored from commit zero (see `.gitignore`) and a secret scan
   runs before any push that touches history.
