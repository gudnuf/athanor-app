import SwiftUI

// The living home screen (lane 14): a breathing forge core at the center with
// the eight glyph doors drifting around it in two elliptical rings, each at its
// own COMPUTED heat. Ported from the operator-approved gallery's Dial-mode
// composition (orbit radii/speeds/sizes, the radial-bloom core). Tap a door: it
// flares and navigates. All of this lives inside the `furnaceFire` ambient
// motion exception; reduced motion stops the drift/breathing but still renders
// each door at its true temperature.

struct HomeDial: View {
    /// Per-door heat, computed in core (never invented here).
    let heats: HomeHeatValues
    /// Tapping a door — the caller routes it (surface turn / begin a session).
    var onTap: (GlyphKey) -> Void

    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    /// When each door was last tapped, for the decaying flare bump.
    @State private var flareStart: [GlyphKey: Date] = [:]

    // MARK: Orbit table (ported from the gallery's `orbits`)

    private struct Orbit {
        let key: GlyphKey
        let a0: Double
        let rr: Double
        let speed: Double
        let size: Double
    }

    /// HOME order: the eight doors (the Furnace itself is the center core).
    private static let order: [GlyphKey] =
        [.bellows, .mercury, .grimoire, .tabula, .adamas, .philosophus, .solve, .azoth]

    private static let orbits: [Orbit] = {
        let count = Double(order.count)
        var out: [Orbit] = []
        for (i, key) in order.enumerated() {
            let idx = Double(i)
            let a0: Double = idx / count * 2 * .pi - .pi / 2
            let rr: Double = 0.30 + 0.13 * Double(i % 2)
            let dir: Double = i % 2 == 1 ? -1.0 : 1.0
            let speed: Double = dir * (0.05 + 0.013 * Double(i % 3))
            let size: Double = 0.16 + (i % 2 == 0 ? 0.03 : 0.0)
            out.append(Orbit(key: key, a0: a0, rr: rr, speed: speed, size: size))
        }
        return out
    }()

    /// The center of the composition and the base radius, for a given canvas.
    private func geometry(_ size: CGSize) -> (center: CGPoint, r: Double) {
        (CGPoint(x: size.width / 2, y: size.height * 0.52), min(size.width, size.height))
    }

    /// A door's center at time `t` — shared by the renderer and hit-testing so a
    /// tap lands on the glyph exactly where it's drawn.
    private func position(_ orbit: Orbit, t: Double, center: CGPoint, r: Double) -> CGPoint {
        let a = orbit.a0 + t * orbit.speed
        // The gallery ran a WIDE ellipse (1.35×0.78) on a wide stage; a portrait
        // phone is the opposite aspect, so the ellipse is squeezed horizontally
        // and given the vertical room — keeping the doors off the side edges.
        return CGPoint(
            x: center.x + cos(a) * r * orbit.rr * 0.92,
            y: center.y + sin(a) * r * orbit.rr * 1.06
        )
    }

    /// The decaying flare bump (0..1) for a door, ~1s after a tap.
    private func flare(_ key: GlyphKey, now: Date) -> Double {
        guard let start = flareStart[key] else { return 0 }
        return max(0, 1 - now.timeIntervalSince(start))
    }

    var body: some View {
        TimelineView(.animation(paused: reduceMotion)) { timeline in
            let now = timeline.date
            Canvas { ctx, size in
                let t = reduceMotion ? 0 : now.timeIntervalSinceReferenceDate
                let (center, r) = geometry(size)
                drawCore(&ctx, center: center, r: r, t: t)
                for orbit in Self.orbits {
                    let pos = position(orbit, t: t, center: center, r: r)
                    let s = r * orbit.size
                    let h = min(1, heats[orbit.key] + flare(orbit.key, now: now) * 0.5)
                    var c = ctx
                    GlyphHeat.draw(
                        &c, key: orbit.key,
                        rect: CGRect(x: pos.x - s / 2, y: pos.y - s / 2, width: s, height: s),
                        heat: h, time: t
                    )
                }
            }
            .contentShape(Rectangle())
            .gesture(
                SpatialTapGesture().onEnded { value in
                    handleTap(value.location, size: proxySize)
                }
            )
            .background(
                GeometryReader { geo in
                    Color.clear.onAppear { proxySize = geo.size }
                        .onChange(of: geo.size) { _, s in proxySize = s }
                }
            )
        }
        // The animated dial is decorative to VoiceOver; a static, labeled list of
        // the doors carries the real semantics (name + door + heat band).
        .accessibilityRepresentation {
            VStack {
                ForEach(Self.order, id: \.self) { key in
                    Button { onTap(key) } label: { Text(accessibilityLabel(key)) }
                }
            }
        }
    }

    @State private var proxySize: CGSize = .zero

    /// Finds the nearest door to a tap and, if close enough, flares + routes it.
    private func handleTap(_ location: CGPoint, size: CGSize) {
        guard size != .zero else { return }
        let t = reduceMotion ? 0 : Date().timeIntervalSinceReferenceDate
        let (center, r) = geometry(size)
        var best: (key: GlyphKey, dist: Double, s: Double)?
        for orbit in Self.orbits {
            let pos = position(orbit, t: t, center: center, r: r)
            let d = Double(hypot(location.x - pos.x, location.y - pos.y))
            if best == nil || d < best!.dist {
                best = (orbit.key, d, r * orbit.size)
            }
        }
        guard let best, best.dist < max(Double(Ember.S.minTarget), best.s * 0.9) else { return }
        flareStart[best.key] = Date()
        onTap(best.key)
    }

    private func accessibilityLabel(_ key: GlyphKey) -> String {
        "\(key.name) — \(key.door) — \(heatBand(for: heats[key]).name)"
    }

    // MARK: The forge core (radial bloom, breathing)

    private func drawCore(_ ctx: inout GraphicsContext, center: CGPoint, r: Double, t: Double) {
        let breathe = 1 + 0.06 * sin(t * 0.9)
        let coreR = r * 0.11 * breathe
        // Brightness rides the furnace heat a touch, but the heart always lives.
        let glow = 0.7 + 0.3 * heats.furnace

        let outer = coreR * 3.1
        let bloom = GraphicsContext.Shading.radialGradient(
            Gradient(stops: [
                .init(color: Color(red: 1, green: 214.0 / 255, blue: 150.0 / 255).opacity(0.95 * glow), location: 0),
                .init(color: Color(red: 1, green: 154.0 / 255, blue: 61.0 / 255).opacity(0.75 * glow), location: 0.18),
                .init(color: Color(red: 163.0 / 255, green: 74.0 / 255, blue: 14.0 / 255).opacity(0.22), location: 0.5),
                .init(color: Color(red: 163.0 / 255, green: 74.0 / 255, blue: 14.0 / 255).opacity(0), location: 1),
            ]),
            center: center, startRadius: coreR * 0.1, endRadius: outer
        )
        ctx.fill(
            Path(ellipseIn: CGRect(x: center.x - outer, y: center.y - outer, width: outer * 2, height: outer * 2)),
            with: bloom
        )
        // The bright heart.
        let heart = coreR * 0.55
        ctx.fill(
            Path(ellipseIn: CGRect(x: center.x - heart, y: center.y - heart, width: heart * 2, height: heart * 2)),
            with: .color(Color(red: 1, green: 236.0 / 255, blue: 204.0 / 255).opacity(0.95))
        )
    }
}
