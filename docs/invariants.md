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

## UI doctrine (operator directive, 2026-07-15)

8. **Hidden depth: capability never crowds the surface.** The visible UI
   stays as clean as it is today; new power lands behind gestures and quiet
   doors — a tap on a word that's already there, a long-press, a triple-tap,
   a subtle chip that only appears in context. Existing exemplars: tap the
   mask name (escape-hatch picker), triple-tap it (dev overlay). A feature
   that earns a new always-visible button, bar, or menu item needs explicit
   operator sign-off; the default answer is a hidden affordance. Corollary:
   hidden ≠ undiscoverable by the owner — each hidden door is recorded in
   `docs/gestures.md` so the operator can learn the full gesture vocabulary.

## Secrets

9. **No secret is ever read into agent context, committed, or logged.**
   `.env` is shell-sourced only. Keys, certs, `.p12`/`.pem`/`.mobileprovision`
   files are gitignored from commit zero (see `.gitignore`) and a secret scan
   runs before any push that touches history.
