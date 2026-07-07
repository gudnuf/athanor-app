# Agent brief

Orientation for any agent (planner, builder, reviewer) picking up work in this
repo cold.

## What this is

The Athanor iOS app: an embedded-Goose Mystagogue, a whisper-STT Bellows, a
tria prima (salt/mercury/sulfur) learning-journal store, rendered with an
Ember aesthetic. Product spec and machine architecture live in the meta repo,
`~/athanor` (`docs/superpowers/specs/2026-07-04-athanor-app-design.md` +
`2026-07-06-athanor-app-goose-build-design.md`). This repo (`athanor-app`) is
**public from day one** — publish-the-process — and carries its own
`docs/plans/`, `docs/research/`, and `docs/grimoire.md`.

## Layout

- `crates/athanor-core` — the domain crate. Owns (once built out): the tria
  prima SQLite store + migrations, the session state machine, the embedded
  Goose engine, the Mystagogue extension's tools, prompt assembly. UniFFI-free
  (invariant #2, docs/invariants.md).
- `crates/ffi` — the UniFFI 0.31 bridge. The only crate with a
  binding-generator dependency. Proc-macro mode; no build.rs, no UDL.
- `crates/stt` — copied (not path-depended) from `~/murmur-rmp/crates/stt`:
  whisper-rs-backed streaming STT, `whisper` feature-gated so default
  workspace tests stay hermetic. The diff between this copy and its origin is
  itself factory evidence — see the meta repo's FACTORY.md.
- `apps/ios` — SwiftUI shell (xcodegen-managed `project.yml`). Renders state,
  dispatches actions, owns no logic.
- `docs/plans/` — implementation plans, one per numbered increment.
- `docs/research/` — spike write-ups (Goose-on-iOS, Whisper-on-iPhone, etc).
- `docs/grimoire.md` — this repo's own dated realizations, in the spirit of
  the product it builds.

## Sequencing (see the build design spec for full detail)

1. Spike gates — Goose-on-iOS, Whisper-on-iPhone. Neither has run yet as of
   repo genesis (2026-07-06).
2. Core skeleton — tria prima store, session state machine, Goose embed,
   Mystagogue extension, prompt assembly, a thin dev CLI.
3. Prompt pack + eval harness, matured together against the dev CLI.
4. First shippable, voice-first: Furnace, Initiation, Session with the
   Bellows, condensation → Grimoire, Mercury list.
5. Daily tending: dogfood, vault export, Tabula surface.

## Conventions

- Rebase + fast-forward merge; linear history. No `Co-Authored-By` footers.
- Commits are meaningful and incremental, not one giant genesis blob.
- Every plan/report/review persists to `docs/` as a file — chat is not the
  record.
- Secrets discipline is constitutional here (public repo): see
  `docs/invariants.md` #8 and `.gitignore`.
