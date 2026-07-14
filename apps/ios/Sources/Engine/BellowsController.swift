import Foundation

// The Swift-side seam for real voice capture (plan Task E4 real half /
// C3). Deliberately FFI-free in its OWN declarations (no AthanorCoreFFI
// import here) so `SessionScreen` — which must compile in both the demo and
// real projects — can hold an `(any BellowsController)?` without any
// `#if canImport` in the shared screen file. Only `BellowsFactory`'s BODY
// (below) is guarded; its signature is not, so the factory itself always
// exists, just returns nil in the demo build.
//
// This is the audio-capture half only — render + capture, no STT logic
// (rmp invariant): `SttStream`/`BellowsHandle` decide everything about
// decoding, endpointing, and bias; this just feeds PCM in and relays
// whatever comes back out as UI-shaped events.
@MainActor
protocol BellowsController: AnyObject {
    /// Events as they arrive. One stream per controller instance.
    var events: AsyncStream<BellowsEvent> { get }

    /// Requests mic permission, installs the capture tap, starts polling.
    /// Yields `.permissionDenied` (not a thrown error) if the learner says no
    /// — the caller degrades to the typed fallback, no retry nagging.
    func start()

    /// Tears down the tap/session. Safe to call multiple times.
    func stop()

    /// Tap-to-bank: true dims the bed and stops feeding PCM (mute), without
    /// tearing down capture. `false` un-mutes.
    func setMuted(_ muted: Bool)

    /// Tap-to-send-now (tapping the settled transcript text, spec: "tapping
    /// the settled text sends immediately"): flush + finalize whatever's
    /// pending, as if silence had just latched.
    func sendNow()

    /// A snapshot of responsiveness metrics for the dev overlay, or nil if the
    /// controller has no metrics source (the demo path). FFI-free by design:
    /// `RealBellowsController` maps `BellowsHandle.metrics()` into this plain
    /// struct so `SessionScreen` never imports AthanorCoreFFI.
    func currentMetrics() -> SttMetricsSnapshot?
}

extension BellowsController {
    // Default: no metrics (demo controller, or any future stub). Only the real
    // whisper-backed controller overrides this.
    func currentMetrics() -> SttMetricsSnapshot? { nil }
}

/// Plain, FFI-free projection of `stt::SttMetrics` (via `FfiSttMetrics`) for the
/// responsiveness overlay. See the Rust `SttMetrics` docs for field semantics:
/// decode wall-time, decoded-window length, realtime factor (< 1.0 = faster
/// than realtime on-device), decode-pass count, whether Metal/GPU was requested
/// (true on device, false on the Simulator), and the last utterance-end latency
/// (the felt "time to send").
struct SttMetricsSnapshot: Equatable {
    var lastDecodeMs: UInt64
    var lastWindowMs: UInt64
    var realtimeFactor: Double
    var decodePasses: UInt64
    var gpuRequested: Bool
    var utteranceEndLatencyMs: UInt64
}

enum BellowsEvent: Equatable {
    /// 0...1 live amplitude, RMS over the same buffer pushed to `push_pcm`
    /// (plan §2: "from the SAME buffers fed to push_pcm — no second audio
    /// path"). Rendered via `Ember.Motion.bellowsEmbers` (the budgeted
    /// spring), not a new animation.
    case amplitude(Double)
    /// Volatile preview tail (mercury-shimmer) — replaces wholesale each tick,
    /// never persisted.
    case previewTail(String)
    /// A finalized segment, append-only, settled/cooled into the transcript.
    case finalizedAppend(String)
    /// Endpointing latched sustained trailing silence — caller should send
    /// the accumulated utterance now.
    case utteranceEnded
    /// Mic permission was refused. No mic UI is shown again this session;
    /// the caller falls back to the typed keyboard path.
    case permissionDenied
}

/// Bias terms for `BellowsHandle.open` — active-domain vocab + recent salt,
/// assembled from the ordinary read surface (grimoire()/mercury()), never
/// from a second Store access. Kept here (not FFI-gated) so both the real
/// controller and any future test can share the exact same assembly rule.
enum BellowsBias {
    @MainActor
    static func terms(engine: any AthanorEngineProtocol) -> [String] {
        let domains = Set(engine.mercury().map(\.domain)).union(engine.grimoire().flatMap(\.domains))
        let recentSalt = engine.grimoire()
            .sorted { $0.date > $1.date }
            .prefix(3)
            .map(\.text)
        return Array(domains) + recentSalt
    }
}

/// The only seam SessionScreen needs: always declared (so it compiles in
/// both projects), body gated on `AthanorCoreFFI` availability. Returns nil
/// whenever the real path isn't available — no model yet, FFI not linked,
/// or the handle failed to open (bad model file, etc).
enum BellowsFactory {
    @MainActor
    static func makeRealController(modelPath: String, tier: ModelTier, biasTerms: [String]) -> (any BellowsController)? {
        #if canImport(AthanorCoreFFI)
        do {
            return try RealBellowsController(modelPath: modelPath, tier: tier, biasTerms: biasTerms)
        } catch {
            NSLog("[Athanor] BellowsHandle.open failed: \(error)")
            return nil
        }
        #else
        return nil
        #endif
    }
}
