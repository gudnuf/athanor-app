//! athanor-core — the furnace's engine room.
//!
//! Phase 0 (repo genesis): this crate is a placeholder. The design
//! (docs/superpowers/specs/2026-07-06-athanor-app-goose-build-design.md,
//! carried in the meta repo ~/athanor) calls for this crate to eventually own:
//! the tria prima SQLite store + migrations, the session state machine, the
//! embedded Goose engine, the Mystagogue extension (fix_salt / open_thread /
//! evaporate_thread / kindle_passage / weave_domains / update_memory), and
//! prompt assembly. None of that lands until the spike gates (Goose-on-iOS,
//! Whisper-on-iPhone) are green — see docs/plans/ for the build sequence.
//!
//! Until then this crate exists to give the workspace, the UniFFI bridge, and
//! CI something real to build and test against.

/// Returns the crate's own identity string. A trivial, stable UniFFI-exportable
/// function so the FFI bridge and the SwiftUI shell have a real round-trip to
/// build against before any domain logic exists.
pub fn furnace_lit() -> String {
    "athanor-core: lit".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn furnace_lit_reports_identity() {
        assert_eq!(furnace_lit(), "athanor-core: lit");
    }
}
