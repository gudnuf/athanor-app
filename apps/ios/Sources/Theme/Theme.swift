import SwiftUI

// Ember design tokens — source of truth is the Ember palette in
// docs/superpowers/specs/2026-07-04-athanor-app-design.md ("Aesthetic") and
// forge/athanor-app/mockups-v2.html (the HTML/CSS mockups this mirrors).
//
// Near-black charcoal ground, incandescent orange-gold for live heat, salt
// gold reserved exclusively for condensation/grimoire moments — never used
// as a generic accent. One serif (the Mystagogue's voice), one sans (chrome).

enum Ember {

    // MARK: - Colors

    enum C {
        // Ground / surfaces (charcoal, warm-biased)
        static let ground   = Color(hex: 0x0c0a09) // near-black charcoal
        static let raised    = Color(hex: 0x171310) // raised surfaces (cards, sheets)
        static let raised2   = Color(hex: 0x211b16) // pressed / chip surface
        static let hairline  = Color(hex: 0x2b2420) // warm-biased divider

        // Ink (text)
        static let ink       = Color(hex: 0xe8e0d4) // warm off-white body
        static let muted     = Color(hex: 0x8a7f70) // muted warm grey
        static let mutedDim  = Color(hex: 0x5f574c) // dimmer still

        // Heat range — live fire, active elements (the furnace at night)
        static let heat      = Color(hex: 0xff9d3d)
        static let heatHot   = Color(hex: 0xffb15e)
        static let heatDeep  = Color(hex: 0xff8a2a)
        static let heatCore  = Color(hex: 0xffd8a0)

        /// Salt gold. RESERVED — condensation moments and grimoire accents
        /// ONLY. Never use as a general accent color; if a screen wants
        /// "accent" reach for `heat`, not `gold`.
        static let gold      = Color(hex: 0xc9a227)
        static let goldDim   = Color(hex: 0x9a7d1e)
    }

    // MARK: - Spacing / metrics

    enum S {
        static let screenPad: CGFloat = 20
        static let radius: CGFloat = 14
        static let buttonHeight: CGFloat = 56
        static let minTarget: CGFloat = 44
    }

    // MARK: - Type

    // The mockups use system faces (ui-serif/"New York" and -apple-system) —
    // no bundled font files, unlike murmur's Barlow/Source Serif set. Roles:
    // serif = the Mystagogue's voice (dialogue, koans, the Tabula); sans = UI
    // chrome (labels, buttons, metadata).
    enum F {
        /// The Mystagogue's voice — dialogue, session copy, the Tabula scroll.
        static func serif(_ size: CGFloat, weight: Font.Weight = .regular, italic: Bool = false) -> Font {
            let base = Font.system(size: size, weight: weight, design: .serif)
            return italic ? base.italic() : base
        }
        /// UI chrome — labels, buttons, nav, metadata.
        static func sans(_ size: CGFloat, weight: Font.Weight = .regular) -> Font {
            .system(size: size, weight: weight, design: .default)
        }
    }

    // MARK: - Glyphs (alchemical marks; status only, never decoration)

    enum Glyph {
        static let furnace  = "🜍" // Furnace tab / sulfur
        static let mercury  = "☿" // Mercury tab / open threads
        static let grimoire = "🜔" // Grimoire tab / salt shelf
        static let fireMask = "🜂" // fire / active mask indicator
    }

    // MARK: - Motion budget
    //
    // Spent on EXACTLY three things (rmp invariant — see the design spec's
    // "Aesthetic" section): the furnace fire, the bellows embers, and the
    // condensation moment. Everything else is calm and instant — no chrome
    // transitions, no tab-switch animation, no streaming-text fades. Screens
    // compose from this vocabulary rather than inventing their own curves;
    // if a screen wants motion for anything not named here, that's a design
    // question to raise, not a curve to write inline.
    enum Motion {
        /// Named durations — the only numbers screens should reach for.
        enum Duration {
            static let quick: Double = 0.12    // bellows embers reacting to live amplitude
            static let slow: Double = 0.9      // condensation settling into salt
            static let ambient: Double = 2.4   // furnace idle-breathing half-cycle
        }

        /// One spring family. Every discrete (non-ambient) motion in the
        /// budget is this same curve at a different duration, so nothing
        /// reads as a one-off. `dampingFraction` is fixed; only `duration`
        /// varies per named use.
        static func spring(_ duration: Double) -> Animation {
            .spring(response: duration, dampingFraction: 0.86, blendDuration: 0)
        }

        /// The Furnace screen's ember bed reflecting held heat (idle breathing).
        /// Ambient and continuous, so it stays ease-based rather than spring —
        /// springs don't loop cleanly; this is the one exception to the
        /// "one spring family" rule, and it's a loop, not a transition.
        static let furnaceFire = Animation.easeInOut(duration: Duration.ambient).repeatForever(autoreverses: true)
        /// The Bellows ember bed responding to live voice amplitude in a session.
        static let bellowsEmbers = spring(Duration.quick)
        /// The condensation moment — mercury fixing into salt (gold, once, on `fix_salt`).
        static let condensation = spring(Duration.slow)

        /// Explicit "no animation" — reach for this everywhere else (tab
        /// switches, streaming text appends, sheet content swaps). Naming it
        /// makes the restraint a decision, not an oversight.
        static let none: Animation? = nil
    }
}

extension Color {
    init(hex: UInt32) {
        self.init(
            .sRGB,
            red: Double((hex >> 16) & 0xFF) / 255,
            green: Double((hex >> 8) & 0xFF) / 255,
            blue: Double(hex & 0xFF) / 255
        )
    }
}
