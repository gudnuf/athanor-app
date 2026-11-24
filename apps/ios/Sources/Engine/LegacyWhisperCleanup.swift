import Foundation
import os

// v1.0.7 downloaded whisper model files (ggml-*.bin, 60–240 MB, plus
// `.resumedata` stubs) into Application Support/Models. v1.0.8 removed whisper,
// so on upgraded phones those files are dead weight. Glue only: once per launch,
// off the main thread, delete that directory if it exists and holds nothing but
// those files. If anything else is in it, delete only the whisper files and
// leave the rest alone. Logs the byte count only.
enum LegacyWhisperCleanup {
    private static let log = Logger(subsystem: "com.damsac.athanor", category: "bellows")

    static func runInBackground() {
        Task.detached(priority: .utility) { run() }
    }

    nonisolated static func run() {
        let fm = FileManager.default
        guard let appSupport = try? fm.url(
            for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: false
        ) else { return }
        let dir = appSupport.appendingPathComponent("Models", isDirectory: true)
        guard let entries = try? fm.contentsOfDirectory(at: dir, includingPropertiesForKeys: [.fileSizeKey]) else {
            return   // absent: nothing to do
        }
        func isWhisperFile(_ url: URL) -> Bool {
            let name = url.lastPathComponent
            return name.hasPrefix("ggml-") && (name.hasSuffix(".bin") || name.hasSuffix(".bin.resumedata"))
        }
        var bytes: Int64 = 0
        var removed = 0
        for url in entries where isWhisperFile(url) {
            let size = (try? url.resourceValues(forKeys: [.fileSizeKey]).fileSize).flatMap { $0 } ?? 0
            if (try? fm.removeItem(at: url)) != nil {
                bytes += Int64(size)
                removed += 1
            }
        }
        // Drop the directory itself only if it is now empty.
        if (try? fm.contentsOfDirectory(atPath: dir.path))?.isEmpty == true {
            try? fm.removeItem(at: dir)
        }
        if removed > 0 {
            log.info("removed legacy whisper models files=\(removed) bytes=\(bytes)")
        }
    }
}
