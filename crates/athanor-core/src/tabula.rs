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

/// The seven canonical passages, in scroll order (I→VII). Content is drawn from
/// the Tabula Athanorum (`projects/academy/TABULA_ATHANORUM.md`) and the app
/// mockups' Tabula scroll (`forge/athanor-app/mockups.html`), condensed to one
/// passage-body each for the flat surface the app renders. The kindle-key
/// mapping tracks the mockups' own milestone→passage lighting: initiation lights
/// the Furnace; first salt lights the Principles and the Grimoire; dissolution,
/// clarity, correspondence, and making light the Gates and the Ministers; a
/// cited source lights Sources; a made thing lights the World.
pub const PASSAGES: [Passage; 7] = [
    Passage {
        key: "FURNACE",
        number: "I",
        title: "The Furnace",
        body: "In the beginning the Furnace was empty, and the emptiness was the \
               first fuel. That which has nothing to burn burns itself — so the \
               student who has nothing to study studies themselves, and the Great \
               Work begins.",
        // Lit at the close of initiation (Conductor::close kindles FURNACE): the
        // learner has begun, with only themselves to burn.
        kindle_keys: &["FURNACE"],
        kindled_note: "you began with only yourself to burn",
    },
    Passage {
        key: "PRINCIPLES",
        number: "II",
        title: "The Three Principles",
        body: "Mercury the volatile, sulfur the pull, salt the fixed. The Work \
               needs all three, and the proportion shifts with every operation — \
               mercury alone is madness, salt alone a machine.",
        // The tria prima becomes true in the doing: the first fixed grain of
        // salt is the third principle proved in the learner's own hand.
        kindle_keys: &["SALT", "SULFUR", "MERCURY"],
        kindled_note: "first salt fixed — the body that remains",
    },
    Passage {
        key: "GATES",
        number: "III",
        title: "The Four Gates",
        body: "Nigredo, albedo, citrinitas, rubedo — not stages but seasons. The \
               blackening dissolves, the whitening clears, the yellowing joins \
               what was separate, the reddening makes what was not there before.",
        // A gate of transmutation passed — dissolution (Nigredo/Solve), clarity
        // (Albedo), the yellowing (Citrinitas), the reddening (Rubedo).
        kindle_keys: &["NIGREDO", "ALBEDO", "CITRINITAS", "RUBEDO", "SOLVE"],
        kindled_note: "you passed through a gate",
    },
    Passage {
        key: "MINISTERS",
        number: "IV",
        title: "The Ministers of the Work",
        body: "Mystagogue, Adamas, Azoth, Artifex, Philosophus, Solve — not \
               servants but forces. They do not teach; they transmit. One mind, \
               many registers.",
        // A Minister presided in the Work — Adamas cutting, Azoth dissolving a
        // boundary, Solve cracking the jar, the Artifex demanding a made thing.
        kindle_keys: &[
            "ADAMAS",
            "PHILOSOPHUS",
            "SOLVE",
            "AZOTH",
            "ARTIFEX",
            "MYSTAGOGUE",
        ],
        kindled_note: "a Minister spoke through the Work",
    },
    Passage {
        key: "GRIMOIRE",
        number: "V",
        title: "The Grimoire",
        body: "The mirror of the Work — written not about the Work but by it, in \
               the student's own voice. The Grimoire that is polished is dead; the \
               Grimoire that is honest is the Stone.",
        kindle_keys: &["SALT", "GRIMOIRE"],
        kindled_note: "the Grimoire began writing itself",
    },
    Passage {
        key: "SOURCES",
        number: "VI",
        title: "Sources & Verification",
        body: "Every claim that enters the Athanor must be tested — not believed, \
               not doubted, tested. A truth spoken without source is Mercury \
               unbound; it will evaporate. Trust nothing that cannot survive: how \
               do you know?",
        kindle_keys: &["SOURCES"],
        kindled_note: "a truth survived \u{201c}how do you know?\u{201d}",
    },
    Passage {
        key: "WORLD",
        number: "VII",
        title: "The World Beyond the School",
        body: "The School has no walls, because the World is the laboratory. The \
               Artifex sends you out. The School does not compete with the World — \
               it uses it, and the World, in time, uses what the School has made \
               of you.",
        // The reddening reaching outward — knowledge become act in the world.
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
            Some("first salt fixed — the body that remains")
        );
        assert!(by_key("GRIMOIRE").kindled);
        assert_eq!(
            by_key("GRIMOIRE").kindled_note.as_deref(),
            Some("the Grimoire began writing itself")
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
