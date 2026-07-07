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
