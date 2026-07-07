//! FFI-facing session event projections (Plan Phase 4, Task C2). Thin
//! `uniffi::Enum`/`uniffi::Record` dictionaries over `athanor-core`'s ACP seam
//! types — never a goose type, never `serde_json::Value` across the boundary.
//!
//! Two of these variants are **bridge-synthesized** (review edit #4): the core
//! `AcpUpdate` stream has only `TextDelta`/`ToolCall`/`TurnComplete`. The
//! bridge derives:
//! - `Condensation` from observing a `fix_salt` tool call during a turn and
//!   reading the newly-fixed realization back out of the store (see
//!   `session.rs`);
//! - `Error` from a `run_turn` that returns `Err` (never a panic across FFI).

/// Which voice a reply run is spoken in — projected across FFI from
/// `athanor_core::engine::Register`. The Session screen (E4) switches between
/// the quick conversational sans voice and the serif reading voice on this.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplyRegister {
    /// The conversational default — quick sans note.
    Quick,
    /// The reading voice — a deeper lesson, rendered as calmer, larger serif.
    Reading,
}

impl From<athanor_core::engine::Register> for ReplyRegister {
    fn from(r: athanor_core::engine::Register) -> Self {
        match r {
            athanor_core::engine::Register::Quick => ReplyRegister::Quick,
            athanor_core::engine::Register::Reading => ReplyRegister::Reading,
        }
    }
}

/// One streamed update from a session, projected for the Swift shell.
#[derive(uniffi::Enum, Clone, Debug, PartialEq)]
pub enum SessionEvent {
    /// A chunk of the Mystagogue's reply text, tagged with the register it is
    /// spoken in. The core's `Conductor` parses the model's reading-voice
    /// markers (identity.md §6), strips them, and tags each run — so `register`
    /// here is a real signal from core, never a bridge default.
    TextDelta {
        text: String,
        register: ReplyRegister,
    },
    /// The engine invoked a Mystagogue tool this turn; `kind` is the tool name
    /// (`fix_salt`, `open_thread`, `evaporate_thread`, `kindle_passage`,
    /// `weave_domains`, `update_memory`).
    ToolCall { kind: String },
    /// **Bridge-synthesized.** The session's current `(mask, mode)` register
    /// (lane 13) — emitted at the top of a turn whenever it differs from what was
    /// last surfaced: the opening pair on the first turn, and the new pair on the
    /// first turn after a `shift_mask` (or a learner pin). Drives the honest
    /// Session-screen header.
    MaskShifted { mask: String, mode: String },
    /// A salt was fixed this turn — the condensation moment. Derived from the
    /// `fix_salt` tool's own `AcpUpdate::ToolResult` (its real `realization_id`
    /// and spiral `child_thread_id`), and carrying the fixed salt's TEXT so the
    /// Session screen can render the gold moment directly, without a second
    /// store read. Falls back to the newest grain only if a result is missing.
    Condensation {
        realization_id: String,
        child_thread_id: Option<String>,
        text: String,
    },
    /// The turn reached its natural end (`AcpUpdate::TurnComplete`).
    TurnComplete,
    /// **Bridge-synthesized.** The turn failed (`run_turn` returned `Err`).
    /// Surfaced instead of unwinding across the FFI boundary.
    Error { message: String },
}

/// Foreign-implemented per-session listener — `with_foreign`, never
/// `callback_interface` (the boxed-trait-object-as-parameter shape fails to
/// compile under uniffi 0.28+: mozilla/uniffi-rs#2797). Stored/passed as
/// `Arc<dyn SessionEventListener>`.
#[uniffi::export(with_foreign)]
pub trait SessionEventListener: Send + Sync {
    fn on_event(&self, event: SessionEvent);
}
