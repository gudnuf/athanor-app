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

/// One streamed update from a session, projected for the Swift shell.
#[derive(uniffi::Enum, Clone, Debug, PartialEq)]
pub enum SessionEvent {
    /// A chunk of the Mystagogue's reply text.
    ///
    /// `register` is the reply-register hint the plan's Session screen (E4)
    /// uses to switch between a quick sans voice and the serif reading voice.
    // TODO(core): athanor-core does not yet emit a register discriminator —
    // the Conductor/engine stream carries no quick-vs-serif signal. This field
    // is defaulted to "quick" at the bridge for every delta until core grows a
    // real register signal to project here (flag raised in the C1/C2 report).
    TextDelta { text: String, register: String },
    /// The engine invoked a Mystagogue tool this turn; `kind` is the tool name
    /// (`fix_salt`, `open_thread`, `evaporate_thread`, `kindle_passage`,
    /// `weave_domains`, `update_memory`).
    ToolCall { kind: String },
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

/// The default reply-register hint until core emits a real signal. See the
/// `TextDelta::register` TODO above.
pub(crate) const DEFAULT_REGISTER: &str = "quick";
