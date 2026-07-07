import SwiftUI

// The salt shelf. E5 fills in the real spiral rendering (parent/child thread
// links); E1 ships a real chronological list off seeded data with the
// immutability posture already correct (no edit affordance anywhere).
struct GrimoireScreen: View {
    var model: AppModel

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text("\(Ember.Glyph.grimoire) Grimoire")
                .font(Ember.F.serif(20, weight: .semibold))
                .foregroundStyle(Ember.C.ink)
                .padding(.horizontal, Ember.S.screenPad)
                .padding(.top, 16)
                .padding(.bottom, 8)

            ScrollView {
                VStack(alignment: .leading, spacing: 18) {
                    ForEach(model.engine.grimoire()) { realization in
                        VStack(alignment: .leading, spacing: 6) {
                            Text(realization.text)
                                .font(Ember.F.serif(17))
                                .foregroundStyle(Ember.C.ink)
                            Text(realization.domains.joined(separator: " · "))
                                .font(Ember.F.sans(11, weight: .semibold))
                                .foregroundStyle(Ember.C.gold)
                        }
                        .padding(.vertical, 10)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .overlay(alignment: .bottom) { Ember.C.hairline.frame(height: 1) }
                    }
                }
                .padding(.horizontal, Ember.S.screenPad)
                .padding(.bottom, 100)
            }
        }
    }
}
