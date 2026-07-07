import SwiftUI

// The glyphs of the Athanor — nine marks drawn from one shared circle/triangle/
// line geometry, rendered on a continuous HEAT dial (lane 14). Ported faithfully
// from the operator-approved design gallery
// (~/athanor/forge/athanor-app/glyphs-of-the-athanor.html): the geometry table
// `G`, the `HEAT_RAMP` color stops, the seven named `BANDS`, and the continuous
// `drawGlyphHeat` renderer (width/cap/bloom/spark/white-core parameters).
//
// Zero image assets: everything is SwiftUI Canvas/Path over a 0..100 model box.
// Heat is COMPUTED (see athanor-core `home_heat`), never hardcoded — heat is the
// app's notification system, so there are no badges, counts, or red dots.

// MARK: - The nine doors

/// One door of the Athanor. Raw value is the core's heat-map key.
enum GlyphKey: String, CaseIterable {
    case furnace, bellows, mercury, grimoire, tabula, adamas, philosophus, solve, azoth

    /// The uppercase name shown under the glyph / read by VoiceOver.
    var name: String {
        switch self {
        case .furnace: return "Furnace"
        case .bellows: return "Bellows"
        case .mercury: return "Mercury"
        case .grimoire: return "Grimoire"
        case .tabula: return "Tabula"
        case .adamas: return "Adamas"
        case .philosophus: return "Philosophus"
        case .solve: return "Solve"
        case .azoth: return "Azoth"
        }
    }

    /// The VoiceOver "door" phrase — what tapping opens.
    var door: String {
        switch self {
        case .furnace: return "home"
        case .bellows: return "open a session"
        case .mercury: return "open threads"
        case .grimoire: return "the salt shelf"
        case .tabula: return "the scroll"
        case .adamas, .philosophus, .solve, .azoth: return "begin a session in this voice"
        }
    }

    /// Whether this door begins a session in a chosen mask (masks vs. surfaces).
    var isMask: Bool {
        switch self {
        case .adamas, .philosophus, .solve, .azoth: return true
        default: return false
        }
    }
}

// MARK: - Geometry (the shared drawn language, 0..100 model box)

/// A single drawing primitive of a glyph, in the 0..100 design box. Curved ops
/// (SVG `Q`/`A`/spiral in the gallery) are pre-sampled to polylines here so both
/// stroking and spark-anchor sampling use one representation; at 96pt the
/// segment count reads as smooth.
enum GlyphOp {
    case circle(center: CGPoint, r: CGFloat)
    case poly(points: [CGPoint], closed: Bool)
    case curve(points: [CGPoint])
    case dot(center: CGPoint, r: CGFloat)
}

private func p(_ x: CGFloat, _ y: CGFloat) -> CGPoint { CGPoint(x: x, y: y) }

/// Samples a quadratic Bézier (SVG `Q from control to`) into a polyline.
private func quad(_ from: CGPoint, _ c: CGPoint, _ to: CGPoint, _ n: Int = 18) -> [CGPoint] {
    (0...n).map { i in
        let t = CGFloat(i) / CGFloat(n)
        let u = 1 - t
        let x = u * u * from.x + 2 * u * t * c.x + t * t * to.x
        let y = u * u * from.y + 2 * u * t * c.y + t * t * to.y
        return CGPoint(x: x, y: y)
    }
}

/// Samples a circular arc (degrees, screen space y-down) into a polyline.
private func arc(center: CGPoint, r: CGFloat, fromDeg: CGFloat, toDeg: CGFloat, n: Int = 26) -> [CGPoint] {
    (0...n).map { i in
        let a = (fromDeg + (toDeg - fromDeg) * CGFloat(i) / CGFloat(n)) * .pi / 180
        return CGPoint(x: center.x + r * cos(a), y: center.y + r * sin(a))
    }
}

/// The gallery's generated spiral: `spiral(50,50,4,30,2.6)`.
private func spiral(cx: CGFloat, cy: CGFloat, r0: CGFloat, r1: CGFloat, turns: CGFloat) -> [CGPoint] {
    let n = Int((turns * 24).rounded())
    return (0...n).map { i in
        let t = CGFloat(i) / CGFloat(n)
        let a = t * turns * 2 * .pi - .pi / 2
        let r = r0 + (r1 - r0) * t
        return CGPoint(x: cx + r * cos(a), y: cy + r * sin(a))
    }
}

