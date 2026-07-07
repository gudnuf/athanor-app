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

    // Real-path input (review BLOCKER-1): on a real build, the ONLY thing
    // that may become the learner's first utterance to the live Mystagogue
    // is something the learner actually said or typed — never a synthetic
    // kickoff string. Mirrors SessionScreen's real-Bellows state; see
    // `beginRealBellows()` below.
    @State private var realBellows: (any BellowsController)?
    @State private var realMuted = false
    @State private var micDenied = false
    @State private var amplitude: Double = 0
    @State private var liveCooled = ""
    @State private var livePreview = ""
    @State private var typedText = ""
    @State private var showKeyboard = false
    @FocusState private var typedFieldFocused: Bool

    var body: some View {
        VStack(spacing: 24) {
            Spacer()
            Text(Ember.Glyph.furnace)
                .font(.system(size: 34))
                .foregroundStyle(Ember.C.heat)
            VStack(spacing: 16) {
                ForEach(Array(lines.enumerated()), id: \.offset) { _, line in
                    initiationLine(line, formatted: true) // settled — render markdown
                }
                if let streaming {
                    initiationLine(streaming, formatted: false) // in-flight — raw, no reflow
                }
            }
            .padding(.horizontal, Ember.S.screenPad)
            Spacer()
            WarmingLine(state: model.modelDownloader.state)
                .padding(.horizontal, Ember.S.screenPad)
                .padding(.bottom, 4)

            if canFinish {
                enterFurnaceButton
            } else if model.engine.isReal {
                // The Mystagogue now speaks first on its own (BLOCKER-1 deep
                // fix): `beginInitiation()` fires the Conductor's ritual
                // opening turn before this screen ever renders `realInput`,
                // so by the time the learner sees this affordance, the
                // silence has already been broken from the other side.
                // `realInput` is still what lets THEM respond — typed/voice,
                // never a synthetic kickoff string.
                realInput
            } else {
                demoBeginButton
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Ember.C.ground.ignoresSafeArea())
        .task { await begin() }
        .task { await beginRealBellows() }
        .task {
            // Screenshot/QA automation hook only (mirrors murmur-rmp's
            // `autoflow=` launch arg) — taps Begin automatically so the
            // word-by-word streaming path can be captured mid-flight without
            // a real tap gesture. Never affects a normal launch. Demo-only:
            // the real path has no fake-tap equivalent to fire (see BLOCKER-1
            // fix above), so this hook is a no-op there.
            guard ProcessInfo.processInfo.arguments.contains("autoplay=1"), !model.engine.isReal else { return }
            try? await Task.sleep(for: .milliseconds(300))
            model.engine.sendTurn("(autoplay)")
        }
    }

    private var enterFurnaceButton: some View {
        Button {
            model.hasCompletedInitiation = true
        } label: {
            Text("Enter the Furnace")
                .font(Ember.F.sans(17, weight: .semibold))
                .foregroundStyle(Ember.C.ground)
                .frame(maxWidth: .infinity)
                .frame(height: Ember.S.buttonHeight)
                .background(Ember.C.heat, in: Capsule())
        }
        .padding(.horizontal, Ember.S.screenPad)
        .padding(.bottom, 32)
    }

    /// Demo path only — `DemoEngine`'s canned script advances on any
    /// `sendTurn` call, so a fixed tap string is a legitimate stand-in for
    /// "the learner said something" (it's fed to a fixed script, not a live
    /// model — see DemoEngine's own no-business-logic discipline). This is
    /// NEVER reachable when `model.engine.isReal` (see `body`).
    private var demoBeginButton: some View {
        Button {
            model.engine.sendTurn("(initiation demo tap)")
        } label: {
            Text("Begin")
                .font(Ember.F.sans(17, weight: .semibold))
                .foregroundStyle(Ember.C.ground)
                .frame(maxWidth: .infinity)
                .frame(height: Ember.S.buttonHeight)
                .background(Ember.C.heat, in: Capsule())
        }
        .padding(.horizontal, Ember.S.screenPad)
        .padding(.bottom, 32)
    }

    /// Real path (review BLOCKER-1 fix): the same input affordances a real
    /// session has — typed field always available, the real Bellows (voice)
    /// once the model's ready and the FFI is linked. Whatever the learner
    /// actually says/types becomes their real first turn; nothing synthetic
    /// is ever sent.
    @ViewBuilder
    private var realInput: some View {
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
                onSubmitTyped: submitRealTyped
            )
        } else {
            // Bellows not constructed yet (model still downloading, or this
            // launch has no mic path) — typed is the minimum legitimate
            // real-path affordance, always available.
            HStack(spacing: 10) {
                TextField("Say it your way…", text: $typedText, axis: .vertical)
                    .focused($typedFieldFocused)
                    .font(Ember.F.sans(15))
                    .foregroundStyle(Ember.C.ink)
                    .padding(.horizontal, 14)
                    .padding(.vertical, 10)
                    .background(Ember.C.raised2, in: RoundedRectangle(cornerRadius: 12))
                Button("Send", action: submitRealTyped)
                    .font(Ember.F.sans(14, weight: .semibold))
                    .foregroundStyle(Ember.C.heat)
            }
            .padding(.horizontal, Ember.S.screenPad)
            .padding(.bottom, 32)
        }
    }

    private func submitRealTyped() {
        let trimmed = typedText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        typedText = ""
        showKeyboard = false
        model.engine.sendTurn(trimmed)
    }

    // Settled lines render the Mystagogue's inline markdown; the in-flight
    // streaming line stays raw (same no-reflow reasoning as `StreamingText`).
    private func initiationLine(_ text: String, formatted: Bool) -> some View {
        Text(formatted ? .mystagogueInline(text) : AttributedString(text))
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

    /// Opens the Bellows against the model F1 already downloaded — mirrors
    /// SessionScreen's `beginRealBellows()` exactly (same bounded wait, same
    /// bias-term assembly, same event handling). Deliberately NOT gated on
    /// `model.engine.isReal` for the same reason as SessionScreen: the
    /// Bellows needs no Anthropic key, only the FFI build + a ready model.
    private func beginRealBellows() async {
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
                livePreview = text
            case .finalizedAppend(let text):
                liveCooled = liveCooled.isEmpty ? text : liveCooled + " " + text
                livePreview = ""
            case .utteranceEnded:
                sendLiveUtterance()
            case .permissionDenied:
                micDenied = true
                showKeyboard = true
            }
        }
    }

    private func sendLiveUtterance() {
        let text = liveCooled.trimmingCharacters(in: .whitespacesAndNewlines)
        liveCooled = ""
        livePreview = ""
        guard !text.isEmpty else { return }
        model.engine.sendTurn(text)
    }
}

/// A quiet presence, not a loading screen (F1 brief: "it should feel
/// incidental"). No spinner, no motion-budget animation — the bar's width
/// just steps to the current fraction (`Ember.Motion.none`); the only
/// screen-level motion here stays E3's (none) plus whatever Initiation
/// itself already spends. Invisible once ready, and quietly honest on
/// failure rather than stuck mid-progress forever.
///
/// Internal (not private): SessionScreen's real-path input fallback reuses
/// this so a "Tend the fire" session opened before the whisper model finished
/// downloading shows the same quiet warming presence instead of nothing.
struct WarmingLine: View {
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
