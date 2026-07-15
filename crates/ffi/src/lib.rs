//! The UniFFI bridge crate: the only crate in the workspace with a
//! binding-generator dependency. `athanor-core` stays UniFFI-free — every
//! `#[uniffi::export]` seam lives here, so an engine upgrade never means
//! touching the domain crate.
//!
//! Phase 0: exposes a single round-trip function so the Swift shell has a
//! real (not mocked) core call to build against before any domain logic
//! lands. See athanor-core::lib for what's next.

uniffi::setup_scaffolding!();

mod bellows;
pub mod engine;
pub mod events;
pub mod records;
pub mod session;

pub use engine::{AthanorEngine, EngineError, TierConfig};
pub use events::{SessionEvent, SessionEventListener};
pub use records::{
    FurnaceState, GrimoireGrain, HomeHeat, OpenThread, SessionDetail, SessionSummary, TendingDay,
    TranscriptTurn,
};
pub use session::SessionHandle;

#[uniffi::export]
pub fn furnace_lit() -> String {
    athanor_core::furnace_lit()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridges_to_core() {
        assert_eq!(furnace_lit(), "athanor-core: lit");
    }
}
