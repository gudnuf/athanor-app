import Foundation

#if canImport(AthanorCoreFFI)
import AVFAudio
import AthanorCoreFFI

// The real Bellows: AVAudioEngine capture (Swift, render+capture ONLY — no
// STT logic, rmp invariant) feeding `BellowsHandle` (crates/ffi/src/bellows.rs,
// wrapping `stt::SttStream`). Compiles ONLY when AthanorCoreFFI is linked
// (the real project) — the demo project never sees this file's body at all,
// which is the "compiles only in the real project" seam BellowsController.swift
// documents.
//
// Cadences (plan §2, hand-verified against the worked arithmetic):
//   - AVAudioEngine tap: 4096-frame buffer @ whatever the input node's native
//     rate is (commonly 48 kHz) → converted to 16 kHz mono f32 before
//     push_pcm. At 48 kHz that's ~85 ms / ~1365 samples per callback (review
//     edit #7: "tap-buffer-sized, not exactly 1600 samples" — this matches).
//   - poll() every ~250 ms on a background Task (never the realtime thread).
//   - preview_tail() at ~10 Hz for the UI shimmer cadence.
//   - Ember amplitude: RMS over the SAME buffer handed to push_pcm — no
//     second audio path (plan §2 "Ember amplitude").
@MainActor
final class RealBellowsController: BellowsController {
    private let handle: AthanorCoreFFI.BellowsHandle
    private let engine = AVAudioEngine()
    private var converter: AVAudioConverter?
    private var pollTask: Task<Void, Never>?
    private var previewTask: Task<Void, Never>?
    private var muted = false
    private var capturing = false

    let events: AsyncStream<BellowsEvent>
    private let continuation: AsyncStream<BellowsEvent>.Continuation

    // KNOWN SIMULATOR-ONLY RISK (E4 real-half verification, confirmed
    // reproducible): `BellowsHandle.open` → `whisper_init_with_params_no_state`
    // → ggml-metal buffer allocation traps with EXC_BREAKPOINT/SIGTRAP inside
    // `MTLSimDevice newBufferWithLength:...` on the iOS Simulator's software
    // Metal shim — before any audio capture starts. Root cause:
    // `crates/stt/src/whisper.rs` hardcodes `params.use_gpu(true)`, and the
    // Simulator's Metal translation layer chokes on ggml's buffer allocation
    // pattern (a known class of ggml-metal/Simulator incompatibility — real
    // device Metal is a different code path and is NOT expected to hit this).
    // This is a native trap, not a Swift error — no `do/catch` here can shield
    // it; it takes the whole process down. Fixing it means either giving
    // `crates/stt` a CPU-fallback toggle or confirming (G2/G3, on-device) that
    // real hardware never hits this — both out of E4's Swift-only scope.
    init(modelPath: String, tier: ModelTier, biasTerms: [String]) throws {
        let ffiTier: AthanorCoreFFI.BellowsTier = tier == .small ? .smallEn : .baseEn
        self.handle = try AthanorCoreFFI.BellowsHandle.open(modelPath: modelPath, biasTerms: biasTerms, tier: ffiTier)
        (self.events, self.continuation) = AsyncStream<BellowsEvent>.makeStream()
    }

    func start() {
        guard !capturing else { return }
        Task { [weak self] in
            guard let self else { return }
            let granted = await Self.requestMicPermission()
            guard granted else {
                self.continuation.yield(.permissionDenied)
                return
            }
            do {
                try self.installTapAndStart()
            } catch {
                NSLog("[Athanor] Bellows capture failed to start: \(error)")
                self.continuation.yield(.permissionDenied)
                return
            }
            self.capturing = true
            self.startPollLoop()
            self.startPreviewLoop()
        }
    }

    func stop() {
        pollTask?.cancel()
        previewTask?.cancel()
        pollTask = nil
        previewTask = nil
        if capturing {
            engine.inputNode.removeTap(onBus: 0)
            engine.stop()
            try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
        }
        capturing = false
    }

    func setMuted(_ muted: Bool) {
        self.muted = muted
        if muted { handle.resetEndpoint() }
    }

