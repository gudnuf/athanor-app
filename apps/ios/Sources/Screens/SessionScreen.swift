import SwiftUI
import os

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

    /// An engine/session error surfaced from the event stream (never a stock
    /// alert): rendered as a calm, in-palette line rather than swallowed. The
    /// transcript is preserved beneath it — a mid-session error doesn't erase
    /// what was already said.
    @State private var sessionError: String?

    private static let log = Logger(subsystem: "com.gudnuf.athanor", category: "session")

    // Real Bellows (E4 real half) — nil in the demo build/path, where the
    // sine-stub `Bellows` view below is used unchanged.
    @State private var realBellows: (any BellowsController)?
    @State private var realMuted = false
    @State private var micDenied = false
    @State private var amplitude: Double = 0
    /// Finalized-but-not-yet-sent text for the current utterance (cooled/settled).
    @State private var liveCooled = ""
    /// Volatile preview tail (mercury-shimmer), replaced wholesale each tick.
    @State private var livePreview = ""

    var body: some View {
        VStack(spacing: 0) {
            header

            // The transcript region ALWAYS renders something (never a blank
            // void): a calm listening invitation before the first word lands,
            // otherwise the streamed dialogue. An engine error surfaces as a
            // calm in-palette line beneath whatever's there — never swallowed,
            // never a stock alert.
            if messages.isEmpty && !isStreaming {
                ListeningInvitation(error: sessionError)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
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
                            if let sessionError {
                                SessionErrorLine(message: sessionError).id("error")
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
                    .onChange(of: sessionError) { _, err in
                        if err != nil { proxy.scrollTo("error", anchor: .bottom) }
                    }
                }
            }

            if let realBellows {
                RealBellows(
                    controller: realBellows,
                    amplitude: amplitude,
                    liveCooled: liveCooled,
                    livePreview: livePreview,
                    muted: $realMuted,
                    micDenied: micDenied,
                    showKeyboard: $showKeyboard,
                    typedText: $typedText,
                    fieldFocused: $typedFieldFocused,
                    onSendCooledNow: { realBellows.sendNow() },
                    onSubmitTyped: submitTyped
                )
            } else if model.engine.isReal {
                // Real build, real Bellows not up yet (model still downloading,
                // or no mic path this launch). NEVER fall back to the demo sine
                // bed here: its tap sends the literal "(bellows: demo utterance)"
                // string, which on the real engine would be posted to the live
                // Mystagogue as the learner's actual words. Instead offer the
                // legitimate real-path affordance — the typed field, always
                // available — with the quiet warming presence covering the wait.
                RealFallbackInput(
                    downloadState: model.modelDownloader.state,
                    typedText: $typedText,
                    fieldFocused: $typedFieldFocused,
                    onSubmitTyped: submitTyped
                )
            } else {
                Bellows(
                    listening: $listening,
                    showKeyboard: $showKeyboard,
                    typedText: $typedText,
                    fieldFocused: $typedFieldFocused,
                    onTapBed: sendDemoTurn,
                    onSubmitTyped: submitTyped
                )
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Ember.C.ground.ignoresSafeArea())
        .task { await begin() }
        .task { await beginRealBellows() }
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
        .task {
            // QA hook only (same launch-arg pattern as `autoplay=1`/`screen=`)
            // — submits a fixed turn as if typed, so the live-engine round
            // trip is exercisable in environments with no real mic/speaker
            // acoustic loopback (e.g. a headless sim host) and no UI-tap
            // automation available. Real voice capture is verified
            // separately (RealBellows genuinely decodes real PCM — see
            // RealBellowsController); this hook only stands in for "a
            // learner turn arrived," same as the typed keyboard fallback
            // already does in the shipped UI. Never fires without the arg.
            guard ProcessInfo.processInfo.arguments.contains("debug-send-turn=1") else { return }
            try? await Task.sleep(for: .milliseconds(2500))
            let text = "What's the thread I keep circling back to."
            messages.append(.learner(id: UUID().uuidString, text: text))
            model.engine.sendTurn(text)
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
        sessionError = nil
        messages.append(.learner(id: UUID().uuidString, text: trimmed))
        typedText = ""
        showKeyboard = false
        model.engine.sendTurn(trimmed)
    }

    /// True when `text` carries no actual speech — only whisper non-speech
    /// markers like "[ Silence ]", "[BLANK_AUDIO]", "(silence)", or runs of
    /// them ("[ Silence ] [BLANK_AUDIO]"). Strips every `[...]`/`(...)` group
    /// and asks whether anything spoken is left; a genuine utterance always
    /// leaves words behind ("I think (maybe) yes" survives), so this can't eat
    /// real speech.
    ///
    /// NOTE: this is a presentation-layer guard, not STT logic — the durable
    /// fix belongs in `crates/stt` (drop non-speech segments at the source).
    /// Kept minimal here so a live turn on the Simulator (ambient silence →
    /// "[BLANK_AUDIO]") isn't polluted before that lands.
    static func isNonSpeechArtifact(_ text: String) -> Bool {
        let t = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !t.isEmpty else { return true }
        let stripped = t.replacing(/[\[(][^\[\]()]*[\])]/, with: " ")
            .trimmingCharacters(in: .whitespacesAndNewlines)
        return stripped.isEmpty
    }

    private func close() {
        realBellows?.stop()
        Task {
            await model.engine.endSession(abandon: false)
            onClose()
        }
    }

    private func begin() async {
        let stream: AsyncStream<SessionEvent>
        do {
            stream = try model.engine.beginSession(threadId: nil)
        } catch {
            // The session couldn't even open — surface it calmly instead of
            // leaving the learner on a screen that never comes alive.
            Self.log.error("beginSession failed: \(error.localizedDescription, privacy: .public)")
            sessionError = "The fire wouldn't catch just now. Close and try again."
            return
        }
        // DemoEngine's canned script only advances on a `sendTurn` call, so a
        // synthetic kickoff plays its opening line with no real interaction
        // yet. The REAL engine's Conductor opens the Socratic turn itself
        // from the assembled SessionPlan — sending a fake "(session opens)"
        // string would inject it as the learner's actual first utterance to
        // the live model, so this kickoff is demo-only.
        if !model.engine.isReal {
            model.engine.sendTurn("(session opens)")
        }
        for await event in stream {
            switch event {
            case .textDelta(let chunk, let register):
                if !isStreaming {
                    isStreaming = true
                    streamingRegister = register
                    streamingText = ""
                    sessionError = nil // a reply is flowing again — clear any prior error
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
            case .error(let message):
                // Surface, don't swallow. A partial reply already streamed
                // stays as a finalized line; the error sits calmly below it.
                Self.log.error("session turn error: \(message, privacy: .public)")
                if isStreaming, !streamingText.isEmpty {
                    messages.append(.teacher(id: UUID().uuidString, text: streamingText, register: streamingRegister))
                }
                isStreaming = false
                streamingText = ""
                sessionError = "The Mystagogue lost the thread for a moment. Say it again when you're ready."
            case .toolCall:
                break
            }
        }
    }

    /// Opens the Bellows against the model F1 already downloaded, with bias
    /// terms assembled from the ordinary read surface (BellowsBias —
    /// grimoire()/mercury(), never a second Store access).
    ///
    /// Deliberately NOT gated on `model.engine.isReal`: the Bellows (mic +
    /// on-device whisper STT) needs no Anthropic key at all — only the FFI
    /// build + a ready model file. Gating on `isReal` would make it
    /// impossible to exercise real audio capture without a live key. The
    /// actual "real build vs demo build" gate lives in `BellowsFactory`
    /// (compiles to `nil` under `#if canImport(AthanorCoreFFI)` absence) —
    /// that's the one seam this screen needs; a no-key real build still
    /// gets real ears even while `sendTurn` reaches DemoEngine's fallback.
    private func beginRealBellows() async {
        // The model may still be mid-download if the learner reaches a
        // session unusually fast after first launch (normally Initiation
        // covers this wait) — poll briefly rather than giving up on a single
        // snapshot. Bounded so a session with no model yet (or none at all,
        // e.g. demo build) still opens promptly.
        var modelPath = model.modelDownloader.readyPath
        var attempts = 0
        while modelPath == nil, attempts < 40 {
            try? await Task.sleep(nanoseconds: 250_000_000)
            modelPath = model.modelDownloader.readyPath
            attempts += 1
        }
        guard let modelPath else { return }
        let bias = BellowsBias.terms(engine: model.engine)
        guard let controller = BellowsFactory.makeRealController(
            modelPath: modelPath, tier: model.modelTier, biasTerms: bias
        ) else { return }
        realBellows = controller
        controller.start()
        for await event in controller.events {
            switch event {
            case .amplitude(let level):
                withAnimation(Ember.Motion.bellowsEmbers) { amplitude = level }
            case .previewTail(let text):
                // Drop whisper's non-speech markers ("[ Silence ]",
                // "[BLANK_AUDIO]", "(silence)"…) from the live shimmer so the
                // preview reads as speech, not decoder chatter — otherwise the
                // sim's ambient silence fills the transcript with brackets.
                livePreview = Self.isNonSpeechArtifact(text) ? "" : text
            case .finalizedAppend(let text):
                // Belt-and-suspenders alongside whisper's own `suppress_nst`
                // (crates/stt): drop any whole-segment non-speech marker that
                // still slips through, so it can't become the learner's
                // utterance (it would be sent verbatim to the live Mystagogue).
                guard !Self.isNonSpeechArtifact(text) else { break }
                liveCooled = liveCooled.isEmpty ? text : liveCooled + " " + text
                livePreview = ""
            case .utteranceEnded:
                sendLiveUtterance()
            case .permissionDenied:
                // Quiet, no nagging: fall back to the typed field and never
                // ask again this session.
                micDenied = true
                showKeyboard = true
            }
        }
    }

    private func sendLiveUtterance() {
        let text = liveCooled.trimmingCharacters(in: .whitespacesAndNewlines)
        liveCooled = ""
        livePreview = ""
        guard !text.isEmpty, !Self.isNonSpeechArtifact(text) else { return }
        sessionError = nil
        messages.append(.learner(id: UUID().uuidString, text: text))
        model.engine.sendTurn(text)
    }
}

// MARK: - Never-blank states

/// Shown in the transcript region before the first word lands (or if a session
/// fails to open at all). The point: entering a session ALWAYS renders an
/// intentional, calm listening state — the fire is lit and waiting — never a
/// black void. No budgeted motion (the ember bed below carries the life); this
/// is a still, in-palette invitation.
private struct ListeningInvitation: View {
    var error: String?

    var body: some View {
        VStack(spacing: 14) {
            Text(Ember.Glyph.fireMask)
                .font(.system(size: 30))
                .foregroundStyle(Ember.C.heat.opacity(0.85))
            if let error {
                Text(error)
                    .font(Ember.F.serif(16, italic: true))
                    .foregroundStyle(Ember.C.muted)
                    .multilineTextAlignment(.center)
                    .fixedSize(horizontal: false, vertical: true)
            } else {
                Text("The fire is lit.")
                    .font(Ember.F.serif(19))
                    .foregroundStyle(Ember.C.ink)
                Text("Speak when you're ready — or tap the keyboard.")
                    .font(Ember.F.serif(14, italic: true))
                    .foregroundStyle(Ember.C.mutedDim)
                    .multilineTextAlignment(.center)
            }
        }
        .padding(.horizontal, Ember.S.screenPad + 12)
    }
}

/// A mid-session error, surfaced calmly inline beneath the transcript (serif,
/// muted, in-palette) — never a stock alert, never a swallow. The conversation
/// above it stays intact.
private struct SessionErrorLine: View {
    var message: String

    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            Text(Ember.Glyph.fireMask)
                .foregroundStyle(Ember.C.heat.opacity(0.7))
            Text(message)
                .font(Ember.F.serif(14, italic: true))
                .foregroundStyle(Ember.C.muted)
                .fixedSize(horizontal: false, vertical: true)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.top, 6)
    }
}

