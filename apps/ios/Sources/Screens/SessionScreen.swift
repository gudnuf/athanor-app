import SwiftUI

// The dialogue + the Bellows, against DemoEngine (mirrors mockups-v2.html
// screens 3 + 4). Real wiring, placeholder audio: this drives
// `AthanorEngineProtocol` (begin → sendTurn → events) and renders the
// streamed reply via `StreamingText` as deltas actually arrive. The ember
// bed's amplitude is a sine stand-in (no AVAudioEngine, no mic permission,
// no capture) clearly seamed for C3's real PCM amplitude — see `BellowsBed`.
struct SessionScreen: View {
    var model: AppModel
    var onClose: () -> Void

    @State private var messages: [SessionMessage] = []
    @State private var streamingText = ""
    @State private var streamingRegister: ReplyRegister = .quick
    @State private var isStreaming = false
    @State private var listening = true
    @State private var showKeyboard = false
    @State private var typedText = ""
    @FocusState private var typedFieldFocused: Bool

    var body: some View {
        VStack(spacing: 0) {
            header

            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 22) {
                        ForEach(messages) { message in
                            messageView(message).id(message.id)
                        }
                        if isStreaming {
                            StreamingText(text: streamingText, register: streamingRegister)
                                .id("streaming")
                        }
                    }
                    .padding(.horizontal, Ember.S.screenPad)
                    .padding(.vertical, 18)
                }
                .onChange(of: streamingText) { _, _ in
                    proxy.scrollTo("streaming", anchor: .bottom)
                }
                .onChange(of: messages.count) { _, _ in
                    if let last = messages.last { proxy.scrollTo(last.id, anchor: .bottom) }
                }
            }

            Bellows(
                listening: $listening,
                showKeyboard: $showKeyboard,
                typedText: $typedText,
                fieldFocused: $typedFieldFocused,
                onTapBed: sendDemoTurn,
                onSubmitTyped: submitTyped
            )
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Ember.C.ground.ignoresSafeArea())
        .task { await begin() }
        .task {
            // Screenshot/QA automation hook only (mirrors the same pattern in
            // InitiationScreen) — taps the bed on a timer so the full script,
            // including the condensation moment, can be captured without a
            // real tap gesture. Never affects a normal launch.
            guard ProcessInfo.processInfo.arguments.contains("autoplay=1") else { return }
            try? await Task.sleep(for: .milliseconds(1200))
            sendDemoTurn()
            try? await Task.sleep(for: .milliseconds(1800))
            sendDemoTurn()
        }
    }

    private var header: some View {
        VStack(spacing: 10) {
            HStack(spacing: 9) {
                // Placeholder mask/mode indicator — the real session's
                // SessionPlan{mask,mode} hasn't surfaced through the engine
                // seam yet (lands with C1/C2); this is cosmetic chrome only.
                Text(Ember.Glyph.fireMask).foregroundStyle(Ember.C.heat)
                Text("ADAMAS").foregroundStyle(Ember.C.ink)
                Text("·").foregroundStyle(Ember.C.mutedDim)
                Text("challenge").foregroundStyle(Ember.C.muted)
                Spacer()
                Button("Close", action: close)
                    .foregroundStyle(Ember.C.heat)
            }
            .font(Ember.F.sans(12, weight: .bold))
            .textCase(.uppercase)
            .tracking(1.2)

            Capsule()
                .fill(Ember.C.raised2)
                .frame(height: 2)
                .overlay(alignment: .leading) {
                    GeometryReader { geo in
                        Capsule()
                            .fill(LinearGradient(colors: [Ember.C.heatDeep, Ember.C.heatHot], startPoint: .leading, endPoint: .trailing))
                            .frame(width: geo.size.width * turnProgress)
                    }
                }
        }
        .padding(.horizontal, Ember.S.screenPad)
        .padding(.top, 6)
        .padding(.bottom, 12)
        .overlay(alignment: .bottom) { Ember.C.hairline.frame(height: 1) }
    }

    /// Cosmetic only — reflects how many scripted beats have played, purely
    /// for the heat-hair progress sliver in the header. No animation (not a
    /// budgeted motion); it just steps.
    private var turnProgress: Double {
        min(Double(messages.count) / 5.0, 1.0)
    }

    @ViewBuilder
    private func messageView(_ message: SessionMessage) -> some View {
        switch message {
        case .teacher(_, let text, let register):
            StreamingText(text: text, register: register)
        case .learner(_, let text):
            Text(text)
                .font(Ember.F.sans(15))
                .foregroundStyle(Ember.C.muted)
                .frame(maxWidth: 260, alignment: .trailing)
                .frame(maxWidth: .infinity, alignment: .trailing)
        case .salt(_, let realization, let childPrompt):
            SaltCard(realization: realization, childPrompt: childPrompt)
                .transition(.scale(scale: 0.92).combined(with: .opacity))
        }
    }

    private func sendDemoTurn() {
        model.engine.sendTurn("(bellows: demo utterance)")
    }

    private func submitTyped() {
        let trimmed = typedText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        messages.append(.learner(id: UUID().uuidString, text: trimmed))
        typedText = ""
        showKeyboard = false
        model.engine.sendTurn(trimmed)
    }

    private func close() {
        Task {
            await model.engine.endSession(abandon: false)
            onClose()
        }
    }

    private func begin() async {
        guard let stream = try? model.engine.beginSession(threadId: nil) else { return }
        model.engine.sendTurn("(session opens)")
        for await event in stream {
            switch event {
            case .textDelta(let chunk, let register):
                if !isStreaming {
                    isStreaming = true
                    streamingRegister = register
                    streamingText = ""
                }
                streamingText += chunk
            case .turnComplete:
                if isStreaming {
                    messages.append(.teacher(id: UUID().uuidString, text: streamingText, register: streamingRegister))
                }
                isStreaming = false
                streamingText = ""
            case .condensation(let realizationId, let childThreadId):
                let realization = model.engine.grimoire().first { $0.id == realizationId }
                let childPrompt = model.engine.mercury().first { $0.id == childThreadId }?.prompt
                if let realization {
                    withAnimation(Ember.Motion.condensation) {
                        messages.append(.salt(id: realizationId, realization: realization, childPrompt: childPrompt))
                    }
                }
            case .toolCall, .error:
                break
            }
        }
    }
}

