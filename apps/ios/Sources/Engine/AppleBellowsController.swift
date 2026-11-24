import Foundation
import os

#if canImport(AthanorCoreFFI)
import AVFAudio
import Speech
import AthanorCoreFFI

// The Apple-speech Bellows (Bellows on Apple Speech plan, Task 7). Render and
// capture ONLY: this class owns AVAudioEngine capture, the SFSpeechRecognizer
// requests, and event forwarding. Rust (`TextBellowsHandle` -> `stt::TextBellows`)
// owns endpointing, the append-only commit, request cycling and the turn end
// (rmp invariant #1: no business logic in Swift). Swift forwards 16 kHz PCM and
// recognizer text in, and obeys `CycleRequest` / `Unavailable` out.
//
// Recognition is ON-DEVICE ONLY: `requiresOnDeviceRecognition = true`, and if
// the device can't do it we fall to the typed path (`.permissionDenied`) — never
// to server recognition, never silently to whisper.
@MainActor
final class AppleBellowsController: BellowsController {
    // `nonisolated` — touched from the CoreAudio tap callback and the poll task.
    // `TextBellowsHandle` is internally synchronized in Rust (a Mutex), and
    // `AsyncStream.Continuation.yield` is safe from any thread.
    nonisolated private let handle: AthanorCoreFFI.TextBellowsHandle
    private let recognizer = SFSpeechRecognizer(locale: Locale(identifier: "en-US"))
    private let engine = AVAudioEngine()
    private let contextual: [String]
    // The tap thread appends to the current request; rotation swaps it. The request isn't Sendable:
    nonisolated private let current = OSAllocatedUnfairLock<SFSpeechAudioBufferRecognitionRequest?>(uncheckedState: nil)
    private var live: [UInt64: (task: SFSpeechRecognitionTask, request: SFSpeechAudioBufferRecognitionRequest)] = [:]
    nonisolated(unsafe) private var muted = false
    nonisolated(unsafe) private var converter: AVAudioConverter?   // built from the incoming buffer format
    private var pollTask: Task<Void, Never>?
    private var capturing = false
    nonisolated private static let log = Logger(subsystem: "com.damsac.athanor", category: "bellows")

    let events: AsyncStream<BellowsEvent>
    nonisolated private let continuation: AsyncStream<BellowsEvent>.Continuation

    init(biasTerms: [String]) {
        handle = TextBellowsHandle(biasTerms: biasTerms, silenceLatchMs: RealBellowsController.defaultSilenceLatchMs)
        contextual = handle.contextualStrings()          // the cap and dedupe are decided in Rust
        (events, continuation) = AsyncStream<BellowsEvent>.makeStream()
    }

    // MARK: - BellowsController

    nonisolated func currentMetrics() -> SttMetricsSnapshot? {
        let m = handle.metrics()
        return SttMetricsSnapshot(
            engine: "apple",
            pickupMs: m.pickupMs,
            latchMs: m.latchMs,
            timeouts: m.timeouts,
            lastDecodeMs: 0,
            lastWindowMs: 0,
            realtimeFactor: 0,
            decodePasses: m.requests,
            gpuRequested: false,
            utteranceEndLatencyMs: m.finalizeMs
        )
    }

    func start() {
        guard !capturing else { return }
        Task { [weak self] in
            guard let self else { return }
            let mic = await AVAudioApplication.requestRecordPermission()
            let speech = await withCheckedContinuation { (c: CheckedContinuation<Bool, Never>) in
                SFSpeechRecognizer.requestAuthorization { c.resume(returning: $0 == .authorized) }
            }
            guard mic, speech else {
                Self.log.info("AppleBellows refused: mic=\(mic) speech=\(speech)")
                self.continuation.yield(.permissionDenied)
                return
            }
            guard let recognizer = self.recognizer, recognizer.supportsOnDeviceRecognition, recognizer.isAvailable else {
                Self.log.info("AppleBellows refused: on-device recognition unavailable")
                self.continuation.yield(.permissionDenied)
                return
            }
            do {
                try self.startCapture()
            } catch {
                Self.log.error("AppleBellows capture failed to start: \(String(describing: error), privacy: .public)")
                self.continuation.yield(.permissionDenied)
                return
            }
            self.capturing = true
            self.startRequest()
            self.startPollLoop()
            Self.log.info("AppleBellows started on_device=true contextual=\(self.contextual.count)")
        }
    }

    func stop() {
        pollTask?.cancel()
        pollTask = nil
        if capturing {
            engine.inputNode.removeTap(onBus: 0)
            engine.stop()
            try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
        }
        capturing = false
        current.withLockUnchecked { $0 = nil }
        for (_, entry) in live {
            entry.request.endAudio()
            entry.task.cancel()
        }
        live.removeAll()
        continuation.finish()
    }

    func setMuted(_ muted: Bool) {
        self.muted = muted
        if muted { handle.resetEndpoint() }
    }

    func sendNow() {
        handle.sendNow()   // the poll loop then delivers the cycle, the final and utteranceEnded
    }

    // MARK: - Capture

    private func startCapture() throws {
        let session = AVAudioSession.sharedInstance()
        try session.setCategory(.record, mode: .measurement, options: [])
        try session.setActive(true)
        let input = engine.inputNode
        let inputFormat = input.outputFormat(forBus: 0)
        // 4096 frames at the input's native format.
        input.installTap(onBus: 0, bufferSize: 4096, format: inputFormat) { [weak self] buffer, _ in
            self?.ingest(buffer)
        }
        engine.prepare()
        try engine.start()
    }