extension GlyphKey {
    /// The glyph's ops, ported 1:1 from the gallery's `const G` geometry table.
    var ops: [GlyphOp] {
        switch self {
        case .furnace:
            return [
                .poly(points: [p(50, 16), p(80, 72), p(20, 72)], closed: true),
                .poly(points: [p(32, 84), p(68, 84)], closed: false),
                .dot(center: p(50, 56), r: 4.5),
            ]
        case .bellows:
            return [
                .curve(points: quad(p(22, 62), p(50, 30), p(78, 62))),
                .curve(points: quad(p(30, 74), p(50, 52), p(70, 74))),
                .dot(center: p(50, 24), r: 3.5),
            ]
        case .mercury:
            return [
                .circle(center: p(50, 52), r: 15),
                // The horns: SVG "M32,26 A18,18 0 0,0 68,26" — a semicircle
                // centered (50,26) bowing UP (through (50,8)).
                .curve(points: arc(center: p(50, 26), r: 18, fromDeg: 180, toDeg: 360)),
                .poly(points: [p(50, 67), p(50, 90)], closed: false),
                .poly(points: [p(39, 79), p(61, 79)], closed: false),
            ]
        case .grimoire:
            return [
                .circle(center: p(50, 52), r: 24),
                .poly(points: [p(26, 52), p(74, 52)], closed: false),
            ]
        case .tabula:
            return [
                .poly(points: [p(50, 14), p(69, 45), p(31, 45)], closed: true),
                .poly(points: [p(50, 45), p(50, 82)], closed: false),
                .poly(points: [p(37, 66), p(63, 66)], closed: false),
            ]
        case .adamas:
            return [
                .poly(points: [p(50, 14), p(80, 50), p(50, 86), p(20, 50)], closed: true),
                .poly(points: [p(20, 50), p(80, 50)], closed: false),
                .poly(points: [p(35, 32), p(65, 68)], closed: false),
            ]
        case .philosophus:
            return [
                .curve(points: quad(p(18, 52), p(50, 22), p(82, 52))),
                .curve(points: quad(p(18, 52), p(50, 82), p(82, 52))),
                .dot(center: p(50, 52), r: 6),
            ]
        case .solve:
            return [
                .poly(points: [p(28, 26), p(72, 26), p(50, 64)], closed: true),
                .dot(center: p(50, 76), r: 3),
                .dot(center: p(43, 87), r: 2.2),
                .dot(center: p(57, 87), r: 2.2),
            ]
        case .azoth:
            return [
                .curve(points: spiral(cx: 50, cy: 50, r0: 4, r1: 30, turns: 2.6)),
                .dot(center: p(50, 50), r: 3),
            ]
        }
    }

    /// The spark anchor points (where sparks catch through the kindled band),
    /// mirroring the gallery's `samplePoints` anchor selection: circle ends,
    /// polyline vertices, curve ends + apex, and dots.
    var anchors: [CGPoint] {
        var out: [CGPoint] = []
        for op in ops {
            switch op {
            case let .circle(c, r):
                out.append(CGPoint(x: c.x + r, y: c.y))
                out.append(CGPoint(x: c.x - r, y: c.y))
            case let .poly(pts, _):
                out.append(contentsOf: pts)
            case let .curve(pts):
                if let f = pts.first { out.append(f) }
                if pts.count > 2 { out.append(pts[pts.count / 2]) }
                if let l = pts.last { out.append(l) }
            case let .dot(c, _):
                out.append(c)
            }
        }
        return out
    }
}

// MARK: - The heat dial (ramp + bands)

/// A named temperature band on the 0..1 dial.
struct HeatBand {
    let threshold: Double
    let name: String
    let meaning: String
}

