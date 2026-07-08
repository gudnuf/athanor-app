import SwiftUI

// The living home screen (lane 14): a breathing forge core at the center with
// the eight glyph doors drifting around it, each at its own COMPUTED heat.
//
// The doors are a slow, soft-body field (operator addendum): a gentle base
// wander plus short-range repulsion between glyphs and from the forge core,
// with velocities damped hard so a near-collision is a lazy redirect, not a
// bounce — drifting coals, not billiards. A small random perturbation keeps the
// arrangement from ever settling into a fixed pattern. The gallery's Dial
// composition remains the reference for density/sizes/feel, not for literal
// orbit math. All of this lives inside the `furnaceFire` ambient motion
// exception; reduced motion freezes the field but still renders each door at
// its true temperature.

struct HomeDial: View {
    /// Per-door heat, computed in core (never invented here).
    let heats: HomeHeatValues
    /// Tapping a door — the caller routes it (surface turn / begin a session).
    var onTap: (GlyphKey) -> Void

    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    @State private var sim = DialField()
    /// When each door was last tapped, for the decaying flare bump.
    @State private var flareStart: [GlyphKey: Date] = [:]

    private func flare(_ key: GlyphKey, now: Date) -> Double {
        guard let start = flareStart[key] else { return 0 }
        return max(0, 1 - now.timeIntervalSince(start))
    }

    var body: some View {
        TimelineView(.animation(paused: reduceMotion)) { timeline in
            let now = timeline.date
            Canvas { ctx, size in
                sim.ensureInitialized(size: size)
                if !reduceMotion {
                    sim.step(to: now.timeIntervalSinceReferenceDate)
                }
                let t = reduceMotion ? 0 : now.timeIntervalSinceReferenceDate
                drawCore(&ctx, center: sim.center, coreR: sim.coreR, t: t)
                for body in sim.bodies {
                    let s = body.size
                    let h = min(1, heats[body.key] + flare(body.key, now: now) * 0.5)
                    var c = ctx
                    GlyphHeat.draw(
                        &c, key: body.key,
                        rect: CGRect(x: body.pos.x - s / 2, y: body.pos.y - s / 2, width: s, height: s),
                        heat: h, time: t
                    )
                }
            }
            .contentShape(Rectangle())
            .gesture(SpatialTapGesture().onEnded { value in handleTap(value.location) })
        }
        // The animated field is decorative to VoiceOver; a static, labeled list
        // of the doors carries the real semantics (name + door + heat band).
        .accessibilityRepresentation {
            VStack {
                ForEach(GlyphKey.allCases.filter { $0 != .furnace }, id: \.self) { key in
                    Button { onTap(key) } label: { Text(accessibilityLabel(key)) }
                }
            }
        }
    }

