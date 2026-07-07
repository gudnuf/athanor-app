import SwiftUI

// The Furnace — home. Mirrors mockups-v2.html screen 2: layered ember glow,
// big serif day count, cold-furnace copy with no streak shame, a mercury
// summary card, and a single "Tend the fire" pill. The ember bed is the one
// screen-level use of `Ember.Motion.furnaceFire` — the ambient-breathing
// exception to the "one spring family" rule (a continuous loop, not a
// discrete transition).
struct FurnaceScreen: View {
    var model: AppModel
    var onBegin: () -> Void
    var onTabula: () -> Void
    /// Turn to the Grimoire (the chamber a turn to the left).
    var onGrimoire: () -> Void = {}
    /// Turn to Mercury (the chamber a turn to the right).
    var onMercury: () -> Void = {}
    /// Begin a session in a chosen mask (lane 14: tapping a mask glyph).
    var onMask: (String) -> Void = { _ in }

    private var fire: FireState { model.engine.furnaceState() }
    private var heats: HomeHeatValues { model.engine.homeHeat() }

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text("THE FURNACE")
                    .font(Ember.F.sans(11, weight: .bold))
                    .tracking(1.6)
                    .foregroundStyle(Ember.C.muted)
                Spacer()
                Button(action: onTabula) {
                    Text(Ember.Glyph.furnace)
                        .font(.system(size: 18))
                        .foregroundStyle(Ember.C.heat)
                }
                .accessibilityLabel("Tabula — the scroll")
            }
            .padding(.horizontal, Ember.S.screenPad)
            .padding(.top, 16)

            // The living dial: the forge core breathing at the center, the eight
            // glyph doors drifting around it at their own temperatures. This IS
            // the ambient status layer — no badges, no counts (heat is the
            // notification system). It replaces the old edge-glyph margin marks.
            HomeDial(heats: heats, onTap: route)
                .frame(maxWidth: .infinity, maxHeight: .infinity)

            VStack(spacing: 4) {
                HStack(alignment: .firstTextBaseline, spacing: 6) {
                    Text("\(fire.wisdomDays)")
                        .foregroundStyle(Ember.C.heatHot)
                    Text(fire.wisdomDays == 1 ? "day tended" : "days tended")
                        .foregroundStyle(Ember.C.ink)
                }
                .font(Ember.F.serif(32, weight: .medium))
                Text(fireCopy)
                    .font(Ember.F.serif(15, italic: true))
                    .foregroundStyle(Ember.C.muted)
                if let recency = recencyLine {
                    Text(recency)
                        .font(Ember.F.sans(11.5))
                        .foregroundStyle(Ember.C.mutedDim)
                        .monospacedDigit()
                }
            }

            Button(action: onBegin) {
                Text("Tend the fire")
                    .font(Ember.F.sans(17, weight: .semibold))
                    .foregroundStyle(Color(hex: 0x1c0f04))
                    .frame(maxWidth: .infinity)
                    .frame(height: Ember.S.buttonHeight)
                    .background(
                        LinearGradient(colors: [Ember.C.heatHot, Ember.C.heatDeep], startPoint: .top, endPoint: .bottom),
                        in: Capsule()
                    )
            }
            .padding(.horizontal, Ember.S.screenPad)
            .padding(.top, 14)
            .padding(.bottom, 20)
        }
    }

    /// Routes a tapped door: the Bellows opens a session, the surfaces turn to
    /// their chamber, a mask begins a session in its voice.
    private func route(_ key: GlyphKey) {
        switch key {
        case .bellows: onBegin()
        case .mercury: onMercury()
        case .grimoire: onGrimoire()
        case .tabula: onTabula()
        case .adamas, .philosophus, .solve, .azoth: onMask(key.rawValue)
        case .furnace: break // the center core is home; the pill is the direct path
        }
    }

    private var fireCopy: String {
        if fire.tendedToday { return "the fire is warm" }
        guard let last = fire.lastTendedDay else { return "the fire is low" }
        switch daysSinceTended(last) {
        case ...1: return "the fire is warm"
        case 2...3: return "the fire is holding"
        default: return "the fire is low"
        }
    }

    /// A quiet, honest recency ground under the fire copy — "last tended
    /// yesterday · 12 min" — from the recency window. Never gamified (no streak,
    /// no shame): wisdom only comes from time, so this just names when the fire
    /// was last fed, and for how long if that's known.
    private var recencyLine: String? {
        guard let last = fire.lastTendedDay else {
            return fire.wisdomDays == 0 ? "not yet tended" : nil
        }
        let days = daysSinceTended(last)
        let when: String
        if fire.tendedToday || days <= 0 {
            when = "tended today"
        } else if days == 1 {
            when = "last tended yesterday"
        } else {
            when = "last tended \(days) days ago"
        }
        // Minutes from the most-recent tending, when the window carries them.
        if let minutes = fire.recent.first?.minutes, minutes > 0 {
            return "\(when) · \(minutes) min"
        }
        return when
    }

    private func daysSinceTended(_ last: Date) -> Int {
        Calendar.current.dateComponents([.day], from: last, to: Date()).day ?? 99
    }
}
