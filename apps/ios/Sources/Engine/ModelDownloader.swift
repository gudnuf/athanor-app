import Foundation
import CryptoKit
import Observation

// First-launch model provisioning (plan Task F1). Fetches the whisper model
// for `BellowsHandle` (C3) to `Application Support`, verifies it, and is
// resumable across app kills. This has NOTHING to do with DemoEngine —
// DemoEngine sessions never touch this class, so demo mode works fully
// offline regardless of download state (per the operator's F1 brief).
//
// AppModel exposes this so E4's real Bellows path can check `state == .ready`
// before opening `BellowsHandle`; InitiationScreen reads it for the quiet
// warming-line presence that covers the wait.
@MainActor
@Observable
final class ModelDownloader: NSObject {
    enum State: Equatable {
        case idle
        case downloading(progress: Double)
        case verifying
        case ready(path: String)
        case failed(String)
    }

    private(set) var state: State = .idle

    /// The verified model path, once ready — nil otherwise. What C3 hands to
    /// `BellowsHandle.open(model_path:...)`.
    var readyPath: String? {
        if case .ready(let path) = state { return path }
        return nil
    }

    @ObservationIgnored
    private var session: URLSession!

    // Mutated only from URLSessionDownloadDelegate callbacks, which arrive on
    // a background delegate queue outside main-actor isolation. `state` (the
    // only thing SwiftUI observes) is always written back on the main actor
    // via an explicit hop in `finish(...)`; these are just handoff scratch.
    @ObservationIgnored nonisolated(unsafe) private var pendingDestination: URL?
    @ObservationIgnored nonisolated(unsafe) private var pendingResumeURL: URL?
    @ObservationIgnored nonisolated(unsafe) private var pendingTier: ModelTier?
    @ObservationIgnored nonisolated(unsafe) private var continuation: CheckedContinuation<Void, Never>?
    @ObservationIgnored nonisolated(unsafe) private var moveError: String?

    override init() {
        super.init()
        session = URLSession(configuration: .default, delegate: self, delegateQueue: nil)
    }

    /// Idempotent — safe to call on every launch. If a verified model
    /// already sits on disk, resolves immediately with no network activity.
    func ensureModel(tier: ModelTier) async {
        guard let destination = try? destinationURL(for: tier) else {
            state = .failed("could not resolve Application Support path")
            return
        }
        if FileManager.default.fileExists(atPath: destination.path),
           verify(fileAt: destination, tier: tier) {
            state = .ready(path: destination.path)
            return
        }
        await download(tier: tier, to: destination)
    }

    // MARK: - Paths

    private func modelsDirectory() throws -> URL {
        let appSupport = try FileManager.default.url(
            for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true
        )
        let dir = appSupport.appendingPathComponent("Models", isDirectory: true)
        if !FileManager.default.fileExists(atPath: dir.path) {
            try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        }
        return dir
    }

    private func destinationURL(for tier: ModelTier) throws -> URL {
        try modelsDirectory().appendingPathComponent(tier.fileName)
    }

    private func resumeDataURL(for tier: ModelTier) throws -> URL {
        try modelsDirectory().appendingPathComponent(tier.fileName + ".resumedata")
    }

    /// QA/test hook: `model-fixture-url=<url>` swaps the tier's real ~57 MB
    /// remote URL for a tiny fixture (a `file://` URL is fine — URLSession's
    /// download task handles it like any other source) so the full
    /// download→verify→ready pipeline is exercisable without ever touching
    /// the real 57 MB asset. Never used unless the launch arg is present.
    private func resolveSourceURL(for tier: ModelTier) -> URL {
        if let arg = ProcessInfo.processInfo.arguments.first(where: { $0.hasPrefix("model-fixture-url=") }) {
            let raw = String(arg.dropFirst("model-fixture-url=".count))
            if let url = URL(string: raw) { return url }
        }
        return tier.remoteURL
    }

    /// The fixture's own expected byte count/hash aren't the tier's real
    /// ones, so verification under the QA hook checks size+sanity only
    /// (no SHA mismatch false-failure) — see `verify(fileAt:tier:)`.
    private var usingFixture: Bool {
        ProcessInfo.processInfo.arguments.contains { $0.hasPrefix("model-fixture-url=") }
    }

    // MARK: - Download

    private func download(tier: ModelTier, to destination: URL) async {
        state = .downloading(progress: 0)
        await fetchExpectedSHA256(for: tier)
        let resumeURL = try? resumeDataURL(for: tier)
        pendingDestination = destination
        pendingResumeURL = resumeURL
        pendingTier = tier
        moveError = nil

        let task: URLSessionDownloadTask
        if let resumeURL, let data = try? Data(contentsOf: resumeURL) {
            task = session.downloadTask(withResumeData: data)
        } else {
            task = session.downloadTask(with: resolveSourceURL(for: tier))
        }

        await withCheckedContinuation { (cont: CheckedContinuation<Void, Never>) in
            self.continuation = cont
            task.resume()
        }

        if let moveError {
            state = .failed(moveError)
            return
        }
        state = .verifying
        if verify(fileAt: destination, tier: tier) {
            state = .ready(path: destination.path)
            if let resumeURL { try? FileManager.default.removeItem(at: resumeURL) }
        } else {
            try? FileManager.default.removeItem(at: destination)
            state = .failed("downloaded file failed verification")
        }
    }