/// The seven named bands (gallery `BANDS`). `band(for:)` picks the hottest whose
/// threshold `h` has passed.
let heatBands: [HeatBand] = [
    HeatBand(threshold: 0.00, name: "cold iron", meaning: "a locked door — not yet"),
    HeatBand(threshold: 0.15, name: "cooling", meaning: "untouched for a long while"),
    HeatBand(threshold: 0.30, name: "engraved", meaning: "a door at rest — present, patient"),
    HeatBand(threshold: 0.50, name: "stirring", meaning: "something waits behind it"),
    HeatBand(threshold: 0.68, name: "kindled", meaning: "sparks at the joints — ripe"),
    HeatBand(threshold: 0.85, name: "molten", meaning: "the Mystagogue would walk here"),
    HeatBand(threshold: 0.97, name: "roaring", meaning: "now — this one, tonight"),
]

func heatBand(for h: Double) -> HeatBand {
    var band = heatBands[0]
    for b in heatBands where h >= b.threshold { band = b }
    return band
}

/// The heat color ramp (gallery `HEAT_RAMP`): grey → brass → cream → white-hot.
private let heatRamp: [(pos: Double, color: (r: Double, g: Double, b: Double))] = [
    (0.00, (0x5b / 255, 0x54 / 255, 0x4a / 255)),
    (0.22, (0x8d / 255, 0x82 / 255, 0x72 / 255)),
    (0.42, (0xe8 / 255, 0xb2 / 255, 0x5c / 255)),
    (0.62, (0xff / 255, 0xd9 / 255, 0xa0 / 255)),
    (0.85, (0xff / 255, 0xb4 / 255, 0x5e / 255)),
    (1.00, (0xff / 255, 0xef / 255, 0xd8 / 255)),
]

/// Interpolates the ramp at `t` (0..1) → a `Color`.
func heatRampColor(_ t: Double) -> Color {
    let stops = heatRamp
    for i in 0..<(stops.count - 1) {
        let (p1, c1) = stops[i]
        let (p2, c2) = stops[i + 1]
        if t <= p2 || i == stops.count - 2 {
            let u = min(1, max(0, (t - p1) / (p2 - p1)))
            return Color(
                red: c1.r + (c2.r - c1.r) * u,
                green: c1.g + (c2.g - c1.g) * u,
                blue: c1.b + (c2.b - c1.b) * u
            )
        }
    }
    return Color(red: heatRamp[0].color.r, green: heatRamp[0].color.g, blue: heatRamp[0].color.b)
}

// MARK: - The continuous renderer