/// Real-build input when the real Bellows isn't up yet (whisper model still
/// downloading, or no mic path this launch). The typed field is the always-
/// available legitimate real-path affordance; the quiet `WarmingLine` covers
/// the model wait without a spinner. Deliberately NOT the demo sine bed — that
/// path's tap injects a canned string into the LIVE engine.
private struct RealFallbackInput: View {
    var downloadState: ModelDownloader.State
    @Binding var typedText: String
    var fieldFocused: FocusState<Bool>.Binding
    var onSubmitTyped: () -> Void

    var body: some View {
        VStack(spacing: 10) {
            WarmingLine(state: downloadState)

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
        }
        .padding(.horizontal, Ember.S.screenPad)
        .padding(.top, 16)
        .padding(.bottom, 12)
        .overlay(alignment: .top) { Ember.C.hairline.frame(height: 1) }
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

// MARK: - The real Bellows (E4 real half)

/// The live transcript + real ember bed, driven by a `BellowsController`.
/// Layout mirrors the demo `Bellows`/`BellowsBed` pair on purpose (same
/// screen position, same keyboard glyph) so the swap in SessionScreen's body
/// is a data-source change, not a different screen. Internal (not private):
/// `InitiationScreen` reuses this so the real path's first session gets the
/// same legitimate input affordances a real session screen has, instead of
/// injecting a fake kickoff string (review BLOCKER-1).
struct RealBellows: View {
    var controller: any BellowsController
    var amplitude: Double
    var liveCooled: String
    var livePreview: String
    @Binding var muted: Bool
    var micDenied: Bool
    @Binding var showKeyboard: Bool
    @Binding var typedText: String
    var fieldFocused: FocusState<Bool>.Binding
    var onSendCooledNow: () -> Void
    var onSubmitTyped: () -> Void

    var body: some View {
        VStack(spacing: 10) {
            if !liveCooled.isEmpty || !livePreview.isEmpty {
                transcript
            }

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
                RealBellowsBed(amplitude: amplitude, muted: muted) {
                    muted.toggle()
                    controller.setMuted(muted)
                }
                .frame(height: 46)
            }

            HStack {
                if micDenied {
                    Text("no mic access — type instead")
                        .font(Ember.F.sans(11))
                        .foregroundStyle(Ember.C.mutedDim)
                } else if !showKeyboard {
                    Text(muted ? "banked" : "listening — tap the bed to bank")
                        .font(Ember.F.sans(11))
                        .foregroundStyle(Ember.C.mutedDim)
                }
                Spacer()
                if !micDenied {
                    Button {
                        showKeyboard.toggle()
                        if showKeyboard { fieldFocused.wrappedValue = true }
                    } label: {
                        Text("⌨").font(.system(size: 16)).foregroundStyle(Ember.C.muted.opacity(0.65))
                    }
                }
            }
        }
        .padding(.horizontal, Ember.S.screenPad)
        .padding(.top, 16)
        .padding(.bottom, 12)
        .overlay(alignment: .top) { Ember.C.hairline.frame(height: 1) }
    }

    /// Cooled (settled/finalized) text upright and tappable — "tapping the
    /// settled text sends immediately" (spec). Volatile preview styled once,
    /// statically (italic + a fixed two-tone foreground) — a look, not a
    /// loop; shimmer here is styling, not a new animation.
    private var transcript: some View {
        (Text(liveCooled.isEmpty ? "" : liveCooled + " ")
            .font(Ember.F.sans(17))
            .foregroundStyle(Ember.C.ink)
        + Text(livePreview)
            .font(Ember.F.serif(17, italic: true))
            .foregroundStyle(Ember.C.muted)
        )
        .frame(maxWidth: .infinity, alignment: .leading)
        .fixedSize(horizontal: false, vertical: true)
        .animation(Ember.Motion.none, value: liveCooled)
        .animation(Ember.Motion.none, value: livePreview)
        .onTapGesture {
            guard !liveCooled.isEmpty else { return }
            onSendCooledNow()
        }
    }
}

/// Real-amplitude coal row. `amplitude` arrives from `RealBellowsController`
/// as RMS-over-push_pcm-buffer events (~12/s at the plan's tap cadence);
/// SwiftUI interpolates between ticks via `Ember.Motion.bellowsEmbers` (the
/// budgeted spring the caller wraps each update in) — no per-frame timer
/// needed here, unlike the demo sine stub. Internal: shared with
/// `RealBellows` (used by both SessionScreen and InitiationScreen).
struct RealBellowsBed: View {
    var amplitude: Double
    var muted: Bool
    var onTap: () -> Void

    private let cellCount = 7
    // Fixed per-cell spread so seven cells don't all move in lockstep off
    // one scalar — a look, not a spectral analysis (out of scope for E4).
    private static let spread: [Double] = [0.55, 0.8, 1.0, 0.85, 0.6, 0.9, 0.7]

    var body: some View {
        let level = muted ? 0.04 : amplitude
        ZStack {
            RoundedRectangle(cornerRadius: 12).fill(Color(hex: 0x0a0806))
            HStack(spacing: 6) {
                ForEach(0..<cellCount, id: \.self) { i in
                    let amp = min(level * Self.spread[i], 1.0)
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
        .contentShape(Rectangle())
        .onTapGesture(perform: onTap)
    }
}
