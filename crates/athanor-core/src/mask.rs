//! The session's live mask/mode register — shared, mutable, one per session.
//!
//! Lane 13 makes mask shifting *fluid* and Mystagogue-driven: the model wears
//! the mask that fits the moment (adamas presses, philosophus midwifes, solve
//! breaks a frame) and shifts it quietly via the `shift_mask` tool. The chosen
//! pair is not a fixed opening choice — it moves across a session.
//!
//! So the current `(mask, mode)` can't live as a frozen field on the
//! `Conductor`: the `shift_mask` tool (which runs mid-turn, inside
//! `ToolDispatch`) and the `Conductor` (which re-assembles the prompt each turn)
//! and the FFI bridge (which surfaces the honest header + the pin escape hatch)
//! all need the same, current value. [`MaskState`] behind an `Arc<Mutex<_>>` is
//! that one shared cell.
//!
//! **Pinning** is the subtle escape hatch: if the learner taps the header and
//! chooses a mask, it `pinned`s — the `shift_mask` tool then no-ops (and tells
//! the model the learner has chosen this register), so the human's choice wins
//! for the rest of the session.

use std::sync::{Arc, Mutex};

/// The current voice/work-mode of a session, plus whether the learner has
/// pinned it. Shared across the Conductor, the `shift_mask` tool, and the FFI
/// bridge — always read/written through [`SharedMask`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaskState {
    pub mask: String,
    pub mode: String,
    /// True once the learner has chosen a mask via the header escape hatch —
    /// `shift_mask` no-ops while pinned.
    pub pinned: bool,
}

/// The one shared, mutable cell of a session's `(mask, mode, pinned)`.
pub type SharedMask = Arc<Mutex<MaskState>>;

/// Builds a fresh shared cell at the session's opening `(mask, mode)`, unpinned.
pub fn shared(mask: &str, mode: &str) -> SharedMask {
    Arc::new(Mutex::new(MaskState {
        mask: mask.to_string(),
        mode: mode.to_string(),
        pinned: false,
    }))
}

/// Reads the current `(mask, mode)` out of a shared cell.
pub fn current(state: &SharedMask) -> (String, String) {
    let s = state.lock().unwrap();
    (s.mask.clone(), s.mode.clone())
}

/// Whether the learner has pinned the mask.
pub fn is_pinned(state: &SharedMask) -> bool {
    state.lock().unwrap().pinned
}

/// Pins the mask to the learner's choice (the header escape hatch). Sets the
/// mask, marks it pinned; leaves the mode as-is (pinning is a voice choice).
pub fn pin(state: &SharedMask, mask: &str) {
    let mut s = state.lock().unwrap();
    s.mask = mask.to_string();
    s.pinned = true;
}
