//! The Tabula Athanorum as data: the app's founding scroll, adapted to seven
//! numbered passages (I–VII), plus the projection that lights each passage
//! from the learner's own kindling events.
//!
//! Design (docs/superpowers/specs/2026-07-04-athanor-app-design.md → "The
//! Tabula"): the scroll renders as a long-form serif document whose passages
//! stay dim until the learner's practice makes them true — "reading it is
//! reading a map of where your practice has and hasn't yet burned." A passage
//! lights when any of its associated concept keys has been kindled in the
//! store (`kindling` table). No new user-facing mechanics: the concept keys
//! are the ones the core already writes (`SALT` on first salt fixed; `CITRINITAS`
//! and `AZOTH` on the first cross-domain correspondence) plus any the Mystagogue
//! kindles through its `kindle_passage` tool (`NIGREDO`, `SOLVE`, `SOURCES`, …).
//!
//! The passage↔key mapping and the kindled-note wording are content decisions,
//! kept here as one small table so they are trivially tunable without touching
//! the projection logic, the FFI record, or the Swift screen.

use std::collections::HashSet;

/// One canonical passage of the scroll (static content). `kindle_keys` is the
/// set of store concept keys that light it — first match wins, order-independent.
pub struct Passage {
    /// Stable identity (never shown): the passage's own key.
    pub key: &'static str,
    /// Roman numeral shown in the scroll ("I"…"VII").
    pub number: &'static str,
    pub title: &'static str,
    pub body: &'static str,
    /// Concept keys whose presence in `kindling` lights this passage.
    pub kindle_keys: &'static [&'static str],
    /// The heat-toned caption shown ONLY once the passage is kindled.
    pub kindled_note: &'static str,
}

/// A passage projected against the learner's kindling state — the read shape
/// the Tabula surface renders. Owned (no borrows cross the store/FFI seam).
#[derive(Clone, Debug, PartialEq)]
pub struct TabulaPassage {
    pub key: String,
    pub number: String,
    pub title: String,
    pub body: String,
    pub kindled: bool,
    /// `Some` only when kindled; `None` while the passage is still dim.
    pub kindled_note: Option<String>,
}

/// The seven canonical passages, in scroll order (I→VII). Bodies match the
/// app's established Tabula copy (the `DemoEngine` reference); the alchemical
/// mapping is grounded in the scroll's own structure (`TABULA_ATHANORUM.md`).
pub const PASSAGES: [Passage; 7] = [
    Passage {
        key: "FURNACE",
        number: "I",
        title: "The Furnace",
        body: "The fire you carry, not the fire you're given.",
        // Lit when the furnace itself is named as first-kindled — the
        // Mystagogue's to strike; nothing auto-fires it, so it stays honestly
        // dim until the Work has an ember of its own.
        kindle_keys: &["FURNACE"],
        kindled_note: "the fire is lit",
    },
    Passage {
        key: "PRINCIPLES",
        number: "II",
        title: "The Three Principles",
        body: "Sulfur, mercury, salt — the pull, the volatile, the fixed.",
        // The tria prima becomes true in the doing: the first fixed grain of
        // salt proves the third principle in the learner's own hand.
        kindle_keys: &["SALT", "SULFUR", "MERCURY"],
        kindled_note: "you named the pull, the volatile, and the fixed",
    },
    Passage {
        key: "GATES",
        number: "III",
        title: "The Four Gates",
        body: "Trace, explain, predict, challenge, design.",
        // A gate of transmutation passed — dissolution (Nigredo/Solve), the
        // yellowing (Citrinitas, auto-lit by a first correspondence), the rest.
        kindle_keys: &["NIGREDO", "ALBEDO", "CITRINITAS", "RUBEDO", "SOLVE"],
        kindled_note: "you have passed through a gate",
    },
    Passage {
        key: "MINISTERS",
        number: "IV",
        title: "The Ministers",
        body: "Adamas, Philosophus, Solve, Azoth — one mind, many registers.",
        // A Minister spoke through the Work — Azoth is auto-lit by a first
        // correspondence; the others by the Mystagogue as they preside.
        kindle_keys: &[
            "ADAMAS",
            "PHILOSOPHUS",
            "SOLVE",
            "AZOTH",
            "ARTIFEX",
            "MYSTAGOGUE",
        ],
        kindled_note: "a Minister has spoken in the Work",
    },
    Passage {
        key: "GRIMOIRE",
        number: "V",
        title: "The Grimoire",
        body: "The salt shelf. A spiral staircase, not a trophy case.",
        kindle_keys: &["SALT", "GRIMOIRE"],
        kindled_note: "first salt fixed",
    },
    Passage {
        key: "SOURCES",
        number: "VI",
        title: "Sources",
        body: "A truth spoken without source is Mercury unbound.",
        kindle_keys: &["SOURCES"],
        kindled_note: "a claim carried its source",
    },
    Passage {
        key: "WORLD",
        number: "VII",
        title: "The World",
        body: "The Work never closes; it is only put down cleanly.",
        // The reddening — knowledge becoming act, the Work stepping outward.
        kindle_keys: &["WORLD", "RUBEDO"],
        kindled_note: "the Work stepped into the world",
    },
];

