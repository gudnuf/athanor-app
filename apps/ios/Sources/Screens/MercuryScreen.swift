import SwiftUI

// Open threads. E5 fills in evaporation-aware behavior; E1 ships the list
// off seeded data with state shown per thread.
struct MercuryScreen: View {
    var model: AppModel

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text("\(Ember.Glyph.mercury) Mercury")
                .font(Ember.F.serif(20, weight: .semibold))
                .foregroundStyle(Ember.C.ink)
                .padding(.horizontal, Ember.S.screenPad)
                .padding(.top, 16)
                .padding(.bottom, 8)

            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    ForEach(model.engine.mercury()) { thread in
                        HStack(alignment: .top, spacing: 10) {
                            Text(Ember.Glyph.mercury)
                                .foregroundStyle(Ember.C.muted)
                            VStack(alignment: .leading, spacing: 3) {
                                Text(thread.prompt)
                                    .font(Ember.F.sans(14))
                                    .foregroundStyle(Ember.C.ink)
                                Text("\(thread.domain) · \(thread.state.rawValue)")
                                    .font(Ember.F.sans(11, weight: .semibold))
                                    .foregroundStyle(Ember.C.mutedDim)
                            }
                        }
                        .padding(.vertical, 8)
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