    func sendNow() {
        guard let tail = try? handle.end() else { return }
        for segment in tail where !segment.text.isEmpty {
            continuation.yield(.finalizedAppend(segment.text))
        }
        continuation.yield(.utteranceEnded)
        handle.resetEndpoint()
    }

    // MARK: - Capture

    private static func requestMicPermission() async -> Bool {
        await AVAudioApplication.requestRecordPermission()
    }

    private func installTapAndStart() throws {
        let session = AVAudioSession.sharedInstance()
        try session.setCategory(.record, mode: .measurement, options: [])
        try session.setActive(true)

        let input = engine.inputNode
        let inputFormat = input.outputFormat(forBus: 0)
        guard let targetFormat = AVAudioFormat(
            commonFormat: .pcmFormatFloat32, sampleRate: 16_000, channels: 1, interleaved: false
        ) else {
            throw BellowsCaptureError.formatUnavailable
        }
        guard let converter = AVAudioConverter(from: inputFormat, to: targetFormat) else {
            throw BellowsCaptureError.formatUnavailable
        }
        self.converter = converter

        // 4096 frames at the input's native rate (plan §2) — off the realtime
        // thread for everything past the conversion itself.
        input.installTap(onBus: 0, bufferSize: 4096, format: inputFormat) { [weak self] buffer, _ in
            guard let self else { return }
            let capacity = AVAudioFrameCount(
                targetFormat.sampleRate * Double(buffer.frameLength) / max(inputFormat.sampleRate, 1)
            ) + 32
            guard let outBuffer = AVAudioPCMBuffer(pcmFormat: targetFormat, frameCapacity: capacity) else { return }
            var conversionError: NSError?
            var delivered = false
            let inputBlock: AVAudioConverterInputBlock = { _, outStatus in
                if delivered {
                    outStatus.pointee = .noDataNow
                    return nil
                }
                delivered = true
                outStatus.pointee = .haveData
                return buffer
            }
            converter.convert(to: outBuffer, error: &conversionError, withInputFrom: inputBlock)
            guard conversionError == nil, let channelData = outBuffer.floatChannelData else { return }
            let frameCount = Int(outBuffer.frameLength)
            guard frameCount > 0 else { return }
            let samples = Array(UnsafeBufferPointer(start: channelData[0], count: frameCount))
            let rms = Self.rms(samples)

            // Hop off the realtime thread for the FFI push + UI event (plan
            // §2: "push ~100 ms chunks over FFI ... off the realtime thread").
            Task { [weak self] in
                guard let self, !self.muted else { return }
                self.handle.pushPcm(pcm: samples)
                self.continuation.yield(.amplitude(Double(min(rms * 5, 1.0))))
            }
        }

        engine.prepare()
        try engine.start()
    }

    private static func rms(_ samples: [Float]) -> Float {
        guard !samples.isEmpty else { return 0 }
        let sumSquares = samples.reduce(Float(0)) { $0 + $1 * $1 }
        return sqrt(sumSquares / Float(samples.count))
    }

    // MARK: - Poll / preview loops (background — never the realtime thread)

    private func startPollLoop() {
        pollTask = Task { [weak self] in
            while !Task.isCancelled {
                guard let self else { return }
                if let segments = try? self.handle.poll() {
                    for segment in segments where !segment.text.isEmpty {
                        self.continuation.yield(.finalizedAppend(segment.text))
                    }
                }
                if self.handle.utteranceEnded() {
                    self.continuation.yield(.utteranceEnded)
                }
                try? await Task.sleep(nanoseconds: 250_000_000)
            }
        }
    }

    private func startPreviewLoop() {
        previewTask = Task { [weak self] in
            while !Task.isCancelled {
                guard let self else { return }
                self.continuation.yield(.previewTail(self.handle.previewTail()))
                try? await Task.sleep(nanoseconds: 100_000_000) // ~10 Hz (plan §2)
            }
        }
    }
}

private enum BellowsCaptureError: Error {
    case formatUnavailable
}
#endif