    /// Finds the nearest door to a tap and, if close enough, flares + routes it.
    private func handleTap(_ location: CGPoint) {
        var best: (key: GlyphKey, dist: Double, s: Double)?
        for body in sim.bodies {
            let d = Double(hypot(location.x - body.pos.x, location.y - body.pos.y))
            if best == nil || d < best!.dist {
                best = (body.key, d, body.size)
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

    private func drawCore(_ ctx: inout GraphicsContext, center: CGPoint, coreR: Double, t: Double) {
        let breathe = 1 + 0.06 * sin(t * 0.9)
        let r = coreR * breathe
        let glow = 0.7 + 0.3 * heats.furnace

        let outer = r * 3.1
        let bloom = GraphicsContext.Shading.radialGradient(
            Gradient(stops: [
                .init(color: Color(red: 1, green: 214.0 / 255, blue: 150.0 / 255).opacity(0.95 * glow), location: 0),
                .init(color: Color(red: 1, green: 154.0 / 255, blue: 61.0 / 255).opacity(0.75 * glow), location: 0.18),
                .init(color: Color(red: 163.0 / 255, green: 74.0 / 255, blue: 14.0 / 255).opacity(0.22), location: 0.5),
                .init(color: Color(red: 163.0 / 255, green: 74.0 / 255, blue: 14.0 / 255).opacity(0), location: 1),
            ]),
            center: center, startRadius: r * 0.1, endRadius: outer
        )
        ctx.fill(
            Path(ellipseIn: CGRect(x: center.x - outer, y: center.y - outer, width: outer * 2, height: outer * 2)),
            with: bloom
        )
        let heart = r * 0.55
        ctx.fill(
            Path(ellipseIn: CGRect(x: center.x - heart, y: center.y - heart, width: heart * 2, height: heart * 2)),
            with: .color(Color(red: 1, green: 236.0 / 255, blue: 204.0 / 255).opacity(0.95))
        )
    }
}

// MARK: - The soft-body field

/// The gentle physics of the drifting glyph doors. A class (held in `@State`) so
/// its mutable simulation state survives re-renders; TimelineView drives the
/// frames, this integrates them.
final class DialField {
    struct Body {
        let key: GlyphKey
        let size: Double
        var pos: CGPoint
        var vel: CGVector
        /// A per-body phase so the wander is decorrelated between doors.
        let phase: Double
    }

    private(set) var bodies: [Body] = []
    private(set) var center: CGPoint = .zero
    private(set) var coreR: Double = 0
    private var canvasSize: CGSize = .zero
    private var r: Double = 0 // min(w,h)
    private var lastTime: TimeInterval = 0

    /// The eight doors (the Furnace itself is the center core), in the gallery's
    /// HOME order — the order the initial ring is seeded in.
    private static let order: [GlyphKey] =
        [.bellows, .mercury, .grimoire, .tabula, .adamas, .philosophus, .solve, .azoth]

    private func sizeFactor(_ i: Int) -> Double { 0.16 + (i % 2 == 0 ? 0.03 : 0.0) }

    /// Lays out the field once (and re-lays it if the canvas size changes) — the
    /// doors start on two calm rings, then the physics takes over.
    func ensureInitialized(size: CGSize) {
        guard size != canvasSize, size.width > 0, size.height > 0 else { return }
        canvasSize = size
        r = Double(min(size.width, size.height))
        center = CGPoint(x: size.width / 2, y: size.height * 0.52)
        coreR = r * 0.11
        let count = Double(Self.order.count)
        var seeded: [Body] = []
        for (i, key) in Self.order.enumerated() {
            let a: Double = Double(i) / count * 2 * .pi - .pi / 2
            let ring: Double = 0.33 + 0.10 * Double(i % 2)
            let s: Double = sizeFactor(i) * r
            let px: Double = Double(center.x) + cos(a) * r * ring
            let py: Double = Double(center.y) + sin(a) * r * ring
            let body = Body(
                key: key, size: s,
                pos: CGPoint(x: px, y: py),
                vel: .zero, phase: Double(i) * 1.7
            )
            seeded.append(body)
        }
        bodies = seeded
        lastTime = 0
    }

    // Tuning — all chosen for "drifting coals": weak forces, hard damping, a low
    // speed cap. Distances are in points; accelerations in points/s².
    private let maxSpeed: Double = 13
    private let damping: Double = 0.9          // per-second velocity retention
    private let wanderAccel: Double = 7        // random perturbation strength
    private let repel: Double = 90             // glyph↔glyph short-range push
    private let coreRepel: Double = 150        // core↔glyph push
    private let containStiffness: Double = 2.2 // soft annulus keeping the ring

    func step(to time: TimeInterval) {
        guard !bodies.isEmpty else { return }
        if lastTime == 0 { lastTime = time; return }
        var dt = time - lastTime
        lastTime = time
        guard dt > 0 else { return }
        dt = min(dt, 1.0 / 20.0) // clamp a hitch so a long frame can't fling a body

        let inner = r * 0.28      // keep-out around the core
        let outer = r * 0.46      // comfortable outer edge of the field
        let velRetain = pow(damping, dt)

        var next = bodies
        for i in next.indices {
            var accel = CGVector(dx: 0, dy: 0)

            // Repulsion from every other door when closer than a comfortable gap.
            for j in next.indices where j != i {
                let d = vec(from: next[j].pos, to: next[i].pos)
                let dist = length(d)
                let minSep = (next[i].size + next[j].size) * 0.5 + 12
                if dist > 0.001, dist < minSep {
                    let push = repel * (minSep - dist) / minSep
                    accel = accel + scaled(normalized(d), push)
                }
            }

            // Repulsion from the forge core (never crowd the fire).
            let fromCore = vec(from: center, to: next[i].pos)
            let coreDist = length(fromCore)
            if coreDist > 0.001, coreDist < inner {
                let push = coreRepel * (inner - coreDist) / inner
                accel = accel + scaled(normalized(fromCore), push)
            }

            // Soft annulus containment — a lazy spring pulling a stray door back
            // toward the ring, so the field breathes but never drifts off-screen.
            if coreDist > outer {
                accel = accel + scaled(normalized(fromCore), -containStiffness * (coreDist - outer))
            } else if coreDist < inner {
                accel = accel + scaled(normalized(fromCore), containStiffness * (inner - coreDist))
            }

            // A slow random wander so the arrangement never settles.
            let w = wanderAccel
            let jitter = CGVector(
                dx: (Double.random(in: -1...1)) * w + sin(time * 0.23 + next[i].phase) * w * 0.5,
                dy: (Double.random(in: -1...1)) * w + cos(time * 0.19 + next[i].phase) * w * 0.5
            )
            accel = accel + jitter

            // Integrate: velocity, hard damping, speed clamp, position.
            var vel = next[i].vel + scaled(accel, dt)
            vel = scaled(vel, velRetain)
            let speed = length(vel)
            if speed > maxSpeed { vel = scaled(vel, maxSpeed / speed) }
            next[i].vel = vel
            next[i].pos = CGPoint(x: next[i].pos.x + vel.dx * dt, y: next[i].pos.y + vel.dy * dt)
        }
        bodies = next
    }

    // Small vector helpers (CGVector has no arithmetic of its own).
    private func vec(from a: CGPoint, to b: CGPoint) -> CGVector { CGVector(dx: b.x - a.x, dy: b.y - a.y) }
    private func length(_ v: CGVector) -> Double { Double(hypot(v.dx, v.dy)) }
    private func scaled(_ v: CGVector, _ k: Double) -> CGVector { CGVector(dx: v.dx * k, dy: v.dy * k) }
    private func normalized(_ v: CGVector) -> CGVector {
        let l = length(v)
        return l > 0.0001 ? CGVector(dx: v.dx / l, dy: v.dy / l) : .zero
    }
}

private func + (a: CGVector, b: CGVector) -> CGVector { CGVector(dx: a.dx + b.dx, dy: a.dy + b.dy) }