// MARK: - Message model

enum SessionMessage: Identifiable {
    case teacher(id: String, text: String, register: ReplyRegister)
    case learner(id: String, text: String)
    case salt(id: String, realization: Realization, childPrompt: String?)

    var id: String {
        switch self {
        case .teacher(let id, _, _): return id
        case .learner(let id, _): return id
        case .salt(let id, _, _): return id
        }
    }
}

// MARK: - The condensation moment

/// THE moment of the app: the learner's own words, fixed. Enters via
/// `Ember.Motion.condensation` (the caller wraps the state mutation), then
/// stays completely still — no repeat, no shimmer loop. Gold is reserved
/// for exactly this.
private struct SaltCard: View {
    var realization: Realization
    var childPrompt: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 8) {
                Text(Ember.Glyph.grimoire).foregroundStyle(Ember.C.gold)
                Text("salt fixed")
                    .font(Ember.F.sans(11, weight: .bold))
                    .tracking(1.2)
                    .textCase(.uppercase)
                    .foregroundStyle(Ember.C.gold)
            }
            Text(realization.text)
                .font(Ember.F.serif(18))
                .foregroundStyle(Ember.C.ink)
                .padding(.leading, 12)
                .overlay(alignment: .leading) {
                    Rectangle().fill(Ember.C.gold.opacity(0.45)).frame(width: 2)
                }
            if let childPrompt {
                HStack(alignment: .top, spacing: 6) {
                    Text(Ember.Glyph.mercury).foregroundStyle(Ember.C.mutedDim)
                    Text(childPrompt)
                        .italic()
                }
                .font(Ember.F.serif(13))
                .foregroundStyle(Ember.C.mutedDim)
                .padding(.leading, 12)
            }
        }
        .padding(16)
        .background(Ember.C.raised, in: RoundedRectangle(cornerRadius: 14))
        .overlay(RoundedRectangle(cornerRadius: 14).stroke(Ember.C.gold.opacity(0.3), lineWidth: 1))
    }
}

