//! Compiled-in prompt assets. The pack under `crates/athanor-core/prompts/` is
//! baked into the binary with `include_str!` — no runtime file IO on device,
//! and the assembled prompt is a pure function of these constants + store state.
//!
//! Content is the prompt-smith lane (iterates under evals); this module only
//! loads it and keys it by name.

pub const IDENTITY: &str = include_str!("../../prompts/identity.md");
pub const CONDENSATION: &str = include_str!("../../prompts/condensation.md");
pub const INITIATION: &str = include_str!("../../prompts/initiation.md");
pub const JUDGE: &str = include_str!("../../prompts/judge.md");

pub const MASK_PHILOSOPHUS: &str = include_str!("../../prompts/masks/philosophus.md");
pub const MASK_ADAMAS: &str = include_str!("../../prompts/masks/adamas.md");
pub const MASK_SOLVE: &str = include_str!("../../prompts/masks/solve.md");

pub const MODE_TRACE: &str = include_str!("../../prompts/modes/trace.md");
pub const MODE_EXPLAIN: &str = include_str!("../../prompts/modes/explain.md");
pub const MODE_PREDICT: &str = include_str!("../../prompts/modes/predict.md");
pub const MODE_CHALLENGE: &str = include_str!("../../prompts/modes/challenge.md");
pub const MODE_DESIGN: &str = include_str!("../../prompts/modes/design.md");

/// The v1 masks (voices). Azoth's mask is deferred; its verb ships via the store.
pub const MASK_IDS: [&str; 3] = ["philosophus", "adamas", "solve"];

/// The five work modes.
pub const MODE_IDS: [&str; 5] = ["trace", "explain", "predict", "challenge", "design"];

/// The six Mystagogue tools the engine exposes for a session (Task 9 lane).
/// Rendered as an availability line so the model knows its verbs. Kept as a
/// local constant rather than importing `Mystagogue::tool_specs()` so this lane
/// stays decoupled from the extension lane; the two lists are asserted equal
/// where they meet.
pub const TOOL_NAMES: [&str; 6] = [
    "fix_salt",
    "open_thread",
    "evaporate_thread",
    "kindle_passage",
    "weave_domains",
    "update_memory",
];

/// Returns the mask asset for a mask id, or `None` if unknown.
pub fn mask_asset(mask: &str) -> Option<&'static str> {
    match mask {
        "philosophus" => Some(MASK_PHILOSOPHUS),
        "adamas" => Some(MASK_ADAMAS),
        "solve" => Some(MASK_SOLVE),
        _ => None,
    }
}

/// Returns the mode asset for a mode id, or `None` if unknown.
pub fn mode_asset(mode: &str) -> Option<&'static str> {
    match mode {
        "trace" => Some(MODE_TRACE),
        "explain" => Some(MODE_EXPLAIN),
        "predict" => Some(MODE_PREDICT),
        "challenge" => Some(MODE_CHALLENGE),
        "design" => Some(MODE_DESIGN),
        _ => None,
    }
}
