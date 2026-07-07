//! athanor-core — the furnace's engine room.
//!
//! Owns the tria prima SQLite store + migrations (`store`) and domain types
//! (`domain`). The design
//! (docs/superpowers/specs/2026-07-06-athanor-app-goose-build-design.md,
//! carried in the meta repo ~/athanor) calls for this crate to eventually also
//! own: the session state machine, the embedded Goose engine, the Mystagogue
//! extension (fix_salt / open_thread / evaporate_thread / kindle_passage /
//! weave_domains / update_memory), and prompt assembly — see docs/plans/ for
//! the build sequence.

pub mod conductor;
pub mod domain;
pub mod error;
pub mod ids;
pub mod mystagogue;
pub mod prompt;
pub mod register;
pub mod session;
pub mod store;
pub mod tabula;

pub use conductor::{Conductor, ConductorError, ConductorOutcome};
pub use error::CoreError;
pub use mystagogue::Mystagogue;
pub use session::{abandon_session, close_session};
pub use store::Store;

pub mod engine;

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