/// Projects the seven passages against the set of kindled concept keys
/// (`Store::kindled`). Always returns all seven, in scroll order; a passage is
/// `kindled` iff at least one of its `kindle_keys` is present, and carries its
/// note only then.
pub fn project(kindled: &[String]) -> Vec<TabulaPassage> {
    let lit: HashSet<&str> = kindled.iter().map(String::as_str).collect();
    PASSAGES
        .iter()
        .map(|p| {
            let is_kindled = p.kindle_keys.iter().any(|k| lit.contains(k));
            TabulaPassage {
                key: p.key.to_string(),
                number: p.number.to_string(),
                title: p.title.to_string(),
                body: p.body.to_string(),
                kindled: is_kindled,
                kindled_note: is_kindled.then(|| p.kindled_note.to_string()),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cold_scroll_is_all_dim_and_noteless_but_fully_legible() {
        let out = project(&[]);
        assert_eq!(out.len(), 7, "all seven passages always render");
        assert!(
            out.iter().all(|p| !p.kindled),
            "nothing kindled on a cold start"
        );
        assert!(
            out.iter().all(|p| p.kindled_note.is_none()),
            "a dim passage carries no note"
        );
        // Still legible: title/body present even while dim.
        assert!(out
            .iter()
            .all(|p| !p.title.is_empty() && !p.body.is_empty()));
    }

    #[test]
    fn passages_render_in_scroll_order_one_through_seven() {
        let out = project(&[]);
        let numbers: Vec<&str> = out.iter().map(|p| p.number.as_str()).collect();
        assert_eq!(numbers, ["I", "II", "III", "IV", "V", "VI", "VII"]);
    }

    #[test]
    fn first_salt_lights_the_principles_and_the_grimoire() {
        let out = project(&["SALT".to_string()]);
        let by_key = |k: &str| out.iter().find(|p| p.key == k).unwrap();
        assert!(by_key("PRINCIPLES").kindled);
        assert_eq!(
            by_key("PRINCIPLES").kindled_note.as_deref(),
            Some("you named the pull, the volatile, and the fixed")
        );
        assert!(by_key("GRIMOIRE").kindled);
        assert_eq!(
            by_key("GRIMOIRE").kindled_note.as_deref(),
            Some("first salt fixed")
        );
        // Untouched passages stay dim.
        assert!(!by_key("GATES").kindled);
        assert!(!by_key("WORLD").kindled);
    }

    #[test]
    fn a_first_correspondence_lights_the_gates_and_the_ministers() {
        // `weave_domains` writes CITRINITAS + AZOTH together.
        let out = project(&["CITRINITAS".to_string(), "AZOTH".to_string()]);
        let by_key = |k: &str| out.iter().find(|p| p.key == k).unwrap();
        assert!(by_key("GATES").kindled, "CITRINITAS is a gate");
        assert!(by_key("MINISTERS").kindled, "AZOTH is a Minister");
    }

    #[test]
    fn an_unknown_key_lights_nothing_and_never_panics() {
        let out = project(&["NOT_A_PASSAGE_KEY".to_string()]);
        assert!(out.iter().all(|p| !p.kindled));
    }
}
