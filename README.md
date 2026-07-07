# Athanor

A furnace you carry. The Athanor iOS app: an embedded-Goose Mystagogue, a
whisper-STT Bellows, a tria prima (salt / mercury / sulfur) learning journal.
Public from day one — see `docs/agent-brief.md` for orientation and
`docs/invariants.md` for the rules that hold across every change.

- `crates/athanor-core` — domain crate (store, session state machine, Goose
  embed, Mystagogue extension). UniFFI-free.
- `crates/ffi` — UniFFI 0.31 bridge; the only crate with a binding-generator
  dependency.
- `crates/stt` — embedded whisper STT, copied from `~/murmur-rmp/crates/stt`,
  `whisper` feature-gated (off by default).
- `apps/ios` — SwiftUI shell (xcodegen). Renders state, dispatches actions,
  owns no logic.

## Building

```sh
nix develop          # or have cargo/rustc 1.95+ on PATH some other way
just check           # fmt --check, clippy, cargo test — the hermetic tier CI runs
```

```sh
cd apps/ios && xcodegen generate
xcodebuild -project Athanor.xcodeproj -scheme Athanor \
  -destination 'platform=iOS Simulator,name=iPhone 17' build
```

## Testing

`cargo test --workspace` — hermetic: no network, no API key, no native model
file (the `whisper` feature in `crates/stt` is off by default). Real-API and
gated tiers land with the eval harness (docs/plans/).

## Secrets

`.env` is shell-sourced only, never committed, never read into an agent's
context. Keys/certs (`*.p12`, `*.pem`, `*.mobileprovision`) are gitignored
from commit zero. See `docs/invariants.md` #8. Run a secret scan before any
push that touches history.

## Docs

- `docs/agent-brief.md` — orientation for any agent picking this up cold.
- `docs/invariants.md` — rules that hold across every change.
- `docs/plans/` — implementation plans.
- `docs/research/` — spike write-ups.
- `docs/grimoire.md` — this repo's own dated realizations.