    // MARK: - Verification
    //
    // Content-Length is checked at download time (didFinishDownloading only
    // fires once the task believes it's complete); here, at rest, size +
    // (when known) SHA-256 is the check. The upstream HF mirror publishes a
    // git-lfs SHA-256 via the `X-Linked-ETag` header on the resolve
    // redirect — `expectedSHA256(for:)` reads that once per tier and caches
    // it; if it's ever unavailable (rate limit, host change, or the QA
    // fixture, which isn't LFS-tracked), verification falls back to a
    // size + magic-bytes sanity read rather than failing outright.
    private func verify(fileAt url: URL, tier: ModelTier) -> Bool {
        guard let attrs = try? FileManager.default.attributesOfItem(atPath: url.path),
              let size = attrs[.size] as? Int, size > 0 else { return false }

        if usingFixture {
            return sanityCheckMagicBytes(url: url)
        }

        if let expected = cachedSHA256[tier] {
            return sha256(of: url) == expected
        }
        // No SHA available (offline HEAD, or host doesn't publish one) —
        // fall back to a coarse size band + magic-bytes sanity so a
        // truncated or HTML-error download can't pass as "ready".
        let withinBand = abs(Int64(size) - tier.approxByteCount) < tier.approxByteCount / 4
        return withinBand && sanityCheckMagicBytes(url: url)
    }

    /// ggml model files begin with the magic `0x67676d6c` ("ggml" as a
    /// little-endian u32) or, for newer gguf-style exports, the ASCII bytes
    /// "GGUF" — reject anything that looks like an HTML error page or an
    /// empty/truncated file instead.
    private func sanityCheckMagicBytes(url: URL) -> Bool {
        guard let handle = try? FileHandle(forReadingFrom: url) else { return false }
        defer { try? handle.close() }
        guard let head = try? handle.read(upToCount: 4), head.count == 4 else { return false }
        let bytes = [UInt8](head)
        let asLEu32 = UInt32(bytes[0]) | UInt32(bytes[1]) << 8 | UInt32(bytes[2]) << 16 | UInt32(bytes[3]) << 24
        let isGGML = asLEu32 == 0x67676d6c
        let isGGUF = bytes == Array("GGUF".utf8)
        return isGGML || isGGUF
    }

    private func sha256(of url: URL) -> String? {
        guard let handle = try? FileHandle(forReadingFrom: url) else { return nil }
        defer { try? handle.close() }
        var hasher = SHA256()
        while let chunk = try? handle.read(upToCount: 1 << 20), !chunk.isEmpty {
            hasher.update(data: chunk)
        }
        return hasher.finalize().map { String(format: "%02x", $0) }.joined()
    }

    /// Fetched lazily via a HEAD request against the tier's real remote URL
    /// (the `X-Linked-Etag` header on Hugging Face's LFS resolve redirect is
    /// the file's git-lfs SHA-256). Best-effort — a failed/timed-out HEAD
    /// just means verification falls back to size+magic-bytes.
    @ObservationIgnored
    private var cachedSHA256: [ModelTier: String] = [:]

    private func fetchExpectedSHA256(for tier: ModelTier) async {
        guard cachedSHA256[tier] == nil, !usingFixture else { return }
        var request = URLRequest(url: tier.remoteURL)
        request.httpMethod = "HEAD"
        request.timeoutInterval = 6
        guard let (_, response) = try? await URLSession.shared.data(for: request),
              let http = response as? HTTPURLResponse,
              let etag = http.value(forHTTPHeaderField: "X-Linked-Etag")?.trimmingCharacters(in: CharacterSet(charactersIn: "\""))
        else { return }
        if etag.count == 64, etag.allSatisfy(\.isHexDigit) {
            cachedSHA256[tier] = etag
        }
    }
}

// MARK: - URLSessionDownloadDelegate

extension ModelDownloader: URLSessionDownloadDelegate {
    nonisolated func urlSession(
        _ session: URLSession, downloadTask: URLSessionDownloadTask,
        didFinishDownloadingTo location: URL
    ) {
        // `location` is a temp file valid ONLY for the duration of this
        // call — the move must happen synchronously here, not after an
        // actor hop, or the file is already gone by the time we get back.
        guard let destination = pendingDestination else { return }
        do {
            if FileManager.default.fileExists(atPath: destination.path) {
                try FileManager.default.removeItem(at: destination)
            }
            try FileManager.default.moveItem(at: location, to: destination)
        } catch {
            moveError = "could not save downloaded model: \(error.localizedDescription)"
        }
    }

    nonisolated func urlSession(
        _ session: URLSession, task: URLSessionTask, didCompleteWithError error: Error?
    ) {
        if let error, moveError == nil {
            let nsError = error as NSError
            if let resumeData = nsError.userInfo[NSURLSessionDownloadTaskResumeData] as? Data,
               let resumeURL = pendingResumeURL {
                try? resumeData.write(to: resumeURL)
            }
            moveError = error.localizedDescription
        }
        let cont = continuation
        continuation = nil
        Task { @MainActor in cont?.resume() }
    }

    nonisolated func urlSession(
        _ session: URLSession, downloadTask: URLSessionDownloadTask,
        didWriteData bytesWritten: Int64, totalBytesWritten: Int64,
        totalBytesExpectedToWrite: Int64
    ) {
        let expected = totalBytesExpectedToWrite > 0
            ? totalBytesExpectedToWrite
            : (pendingTier?.approxByteCount ?? totalBytesWritten)
        let fraction = expected > 0 ? min(Double(totalBytesWritten) / Double(expected), 1.0) : 0
        Task { @MainActor [weak self] in
            self?.state = .downloading(progress: fraction)
        }
    }
}
