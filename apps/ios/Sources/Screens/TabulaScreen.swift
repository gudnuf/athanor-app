import SwiftUI

// The Tabula Athanorum — a living scroll, reachable from the Furnace's 🜍
// corner mark. Kindled passages glow heat-toned; untraveled ones stay dim.
// (Schema: derived `kindling` events — no new user-facing mechanics.)
struct TabulaScreen: View {
    var model: AppModel

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 22) {
                Text("Tabula Athanorum")
                    .font(Ember.F.serif(24, weight: .semibold))
                    .foregroundStyle(Ember.C.ink)
                    .padding(.top, 24)

                ForEach(model.engine.tabula()) { passage in
                    VStack(alignment: .leading, spacing: 6) {
                        Text("\(passage.number) · \(passage.title)")
                            .font(Ember.F.serif(15, weight: .semibold, italic: true))
                            .foregroundStyle(passage.kindled ? Ember.C.heatHot : Ember.C.mutedDim)
                        Text(passage.body)
                            .font(Ember.F.serif(15))
                            .foregroundStyle(passage.kindled ? Ember.C.ink : Ember.C.mutedDim)
                        if let note = passage.kindledNote {
                            Text("\(Ember.Glyph.furnace) \(note)")
                                .font(Ember.F.sans(11, weight: .semibold))
                                .foregroundStyle(Ember.C.heat)
                        }
                    }
                    .padding(.vertical, 8)
                }
            }
            .padding(.horizontal, Ember.S.screenPad)
            .padding(.bottom, 40)
        }
        .background(Ember.C.ground.ignoresSafeArea())
    }
}
