import SwiftUI

// The Furnace — home. E2 fills this in properly (ember bed reflecting held
// heat, cold-furnace copy, no streak shame). E1 ships enough real structure
// (seeded fire state, one-tap begin, 🜍 Tabula corner) that E2 is a body
// rewrite, not a new screen.
struct FurnaceScreen: View {
    var model: AppModel
    var onBegin: () -> Void
    var onTabula: () -> Void

    private var fire: FireState { model.engine.furnaceState() }

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text("\(Ember.Glyph.furnace) Athanor")
                    .font(Ember.F.serif(20, weight: .semibold))
                    .foregroundStyle(Ember.C.ink)
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

            VStack(spacing: 6) {
                Text("\(fire.wisdomDays)")
                    .font(Ember.F.serif(48, weight: .medium))
                    .foregroundStyle(Ember.C.heatHot)
                Text(fire.wisdomDays == 1 ? "day tended" : "days tended")
                    .font(Ember.F.serif(15, italic: true))
                    .foregroundStyle(Ember.C.muted)
                Text(fire.tendedToday ? "the fire is fed today" : "the fire is low")
                    .font(Ember.F.sans(13))
                    .foregroundStyle(Ember.C.mutedDim)
                    .padding(.top, 4)
            }

            Spacer()

            Button(action: onBegin) {
                Text("Begin")
                    .font(Ember.F.sans(17, weight: .semibold))
                    .foregroundStyle(Ember.C.ground)
                    .frame(maxWidth: .infinity)
                    .frame(height: Ember.S.buttonHeight)
                    .background(Ember.C.heat, in: Capsule())
            }
            .padding(.horizontal, Ember.S.screenPad)
            .padding(.bottom, 24)
        }
    }
}