// MARK: - The Bellows

/// Voice-first input row: an ember bed whose amplitude is a sine stand-in
/// (no audio capture — that's C3), a keyboard glyph for the typed fallback,
/// and (when open) a typed-input field. Tapping the bed simulates an
/// utterance ending and sends a turn — real endpointing (silence-triggered
/// auto-send) replaces this tap once `BellowsHandle` exists.
private struct Bellows: View {
    @Binding var listening: Bool
    @Binding var showKeyboard: Bool
    @Binding var typedText: String
    var fieldFocused: FocusState<Bool>.Binding
    var onTapBed: () -> Void
    var onSubmitTyped: () -> Void

    var body: some View {
        VStack(spacing: 10) {
            if showKeyboard {
                HStack(spacing: 10) {
                    TextField("Say it your way…", text: $typedText, axis: .vertical)
                        .focused(fieldFocused)
                        .font(Ember.F.sans(15))
                        .foregroundStyle(Ember.C.ink)
                        .padding(.horizontal, 14)
                        .padding(.vertical, 10)
                        .background(Ember.C.raised2, in: RoundedRectangle(cornerRadius: 12))
                    Button("Send", action: onSubmitTyped)
                        .font(Ember.F.sans(14, weight: .semibold))
                        .foregroundStyle(Ember.C.heat)
                }
            } else {
                BellowsBed(listening: listening, onTap: onTapBed)
                    .frame(height: 46)
            }

            HStack {
                if !showKeyboard {
                    Text(listening ? "listening — tap to send" : "banked")
                        .font(Ember.F.sans(11))
                        .foregroundStyle(Ember.C.mutedDim)
                }
                Spacer()
                Button {
                    showKeyboard.toggle()
                    if showKeyboard { fieldFocused.wrappedValue = true }
                } label: {
                    Text("⌨").font(.system(size: 16)).foregroundStyle(Ember.C.muted.opacity(0.65))
                }
            }
        }
        .padding(.horizontal, Ember.S.screenPad)
        .padding(.top, 16)
        .padding(.bottom, 12)
        .overlay(alignment: .top) { Ember.C.hairline.frame(height: 1) }
    }
}

/// Amplitude-reactive coal row. Driven by `TimelineView(.animation)` — a
/// continuous per-frame sine, not a discrete SwiftUI `Animation` — so it
/// isn't a fourth budgeted motion, just a live readout. `push_pcm` RMS
/// (C3) replaces `sine(t)` here; nothing else about this view changes.
private struct BellowsBed: View {
    var listening: Bool
    var onTap: () -> Void

    private let cellCount = 7

    var body: some View {
        TimelineView(.animation) { context in
            let t = context.date.timeIntervalSinceReferenceDate
            ZStack {
                RoundedRectangle(cornerRadius: 12).fill(Color(hex: 0x0a0806))
                HStack(spacing: 6) {
                    ForEach(0..<cellCount, id: \.self) { i in
                        let phase = Double(i) * 0.55
                        let amp = listening ? (0.30 + 0.70 * abs(sin(t * 2.1 + phase))) : 0.08
                        Capsule()
                            .fill(
                                RadialGradient(
                                    colors: [Ember.C.heatCore, Ember.C.heatHot, Ember.C.heatDeep],
                                    center: .center, startRadius: 0, endRadius: 20
                                )
                            )
                            .opacity(0.16 + 0.84 * amp)
                            .frame(width: 20, height: 10 + 26 * amp)
                    }
                }
                RoundedRectangle(cornerRadius: 12)
                    .stroke(Ember.C.hairline, lineWidth: 1)
            }
        }
        .contentShape(Rectangle())
        .onTapGesture(perform: onTap)
    }
}
