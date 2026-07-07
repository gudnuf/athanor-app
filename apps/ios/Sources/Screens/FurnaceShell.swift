import SwiftUI

// Navigation shell. The Furnace is HOME — Mercury and Grimoire are chambers you
// turn to from it, not sibling tabs. There is no bottom bar (the operator's
// call: a persistent banner fights the app's ritual-object feel). Instead the
// three surfaces live on one horizontal plane you turn through — Furnace at
// centre, Grimoire a turn to the left, Mercury a turn to the right — reached by
// swiping or by the quiet glyph marks in the Furnace's own margins. The Tabula
// (a scroll you pull up) stays a sheet; a session (immersive) stays a cover.

enum FurnaceSurface: Hashable {
    case grimoire, furnace, mercury
}

struct FurnaceShell: View {
    var model: AppModel
    @State private var surface: FurnaceSurface
    @State private var showTabula = false
    @State private var sessionActive: Bool

    init(model: AppModel) {
        self.model = model
        // Screenshot/QA automation hook only (mirrors murmur-rmp's `screen=`
        // launch arg) — turns straight to a surface or opens the session/scroll
        // so screens can be captured without scripting real swipes. Never
        // affects a normal launch.
        let args = ProcessInfo.processInfo.arguments
        let screen = args.first(where: { $0.hasPrefix("screen=") })?.dropFirst("screen=".count)
        _surface = State(initialValue: screen == "mercury" ? .mercury : screen == "grimoire" ? .grimoire : .furnace)
        _sessionActive = State(initialValue: screen == "session")
        _showTabula = State(initialValue: screen == "tabula")
    }

    var body: some View {
        // One horizontal plane. Page order IS the spatial layout: Grimoire to
        // the left of the Furnace, Mercury to its right. Swipe to turn; no page
        // dots (that would just be the banner again, smaller).
        TabView(selection: $surface) {
            GrimoireScreen(model: model)
                .overlay(alignment: .topTrailing) { HomeMark(onHome: turnHome) }
                .tag(FurnaceSurface.grimoire)

            FurnaceScreen(
                model: model,
                onBegin: { sessionActive = true },
                onTabula: { showTabula = true },
                onGrimoire: { turn(to: .grimoire) },
                onMercury: { turn(to: .mercury) }
            )
            .tag(FurnaceSurface.furnace)

            MercuryScreen(model: model)
                .overlay(alignment: .topTrailing) { HomeMark(onHome: turnHome) }
                .tag(FurnaceSurface.mercury)
        }
        .tabViewStyle(.page(indexDisplayMode: .never))
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Ember.C.ground.ignoresSafeArea())
        .fullScreenCover(isPresented: $sessionActive) {
            SessionScreen(model: model, onClose: { sessionActive = false })
        }
        .sheet(isPresented: $showTabula) {
            TabulaScreen(model: model)
        }
        .task {
            // Screenshot/recording QA hook only (same family as `screen=` /
            // `autoplay=`): turns through the surfaces on a timer so the spring
            // transition can be captured on video without a scripted swipe.
            // Never fires on a normal launch.
            guard ProcessInfo.processInfo.arguments.contains("autonav=1") else { return }
            for next in [FurnaceSurface.mercury, .furnace, .grimoire, .furnace] {
                try? await Task.sleep(for: .milliseconds(1100))
                turn(to: next)
            }
        }
    }

    /// Turn to a surface — the one spring family (the room turning under the
    /// hand), never a bespoke curve.
    private func turn(to s: FurnaceSurface) {
        withAnimation(Ember.Motion.surfaceTurn) { surface = s }
    }

    private func turnHome() { turn(to: .furnace) }
}

/// The quiet way back from a chamber: the Furnace's own mark, top-trailing,
/// where a chamber's header leaves room. Swiping back works too — this is the
/// visible affordance for it. A full 44pt target around a small glyph.
private struct HomeMark: View {
    var onHome: () -> Void

    var body: some View {
        Button(action: onHome) {
            Text(Ember.Glyph.furnace)
                .font(.system(size: 17))
                .foregroundStyle(Ember.C.heat.opacity(0.8))
                .frame(width: Ember.S.minTarget, height: Ember.S.minTarget)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .padding(.trailing, Ember.S.screenPad - 8)
        .padding(.top, 8)
        .accessibilityLabel("Return to the Furnace")
    }
}