/// Draws one glyph at heat `h` (0..1) and time `t` (seconds, for the bloom/spark
/// breathing) into `ctx`, filling the `rect`. A faithful port of the gallery's
/// `drawGlyphHeat`: stroke color ramp, width 5.5→1.5 across the cold half, caps
/// rounding in from ~.18, a blur bloom rising from .36, sparks at anchors from
/// .45, and a white-hot core line above .9. All continuous.
enum GlyphHeat {
    static func draw(_ ctx: inout GraphicsContext, key: GlyphKey, rect: CGRect, heat h: Double, time t: Double) {
        let size = min(rect.width, rect.height)
        let scale = size / 100
        let originX = rect.midX - size / 2
        let originY = rect.midY - size / 2
        // Maps a 0..100 model point into the target rect.
        func m(_ pt: CGPoint) -> CGPoint {
            CGPoint(x: originX + pt.x * scale, y: originY + pt.y * scale)
        }

        let ops = key.ops
        let col = heatRampColor(h)
        let w = h < 0.3 ? 5.5 - (h / 0.3) * 3.6 : max(1.5, 1.9 - (h - 0.3) * 0.4)
        let cap: CGLineCap = h < 0.18 ? .butt : .round
        let join: CGLineJoin = h < 0.18 ? .miter : .round

        // Builds the stroked Path (lines/curves/circles) at a given model-space
        // width; dots are filled separately.
        func strokePath() -> Path {
            var path = Path()
            for op in ops {
                switch op {
                case let .circle(c, r):
                    let center = m(c)
                    path.addEllipse(in: CGRect(
                        x: center.x - r * scale, y: center.y - r * scale,
                        width: r * 2 * scale, height: r * 2 * scale))
                case let .poly(pts, closed):
                    guard let first = pts.first else { continue }
                    path.move(to: m(first))
                    for pt in pts.dropFirst() { path.addLine(to: m(pt)) }
                    if closed { path.closeSubpath() }
                case let .curve(pts):
                    guard let first = pts.first else { continue }
                    path.move(to: m(first))
                    for pt in pts.dropFirst() { path.addLine(to: m(pt)) }
                case .dot:
                    continue
                }
            }
            return path
        }

        func dotsPath(_ color: Color) -> Path {
            var path = Path()
            for case let .dot(c, r) in ops {
                let center = m(c)
                path.addEllipse(in: CGRect(
                    x: center.x - r * scale, y: center.y - r * scale,
                    width: r * 2 * scale, height: r * 2 * scale))
            }
            return path
        }

        // Bloom — grows from the stirring band upward, breathing slightly with t.
        if h > 0.36 {
            let grow = min(1, (h - 0.36) / 0.55)
            let bloomA = min(1, (h - 0.36) / 0.5) * (0.55 + 0.2 * sin(t * 1.1))
            let bloomWidth = (2 + 8 * grow) * scale
            let blur = 4 + 20 * grow
            ctx.drawLayer { layer in
                layer.addFilter(.blur(radius: blur))
                let style = StrokeStyle(lineWidth: bloomWidth, lineCap: .round, lineJoin: .round)
                let bloomColor = Color(red: 1, green: 138.0 / 255, blue: 30.0 / 255)
                    .opacity(max(0, bloomA * 0.9))
                layer.stroke(strokePath(), with: .color(bloomColor), style: style)
                layer.fill(dotsPath(bloomColor), with: .color(bloomColor))
            }
        }

        // The line itself.
        let style = StrokeStyle(lineWidth: w * scale, lineCap: cap, lineJoin: join)
        ctx.stroke(strokePath(), with: .color(col), style: style)
        ctx.fill(dotsPath(col), with: .color(col))

        // Sparks catch at anchors through the kindled band.
        if h > 0.45 {
            let sparkA = h < 0.75 ? (h - 0.45) / 0.3 : 1
            for (i, a) in key.anchors.enumerated() {
                let tw = 0.6 + 0.4 * sin(t * 2 + Double(i) * 1.7)
                let center = m(a)
                let r = (1.6 + 1.4 * sparkA) * scale
                let sparkColor = Color(red: 1, green: 224.0 / 255, blue: 178.0 / 255)
                    .opacity(max(0, sparkA * tw))
                // Glow: a soft blurred halo under the crisp spark dot.
                ctx.drawLayer { layer in
                    layer.addFilter(.blur(radius: 6 * sparkA))
                    layer.fill(
                        Path(ellipseIn: CGRect(x: center.x - r, y: center.y - r, width: r * 2, height: r * 2)),
                        with: .color(Color(red: 1, green: 180.0 / 255, blue: 94.0 / 255).opacity(max(0, sparkA * tw * 0.9)))
                    )
                }
                ctx.fill(
                    Path(ellipseIn: CGRect(x: center.x - r, y: center.y - r, width: r * 2, height: r * 2)),
                    with: .color(sparkColor)
                )
            }
        }

        // White-hot core line at the top of the dial.
        if h > 0.9 {
            let coreStyle = StrokeStyle(lineWidth: 1 * scale, lineCap: .round, lineJoin: .round)
            let coreColor = Color(red: 1, green: 246.0 / 255, blue: 230.0 / 255)
                .opacity(min(1, (h - 0.9) * 8))
            ctx.stroke(strokePath(), with: .color(coreColor), style: coreStyle)
            ctx.fill(dotsPath(coreColor), with: .color(coreColor))
        }
    }
}

// MARK: - A single glyph view (spectrum cells, mask chooser, previews)

/// One glyph rendered at a fixed heat. Animated (bloom/spark breathing) unless
/// reduced motion is on, in which case it renders statically.
struct GlyphView: View {
    let key: GlyphKey
    var heat: Double
    var animated: Bool = true

    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        TimelineView(.animation(paused: !animated || reduceMotion)) { timeline in
            Canvas { ctx, size in
                let t = reduceMotion ? 0 : timeline.date.timeIntervalSinceReferenceDate
                var c = ctx
                GlyphHeat.draw(&c, key: key, rect: CGRect(origin: .zero, size: size), heat: heat, time: t)
            }
        }
    }
}
