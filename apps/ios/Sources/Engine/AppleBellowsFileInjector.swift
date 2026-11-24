import Foundation
import os

#if canImport(AthanorCoreFFI) && DEBUG
import AVFAudio

// DEBUG-only QA hook (Bellows on Apple Speech plan, Task 10): the simulator has
// no usable microphone, so `bellows-audio-file=<path>` feeds a deterministic
// audio file through the SAME `AppleBellowsController.ingest(_:)` path the real
// tap uses (recognizer append + 16 kHz conversion + Rust pushPcm + ember RMS).
// It is a separate file so the controller itself stays free of text/arg helpers.
extension AppleBellowsController {
    static func injectedAudioURL() -> URL? {
        let prefix = "bellows-audio-file="
        guard let arg = ProcessInfo.processInfo.arguments.first(where: { $0.hasPrefix(prefix) }) else { return nil }
        return URL(fileURLWithPath: String(arg.dropFirst(prefix.count)))
    }

    /// Realtime-paced feed of `url` in 4096-frame buffers, then 2 s of silence
    /// in the same format so the Rust latch fires without any padding tool.
    func injectFile(_ url: URL) {
        Task.detached { [weak self] in
            let log = Logger(subsystem: "com.damsac.athanor", category: "bellows")
            guard let file = try? AVAudioFile(forReading: url) else {
                log.error("bellows-audio-file: cannot open \(url.lastPathComponent, privacy: .public)")
                return
            }
            let format = file.processingFormat
            log.info("bellows-audio-file: injecting \(url.lastPathComponent, privacy: .public) rate=\(format.sampleRate) ch=\(format.channelCount) frames=\(file.length)")
            let frames: AVAudioFrameCount = 4096
            func pace(_ n: AVAudioFrameCount) async {
                try? await Task.sleep(nanoseconds: UInt64(Double(n) / format.sampleRate * 1_000_000_000))
            }
            while file.framePosition < file.length {
                guard let self, let buf = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: frames) else { return }
                do { try file.read(into: buf, frameCount: frames) } catch { break }
                if buf.frameLength == 0 { break }
                self.ingest(buf)
                await pace(buf.frameLength)
            }
            var silence = Int(format.sampleRate * 2)
            while silence > 0 {
                guard let self, let buf = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: frames) else { return }
                let n = AVAudioFrameCount(min(silence, Int(frames)))
                buf.frameLength = n
                for c in 0..<Int(format.channelCount) {
                    if let p = buf.floatChannelData?[c] { p.update(repeating: 0, count: Int(n)) }
                }
                self.ingest(buf)
                await pace(n)
                silence -= Int(n)
            }
            log.info("bellows-audio-file: injection complete")
        }
    }
}
#endif
