import SwiftUI

// The dialogue + the Bellows. Placeholder for E1 — E4 fills in the real
// ember-bed/transcript/endpointing UI. This much is real: it drives
// `AthanorEngineProtocol` (begin → sendTurn → events) and renders the
// streamed reply via `StreamingText` as deltas actually arrive (word by
// word, no jump) — so E4 replaces the ember-bed/audio wiring, not the
// streaming-render contract.
struct SessionScreen: View {
    var model: AppModel
    var onClose: () -> Void

    /// Finalized turns (completed on `.turnComplete`).
    @State private var lines: [(text: String, register: ReplyRegister)] = []
    /// The turn currently streaming in, if any. Rendered by the same
    /// `StreamingText` view as finalized lines — appending here is instant
    /// (Ember.Motion.none), never re-laying-out what's already on screen.
    @State private var streaming: (text: String, register: ReplyRegister)?
    @State private var condensed = false

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
                        StreamingText(text: line.text, register: line.register)
                    }
                    if let streaming {
                        StreamingText(text: streaming.text, register: streaming.register)
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
        model.engine.sendTurn("(session opens)")
        for await event in s {
            switch event {
            case .textDelta(let chunk, let register):
                // Accumulate deltas exactly as they arrive — never buffer a
                // whole line before showing it. Same behavior the real
                // engine's token stream will drive.
                streaming = (text: (streaming?.text ?? "") + chunk, register: register)
            case .condensation:
                condensed = true
            case .turnComplete:
                if let streaming {
                    lines.append(streaming)
                }
                streaming = nil
            case .toolCall, .error:
                break
            }
        }
    }
}