    /// The single audio entry point (the tap, and the DEBUG file injector, both call it).
    nonisolated func ingest(_ buffer: AVAudioPCMBuffer) {
        guard !muted else { return }                       // muted: skip BOTH the append and pushPcm
        current.withLockUnchecked { $0?.append(buffer) }
        guard let samples = convertTo16kMono(buffer) else { return }
        handle.pushPcm(pcm: samples)
        let amplitude = Double(min(Self.rms(samples) * 5, 1.0))  // same rms, same buffers: identical ember bed
        continuation.yield(.amplitude(amplitude))
    }

    /// 16 kHz mono f32, converter built (and cached) from the incoming buffer's own format.
    nonisolated private func convertTo16kMono(_ buffer: AVAudioPCMBuffer) -> [Float]? {
        guard let target = AVAudioFormat(
            commonFormat: .pcmFormatFloat32, sampleRate: 16_000, channels: 1, interleaved: false
        ) else { return nil }
        if converter == nil || converter?.inputFormat != buffer.format {
            converter = AVAudioConverter(from: buffer.format, to: target)
        }
        guard let converter else { return nil }
        let capacity = AVAudioFrameCount(
            target.sampleRate * Double(buffer.frameLength) / max(buffer.format.sampleRate, 1)
        ) + 32
        guard let out = AVAudioPCMBuffer(pcmFormat: target, frameCapacity: capacity) else { return nil }
        var conversionError: NSError?
        var delivered = false
        let block: AVAudioConverterInputBlock = { _, status in
            if delivered {
                status.pointee = .noDataNow
                return nil
            }
            delivered = true
            status.pointee = .haveData
            return buffer
        }
        converter.convert(to: out, error: &conversionError, withInputFrom: block)
        guard conversionError == nil, let channel = out.floatChannelData, out.frameLength > 0 else { return nil }
        return Array(UnsafeBufferPointer(start: channel[0], count: Int(out.frameLength)))
    }

    nonisolated static func rms(_ samples: [Float]) -> Float {
        guard !samples.isEmpty else { return 0 }
        let sumSquares = samples.reduce(Float(0)) { $0 + $1 * $1 }
        return sqrt(sumSquares / Float(samples.count))
    }

    // MARK: - Recognition requests

    /// Begins a new request (Rust assigns the id), swaps it into the tap path, starts its task.
    private func startRequest() {
        let id = handle.beginRequest()
        guard id != 0, let recognizer else { return }       // 0: Rust has halted (self-close storm)
        let request = SFSpeechAudioBufferRecognitionRequest()
        request.shouldReportPartialResults = true
        request.requiresOnDeviceRecognition = true
        request.contextualStrings = contextual
        request.addsPunctuation = true
        request.taskHint = .dictation
        current.withLockUnchecked { $0 = request }
        let h = handle
        let task = recognizer.recognitionTask(with: request) { [weak self] result, error in
            // Never touches the UI: forward to Rust, then drop our bookkeeping on the main actor.
            if let result {
                let text = result.bestTranscription.formattedString
                if result.isFinal {
                    h.ingestFinal(requestId: id, text: text)
                    Task { @MainActor in self?.live[id] = nil }
                } else {
                    h.ingestPartial(requestId: id, text: text)
                }
            } else if error != nil {
                h.ingestError(requestId: id)
                Task { @MainActor in self?.live[id] = nil }
            }
        }
        live[id] = (task: task, request: request)
    }

    /// Rust asked for a cycle. Swap the new request in BEFORE ending the old one, so no audio is lost at the seam.
    private func rotate(from id: UInt64) {
        Self.log.info("cycle from=\(id)")
        startRequest()
        live[id]?.request.endAudio()   // the old task stays alive so its final can arrive
    }

    // MARK: - Poll loop (cheap: no decode)

    private func startPollLoop() {
        pollTask = Task.detached { [weak self] in
            while !Task.isCancelled {
                guard let self else { return }
                for event in self.handle.poll() {
                    switch event {
                    case .preview(let text):
                        self.continuation.yield(.previewTail(text))
                    case .finalized(let text):
                        #if DEBUG
                        Self.log.info("finalized: \(text, privacy: .public)")
                        #else
                        Self.log.info("finalized words=\(text.filter { $0 == " " }.count + 1)")
                        #endif
                        self.continuation.yield(.finalizedAppend(text))
                    case .utteranceEnded:
                        let m = self.handle.metrics()
                        Self.log.info("utterance ended finalize_ms=\(m.finalizeMs) latch_ms=\(m.latchMs) timeouts=\(m.timeouts) pickup_ms=\(m.pickupMs) requests=\(m.requests) self_closes=\(m.selfCloses) resets=\(m.resets) revisions_dropped=\(m.revisionsDropped)")
                        self.continuation.yield(.utteranceEnded)
                    case .cycleRequest(let id):
                        await self.rotate(from: id)
                    case .unavailable:
                        // Rust's verdict (R4): the recognizer keeps closing itself with no words.
                        Self.log.error("recognizer unavailable: self-close storm")
                        self.continuation.yield(.permissionDenied)
                        await self.stop()
                    }
                }
                try? await Task.sleep(nanoseconds: 50_000_000)
            }
        }
    }
}
#endif
