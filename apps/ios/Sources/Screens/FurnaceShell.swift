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
    /// Bumped when a session closes so the read surfaces re-fetch. The engine
    /// reads (`furnaceState`/`mercury`/`grimoire`) aren't `@Observable`, so a
    /// salt fixed or thread opened this session wouldn't otherwise show until a
    /// relaunch — returning from a session must reflect what just changed.
    @State private var readEpoch = 0

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
                .id(readEpoch)
                .tag(FurnaceSurface.grimoire)

            FurnaceScreen(
                model: model,
                onBegin: { sessionActive = true },
                onTabula: { showTabula = true },
                onGrimoire: { turn(to: .grimoire) },
                onMercury: { turn(to: .mercury) }
            )
            .id(readEpoch)
            .tag(FurnaceSurface.furnace)

            MercuryScreen(model: model)
                .overlay(alignment: .topTrailing) { HomeMark(onHome: turnHome) }
                .id(readEpoch)
                .tag(FurnaceSurface.mercury)
        }
        .tabViewStyle(.page(indexDisplayMode: .never))
        // A closed session may have fixed salt / opened a thread — re-fetch the
        // read surfaces so the change shows without a relaunch.
        .onChange(of: sessionActive) { _, active in
            if !active { readEpoch += 1 }
        }
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
        .task {
            // Recording QA hook only (`demo-arc=1`): drives the whole demo arc
            // hands-free for one continuous screen recording — Furnace → turn to
            // Mercury and back → the Tabula scroll → Tend the fire (the session's
            // own `debug-turn=`/`debug-turn2=` inject Sam's live exchange, whose
            // climax is a real fix_salt firing the gold condensation) → close →
            // Grimoire with the new grain on top. Timings are generous so a live
            // turn has room to stream. Never fires on a normal launch.
            guard ProcessInfo.processInfo.arguments.contains("demo-arc=1") else { return }
            func beat(_ ms: UInt64) async { try? await Task.sleep(for: .milliseconds(ms)) }
            await beat(3800)
            turn(to: .mercury)     // swipe to Sam's open questions
            await beat(4200)
            turn(to: .furnace)     // turn home
            await beat(1800)
            showTabula = true      // pull up the kindled scroll
            await beat(4200)
            showTabula = false
            await beat(1600)
            sessionActive = true   // Tend the fire → the live session + condensation
            await beat(34000)      // room for the 2-turn live exchange to land salt
            sessionActive = false  // close
            await beat(2200)
            turn(to: .grimoire)    // the new grain sits at the top
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
