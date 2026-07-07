import SwiftUI

// Navigation shell: Furnace (root) → Session, Grimoire, Mercury are its three
// destinations. E2–E5 fill in these screens' real content; E1 ships them as
// placeholders wired into real navigation so later tasks only touch screen
// bodies, never the shell.

enum FurnaceTab: Hashable {
    case furnace, mercury, grimoire
}

struct FurnaceShell: View {
    var model: AppModel
    @State private var tab: FurnaceTab
    @State private var showTabula = false
    @State private var sessionActive: Bool

    init(model: AppModel) {
        self.model = model
        // Screenshot/QA automation hook only (mirrors murmur-rmp's `screen=`
        // launch arg) — jumps straight to a tab or the session screen so
        // screens can be captured without scripting real taps. Never
        // affects a normal launch.
        let args = ProcessInfo.processInfo.arguments
        let screen = args.first(where: { $0.hasPrefix("screen=") })?.dropFirst("screen=".count)
        _tab = State(initialValue: screen == "mercury" ? .mercury : screen == "grimoire" ? .grimoire : .furnace)
        _sessionActive = State(initialValue: screen == "session")
    }

    var body: some View {
        Group {
            switch tab {
            case .furnace:
                FurnaceScreen(model: model, onBegin: { sessionActive = true }, onTabula: { showTabula = true })
            case .mercury:
                MercuryScreen(model: model)
            case .grimoire:
                GrimoireScreen(model: model)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Ember.C.ground.ignoresSafeArea())
        // safeAreaInset (not a ZStack overlay) so screen content lays out
        // above the tab bar instead of being clipped behind it.
        .safeAreaInset(edge: .bottom, spacing: 0) {
            EmberTabBar(tab: $tab)
        }
        .fullScreenCover(isPresented: $sessionActive) {
            SessionScreen(model: model, onClose: { sessionActive = false })
        }
        .sheet(isPresented: $showTabula) {
            TabulaScreen(model: model)
        }
    }
}

/// Bottom tab bar — glyph-first, matching the mockups (🜍 Furnace, ☿ Mercury,
/// 🜔 Grimoire). Status marks, not decoration (spec: "Glyphs").
private struct EmberTabBar: View {
    @Binding var tab: FurnaceTab

    var body: some View {
        HStack(spacing: 0) {
            tabButton(.furnace, glyph: Ember.Glyph.furnace, label: "Furnace")
            tabButton(.mercury, glyph: Ember.Glyph.mercury, label: "Mercury")
            tabButton(.grimoire, glyph: Ember.Glyph.grimoire, label: "Grimoire")
        }
        .padding(.top, 10)
        .padding(.bottom, 24)
        .background(
            Ember.C.raised
                .overlay(alignment: .top) { Ember.C.hairline.frame(height: 1) }
        )
    }

    private func tabButton(_ value: FurnaceTab, glyph: String, label: String) -> some View {
        let active = tab == value
        return Button {
            tab = value
        } label: {
            VStack(spacing: 4) {
                Text(glyph).font(.system(size: 20))
                Text(label).font(Ember.F.sans(11, weight: .semibold))
            }
            .foregroundStyle(active ? Ember.C.heat : Ember.C.mutedDim)
            .frame(maxWidth: .infinity)
        }
        .buttonStyle(.plain)
    }
}
