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
/// The close-only distillation prompt (`Conductor::condense`): asks the
/// Mystagogue to look back over the whole exchange and set down the durable
/// session note + any warranted profile refinements.
pub const CONDENSE: &str = include_str!("../../prompts/condense.md");

pub const MASK_PHILOSOPHUS: &str = include_str!("../../prompts/masks/philosophus.md");
pub const MASK_ADAMAS: &str = include_str!("../../prompts/masks/adamas.md");
pub const MASK_SOLVE: &str = include_str!("../../prompts/masks/solve.md");

/// The delimiter `initiation.md` wraps its ritual-opening-turn marker in
/// (`<!-- ritual-opening-turn: ... -->`). Kept as a parsing convention here so
/// the marker's actual *content* — what the engine seeds as the synthesized
/// learner-arrival turn — lives entirely in the versioned prompt pack, not as
/// a string hardcoded in Rust (BLOCKER-1 deep fix: initiation has no other
/// first-speaker channel, so the Conductor's `open_turn` needs *something* to
/// seed as the driving turn; this is that something).
const RITUAL_OPENING_MARKER_PREFIX: &str = "<!-- ritual-opening-turn: ";
const RITUAL_OPENING_MARKER_SUFFIX: &str = " -->";

/// Extracts the ritual-opening-turn marker text from `initiation.md`. Panics
/// if the marker is missing or malformed — this is compiled-in prompt-pack
/// content, not user input, so a missing marker is a build-time authoring bug
/// that should fail loudly (mirrors the rest of this module's total-over-
/// compiled-assets discipline).
pub fn initiation_opening_turn() -> &'static str {
    let start = INITIATION
        .find(RITUAL_OPENING_MARKER_PREFIX)
        .expect("initiation.md must define a <!-- ritual-opening-turn: ... --> marker")
        + RITUAL_OPENING_MARKER_PREFIX.len();
    let rest = &INITIATION[start..];
    let end = rest
        .find(RITUAL_OPENING_MARKER_SUFFIX)
        .expect("initiation.md's ritual-opening-turn marker must be closed with ` -->`");
    &rest[..end]
}

pub const MODE_TRACE: &str = include_str!("../../prompts/modes/trace.md");
pub const MODE_EXPLAIN: &str = include_str!("../../prompts/modes/explain.md");
pub const MODE_PREDICT: &str = include_str!("../../prompts/modes/predict.md");
pub const MODE_CHALLENGE: &str = include_str!("../../prompts/modes/challenge.md");
pub const MODE_DESIGN: &str = include_str!("../../prompts/modes/design.md");

/// The v1 masks (voices). Azoth's mask is deferred; its verb ships via the store.
pub const MASK_IDS: [&str; 3] = ["philosophus", "adamas", "solve"];

/// The five work modes.
pub const MODE_IDS: [&str; 5] = ["trace", "explain", "predict", "challenge", "design"];

/// The Mystagogue tool names, in order, derived from the extension's real
/// specs. Deriving (rather than a local const) means the assembled
/// tool-availability line can never drift from the tools actually dispatched.
pub fn tool_names() -> Vec<String> {
    crate::Mystagogue::tool_specs()
        .into_iter()
        .map(|spec| spec.name)
        .collect()
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initiation_opening_turn_is_defined_and_nonempty() {
        let marker = initiation_opening_turn();
        assert!(!marker.trim().is_empty());
        assert_eq!(marker, "[the learner arrives]");
        // the marker text is also visible in the assembled asset verbatim
        // (not just parsed out of it) — anyone reading initiation.md sees
        // exactly what the engine seeds.
        assert!(INITIATION.contains(marker));
    }
}
