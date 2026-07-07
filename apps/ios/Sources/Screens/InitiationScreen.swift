import SwiftUI

// First-launch = the Mystagogue's first session (`begin_initiation`), cold —
// no academy seeding (the generalization test). E3 fills in the real
// dialogue + hands the Tabula at close; E1 ships enough to route first-launch
// correctly and drive the engine.
struct InitiationScreen: View {
    var model: AppModel

    @State private var lines: [String] = []
    @State private var canFinish = false

    var body: some View {
        VStack(spacing: 24) {
            Spacer()
            Text(Ember.Glyph.furnace)
                .font(.system(size: 34))
                .foregroundStyle(Ember.C.heat)
            VStack(spacing: 16) {
                ForEach(Array(lines.enumerated()), id: \.offset) { _, line in
                    Text(line)
                        .font(Ember.F.serif(21))
                        .foregroundStyle(Ember.C.ink)
                        .multilineTextAlignment(.center)
                }
            }
            .padding(.horizontal, Ember.S.screenPad)
            Spacer()
            Button {
                if canFinish {
                    model.hasCompletedInitiation = true
                } else {
                    model.engine.sendTurn("(initiation demo tap)")
                }
            } label: {
                Text(canFinish ? "Enter the Furnace" : "Begin")
                    .font(Ember.F.sans(17, weight: .semibold))
                    .foregroundStyle(Ember.C.ground)
                    .frame(maxWidth: .infinity)
                    .frame(height: Ember.S.buttonHeight)
                    .background(Ember.C.heat, in: Capsule())
            }
            .padding(.horizontal, Ember.S.screenPad)
            .padding(.bottom, 32)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Ember.C.ground.ignoresSafeArea())
        .task { await begin() }
    }

    private func begin() async {
        guard let stream = try? model.engine.beginInitiation() else { return }
        for await event in stream {
            switch event {
            case .textDelta(let text, _):
                lines.append(text)
            case .turnComplete:
                canFinish = true
            case .condensation, .toolCall, .error:
                break
            }
        }
    }
}
