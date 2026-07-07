import SwiftUI

// The dialogue + the Bellows. Placeholder for E1 — E4 fills in the real
// ember-bed/transcript/endpointing UI. This much is real: it drives
// `AthanorEngineProtocol` (begin → sendTurn → events), so E4 replaces
// rendering, not wiring.
struct SessionScreen: View {
    var model: AppModel
    var onClose: () -> Void

    @State private var lines: [String] = []
    @State private var condensed = false
    @State private var stream: AsyncStream<SessionEvent>?

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text("Session").font(Ember.F.sans(13, weight: .semibold)).foregroundStyle(Ember.C.muted)
                Spacer()
                Button("Close", action: onClose)
                    .font(Ember.F.sans(14, weight: .semibold))
                    .foregroundStyle(Ember.C.heat)
            }
            .padding(Ember.S.screenPad)

            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    ForEach(Array(lines.enumerated()), id: \.offset) { _, line in
                        Text(line)
                            .font(Ember.F.serif(19))
                            .foregroundStyle(Ember.C.ink)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, Ember.S.screenPad)
            }

            if condensed {
                Text("🜔 salt fixed")
                    .font(Ember.F.serif(14, italic: true))
                    .foregroundStyle(Ember.C.gold)
                    .padding(.bottom, 8)
            }

            // Placeholder ember bed — E4 replaces with live PCM-amplitude
            // rendering + preview_tail/finals/endpointing.
            Circle()
                .fill(Ember.C.heatDeep.opacity(0.4))
                .frame(width: 64, height: 64)
                .overlay(Circle().stroke(Ember.C.heat, lineWidth: 2))
                .padding(.bottom, 32)
                .onTapGesture { model.engine.sendTurn("(demo tap)") }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Ember.C.ground.ignoresSafeArea())
        .task { await begin() }
    }

    private func begin() async {
        guard let s = try? model.engine.beginSession(threadId: nil) else { return }
        stream = s
        model.engine.sendTurn("(session opens)")
        for await event in s {
            switch event {
            case .textDelta(let text, _):
                lines.append(text)
            case .condensation:
                condensed = true
            case .turnComplete, .toolCall, .error:
                break
            }
        }
    }
}
