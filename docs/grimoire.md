# Grimoire

Dated, one-line-minimum realizations about this app and its own build
process. Append-only (invariant #5, docs/invariants.md) — new entries go at
the bottom.

- **2026-07-06** — Repo genesis. Scaffolded from the murmur-rmp / sitewalk
  rmp pattern: Cargo workspace (`athanor-core` + `ffi` + `stt`), UniFFI 0.31
  bridge, xcodegen-managed SwiftUI shell, Nix devshell, `just` commands.
  `crates/stt` copied verbatim from `~/murmur-rmp/crates/stt` @ `af4afe1`
  (not path-depended); `whisper` feature stays off by default so
  `cargo test --workspace` is hermetic. No product logic yet — both spike
  gates (Goose-on-iOS, Whisper-on-iPhone) are still ahead.

## 2026-07-07 — the overnight sprint: genesis to a living MVP

One night, one meta-agent, ~20 builder/reviewer lanes, ~30 ff-merges. Both
spike gates went GREEN (embedded goose v1.41.0 in-process on iOS; whisper
base.en at RTF 0.0088 on the host Metal path), and everything through the
reviewed Phase-4 plan is on main: tria prima store, Conductor, the six
Mystagogue tools, ingested prompt pack + structure-locking snapshots, the
persona eval harness, dev CLI, UniFFI surface, xcframework build system, and
the full Ember SwiftUI shell. The whole arc — launch → initiation → voice
pipeline → live Anthropic turn → streamed reply — ran on the simulator and
was recorded. The Mystagogue now carries its own dialogue history (proven by
asking it to remember an invented word across turns) and opens the initiation
itself, ritual marker versioned in the prompt pack. A lived-in seed translates
the operator's real academy markdown through the store's own APIs, so the
Furnace lights with real wisdom-days (the seed data itself never enters this
public repo).

What held: fail-closed integration gates on test exit codes; verifying every
lane through the devshell rather than ambient toolchains; a final cross-module
review that caught a demo string one tap away from being the learner's first
words to live Claude. What bit: a mid-rebase recovery silently dropped a
merged test suite from main's ancestry (caught by a later builder's honest
"this file doesn't exist" — restored); the sim's software Metal traps ggml
(stt now runs CPU-only under target_abi=sim); and build-ffi.sh must be run
from a login shell, which it now enforces itself.

Remaining are the operator gates: device install, on-phone RTF, the first
live device turn, and the taste-check that only the learner can perform.
