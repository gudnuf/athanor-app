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
            WarmingLine(state: model.modelDownloader.state)
                .padding(.horizontal, Ember.S.screenPad)
                .padding(.bottom, 4)
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

/// A quiet presence, not a loading screen (F1 brief: "it should feel
/// incidental"). No spinner, no motion-budget animation — the bar's width
/// just steps to the current fraction (`Ember.Motion.none`); the only
/// screen-level motion here stays E3's (none) plus whatever Initiation
/// itself already spends. Invisible once ready, and quietly honest on
/// failure rather than stuck mid-progress forever.
private struct WarmingLine: View {
    var state: ModelDownloader.State

    var body: some View {
        switch state {
        case .idle, .ready:
            EmptyView()
        case .downloading(let progress):
            bar(label: "warming the ear", progress: progress)
        case .verifying:
            bar(label: "settling in", progress: 1)
        case .failed:
            Text("the ear stays cold for now — offline")
                .font(Ember.F.sans(11))
                .foregroundStyle(Ember.C.mutedDim)
        }
    }

    private func bar(label: String, progress: Double) -> some View {
        VStack(spacing: 6) {
            Capsule()
                .fill(Ember.C.raised2)
                .frame(height: 2)
                .overlay(alignment: .leading) {
                    GeometryReader { geo in
                        Capsule()
                            .fill(Ember.C.heat.opacity(0.55))
                            .frame(width: geo.size.width * progress)
                    }
                }
                .animation(Ember.Motion.none, value: progress)
            Text(label)
                .font(Ember.F.sans(11))
                .foregroundStyle(Ember.C.mutedDim)
        }
    }
}
