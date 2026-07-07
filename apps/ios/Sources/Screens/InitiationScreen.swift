import SwiftUI

// First-launch = the Mystagogue's first session (`begin_initiation`), cold —
// no academy seeding (the generalization test). E3 fills in the real
// dialogue + hands the Tabula at close; E1 ships enough to route first-launch
// correctly, drive the engine, and render its streamed reply without jumping
// (deltas accumulate with Ember.Motion.none — the only animation in this
// screen's budget is none at all; furnace/bellows/condensation stay E2/E4/E5's).
struct InitiationScreen: View {
    var model: AppModel

    /// Finalized turns.
    @State private var lines: [String] = []
    /// The turn currently streaming in — same accumulate-as-deltas-arrive
    /// contract as SessionScreen, just centered/serif for this screen's copy.
    @State private var streaming: String?
    @State private var completedTurns = 0
    @State private var canFinish = false

    var body: some View {
        VStack(spacing: 24) {
            Spacer()
            Text(Ember.Glyph.furnace)
                .font(.system(size: 34))
                .foregroundStyle(Ember.C.heat)
            VStack(spacing: 16) {
                ForEach(Array(lines.enumerated()), id: \.offset) { _, line in
                    initiationLine(line)
                }
                if let streaming {
                    initiationLine(streaming)
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
        .task {
            // Screenshot/QA automation hook only (mirrors murmur-rmp's
            // `autoflow=` launch arg) — taps Begin automatically so the
            // word-by-word streaming path can be captured mid-flight without
            // a real tap gesture. Never affects a normal launch.
            guard ProcessInfo.processInfo.arguments.contains("autoplay=1") else { return }
            try? await Task.sleep(for: .milliseconds(300))
            model.engine.sendTurn("(autoplay)")
        }
    }

    private func initiationLine(_ text: String) -> some View {
        Text(text)
            .font(Ember.F.serif(21))
            .foregroundStyle(Ember.C.ink)
            .multilineTextAlignment(.center)
            .fixedSize(horizontal: false, vertical: true)
            .animation(Ember.Motion.none, value: text)
    }

    private func begin() async {
        guard let stream = try? model.engine.beginInitiation() else { return }
        for await event in stream {
            switch event {
            case .textDelta(let chunk, _):
                streaming = (streaming ?? "") + chunk
            case .turnComplete:
                if let streaming {
                    lines.append(streaming)
                }
                streaming = nil
                completedTurns += 1
                // Two scripted turns make up the demo initiation dialogue
                // (see DemoEngine.initiationTurns); the real E3 flow decides
                // this from the Mystagogue's own close, not a tap count.
                if completedTurns >= 2 {
                    canFinish = true
                }
            case .condensation, .toolCall, .error:
                break
            }
        }
    }
}
