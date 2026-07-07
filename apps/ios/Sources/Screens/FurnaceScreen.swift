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

    private var fire: FireState { model.engine.furnaceState() }
    private var openThreads: [Thread] { model.engine.mercury().filter { $0.state == .volatile || $0.state == .condensing } }
    private var ripeThread: Thread? { openThreads.first(where: \.isRipe) }

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
            }
            .padding(.horizontal, Ember.S.screenPad)
            .padding(.top, 16)

            Spacer()

            EmberBed(intensity: emberIntensity)
                .frame(width: 220, height: 220)

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
            }
            .padding(.top, 10)

            Spacer()

            VStack(spacing: 14) {
                if !openThreads.isEmpty {
                    MercuryRow(count: openThreads.count, ripe: ripeThread)
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
            }
            .padding(.horizontal, Ember.S.screenPad)
            .padding(.bottom, 20)
        }
    }

    /// 0...1 — the ember bed reflects held heat: more days tended, brighter
    /// coals; tended-today runs hottest. A placeholder heuristic (the real
    /// weighting is athanor-core's to decide), just never a flat constant.
    private var emberIntensity: Double {
        let base = min(Double(fire.wisdomDays) / 60.0, 1.0)
        return fire.tendedToday ? min(base + 0.25, 1.0) : base
    }

    private var fireCopy: String {
        if fire.tendedToday { return "the fire is warm" }
        guard let last = fire.lastTendedDay else { return "the fire is low" }
        let days = Calendar.current.dateComponents([.day], from: last, to: Date()).day ?? 99
        switch days {
        case ...1: return "the fire is warm"
        case 2...3: return "the fire is holding"
        default: return "the fire is low"
        }
    }
}

/// Layered radial-gradient coal, breathing at `Ember.Motion.furnaceFire`'s
/// cadence. `intensity` scales both brightness and the breathing range.
private struct EmberBed: View {
    var intensity: Double
    @State private var breathe = false

    var body: some View {
        ZStack {
            Circle()
                .fill(RadialGradient(colors: [Ember.C.heatDeep.opacity(0.32 * intensity), .clear], center: .center, startRadius: 0, endRadius: 110))
                .blur(radius: 22)
            Circle()
                .fill(RadialGradient(colors: [Ember.C.heat.opacity(0.45 * intensity), .clear], center: .center, startRadius: 0, endRadius: 78))
                .blur(radius: 14)
                .padding(30)
            Circle()
                .fill(RadialGradient(
                    colors: [Ember.C.heatCore, Ember.C.heatHot, Ember.C.heatDeep, Ember.C.heatDeep.opacity(0.35), .clear],
                    center: .center, startRadius: 0, endRadius: 40
                ))
                .blur(radius: 3)
                .padding(74)
                .opacity(0.3 + 0.7 * intensity)
            Circle()
                .fill(RadialGradient(colors: [Ember.C.heatCore, Ember.C.heatHot, Ember.C.heatDeep], center: UnitPoint(x: 0.46, y: 0.4), startRadius: 0, endRadius: 40))
                .frame(width: 58, height: 58)
                .shadow(color: Ember.C.heatCore.opacity(0.75), radius: 24)
        }
        .scaleEffect(breathe ? 1.035 : 0.975)
        .onAppear {
            withAnimation(Ember.Motion.furnaceFire) { breathe = true }
        }
    }
}

private struct MercuryRow: View {
    var count: Int
    var ripe: Thread?

    var body: some View {
        HStack(spacing: 10) {
            Text(Ember.Glyph.mercury)
                .foregroundStyle(Ember.C.muted)
            Text("\(count) \(count == 1 ? "thread" : "threads") volatile" + (ripe != nil ? " — one is ripe" : ""))
                .font(Ember.F.sans(13.5))
                .foregroundStyle(Ember.C.muted)
            Spacer(minLength: 8)
            if ripe != nil {
                Text("ripe")
                    .font(Ember.F.sans(12, weight: .semibold))
                    .foregroundStyle(Ember.C.heat)
                    .padding(.horizontal, 9)
                    .padding(.vertical, 3)
                    .overlay(Capsule().stroke(Ember.C.heat.opacity(0.4), lineWidth: 1))
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 14)
        .background(Ember.C.raised, in: RoundedRectangle(cornerRadius: 16))
        .overlay(RoundedRectangle(cornerRadius: 16).stroke(Ember.C.hairline, lineWidth: 1))
    }
}
