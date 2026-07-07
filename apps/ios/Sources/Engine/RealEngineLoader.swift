import Foundation

// D3 real-engine seam. The ONE adapter that turns the checked-in FFI package
// (AthanorCoreFFI, produced by build-ffi.sh) into a live engine — everything
// above `AthanorEngineProtocol` (screens, AppModel) is unchanged. When the
// xcframework is absent (clean checkout / demo build), `#if canImport(...)` is
// false and this whole file degrades to `resolve()` returning `DemoEngine()`.
//
// Selection order (AthanorApp.resolveEngine calls RealEngineLoader.resolve):
//   1. xcframework linked?  no  -> DemoEngine
//   2. key available?       no  -> DemoEngine
//   3. AthanorEngine constructs? no -> DemoEngine (logged), else the real engine.
//
// KEY TRUTH (plan review #5): in a real build the key is injected by generate.sh
// into the built app's Info.plist (build setting ANTHROPIC_API_KEY). This copies
// it into the Keychain on first launch and reads it back — but the Info.plist
// copy means the KEY IS IN THE APP BUNDLE. That is acceptable for a
// single-device dogfood build only. Do NOT ship this to a multi-tester
// TestFlight; that would distribute the key. A key-free bundle needs a runtime
// paste-field flow (not built — not needed day-1). The key value is never
// logged here.

enum RealEngineLoader {
    /// Resolve the engine the app runs with. Always returns a working engine —
    /// falls back to `DemoEngine` whenever the real path is unavailable.
    @MainActor
    static func resolve() -> any AthanorEngineProtocol {
        #if canImport(AthanorCoreFFI)
        if let real = tryRealEngine() { return real }
        #endif
        return DemoEngine()
    }

    #if canImport(AthanorCoreFFI)
    @MainActor
    private static func tryRealEngine() -> (any AthanorEngineProtocol)? {
        guard let key = resolveKey(), !key.isEmpty else {
            // Real core is linked but no key present — run the demo engine until
            // one is provisioned (mirrors generate.sh's WARN path).
            return nil
        }
        do {
            let engine = try AthanorCoreEngine(dbPath: databasePath(), apiKey: key, model: nil)
            // NEVER log the key — only the fact of construction.
            NSLog("[Athanor] real athanor-core engine constructed (FFI linked, key present)")
            return engine
        } catch {
            NSLog("[Athanor] real engine construction failed (\(error)); falling back to demo")
            return nil
        }
    }

    /// Key resolution + first-launch Keychain seed. Prefers a key already in the
    /// Keychain; otherwise seeds it from the Info.plist build setting (injected
    /// by generate.sh) and stores it. Returns nil when neither has a key.
    @MainActor
    private static func resolveKey() -> String? {
        if let existing = KeychainKeyStore.load(), !existing.isEmpty { return existing }
        guard let seeded = Bundle.main.object(forInfoDictionaryKey: "ANTHROPIC_API_KEY") as? String,
              !seeded.isEmpty else { return nil }
        KeychainKeyStore.save(seeded)
        return seeded
    }

    /// On-device store path (Application Support/athanor.sqlite).
    ///
    /// Lived-in demo hook (mirrors FurnaceShell's `screen=` QA arg): a
    /// `seed-db=<path>` launch argument overrides the store path so a build can
    /// open a pre-seeded db produced by `athanor-cli seed`. This runs the REAL
    /// engine against real seeded state — streams and reads render it exactly as
    /// live use. Never affects a normal launch (no arg → the on-device path).
    private static func databasePath() -> String {
        if let arg = ProcessInfo.processInfo.arguments
            .first(where: { $0.hasPrefix("seed-db=") }) {
            let path = String(arg.dropFirst("seed-db=".count))
            if !path.isEmpty {
                NSLog("[Athanor] opening seeded db from launch arg (lived-in demo)")
                return path
            }
        }
        let fm = FileManager.default
        let dir = fm.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
        try? fm.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir.appendingPathComponent("athanor.sqlite").path
    }
    #endif
}

#if canImport(AthanorCoreFFI)
import AthanorCoreFFI

// Thin adapter: the app's `AthanorEngineProtocol` over the generated FFI types.
// FFI types share names with the app's own value types (SessionEvent,
// AthanorEngine, ...), so every FFI reference is module-qualified
// (`AthanorCoreFFI.*`). Projection + wiring only — no business logic (rmp
// invariant): the Rust core decides everything; this maps records and forwards
// calls.
//
// Scope note (D3): reads are fully wired; session streaming is wired via a
// listener→AsyncStream bridge so the FULL FFI surface links. A live turn is
// operator-gated (needs a real key + device); this file's bar is that the real
// engine constructs and links. E4/E5 own the on-device audio + render polish.
@MainActor
final class AthanorCoreEngine: AthanorEngineProtocol {
    let isReal = true

    private let engine: AthanorCoreFFI.AthanorEngine
    private var currentHandle: AthanorCoreFFI.SessionHandle?
    private var currentBridge: SessionEventBridge?

    init(dbPath: String, apiKey: String, model: String?) throws {
        self.engine = try AthanorCoreFFI.AthanorEngine(
            dbPath: dbPath,
            anthropicKey: apiKey,
            tier: AthanorCoreFFI.TierConfig(model: model)
        )
    }

    // MARK: Sessions

    func beginSession(threadId: String?) throws -> AsyncStream<SessionEvent> {
        attach(try engine.beginSession(mask: nil, mode: nil, threadId: threadId))
    }

    // BLOCKER-1 deep fix: initiation has no other first-speaker channel — the
    // Mystagogue must open the exchange itself. `open()` runs the Conductor's
    // ritual-opening turn (core-side: `Conductor::open_turn`, seeded from the
    // versioned prompt pack, never a hardcoded Swift string) and streams its
    // reply through the same listener `attach` just wired up. Fire-and-forget,
    // mirroring `sendTurn`'s own Task pattern — the screen just observes the
    // stream; it never has to tap anything to make the Mystagogue speak.
    func beginInitiation() throws -> AsyncStream<SessionEvent> {
        let handle = try engine.beginInitiation()
        let stream = attach(handle)
        Task { await handle.open() }
        return stream
    }

    private func attach(_ handle: AthanorCoreFFI.SessionHandle) -> AsyncStream<SessionEvent> {
        let (stream, continuation) = AsyncStream<SessionEvent>.makeStream()
        let bridge = SessionEventBridge(continuation)
        handle.setListener(listener: bridge)
        currentHandle = handle
        currentBridge = bridge
        return stream
    }

    func sendTurn(_ text: String) {
        guard let handle = currentHandle else { return }
        Task { await handle.sendTurn(text: text) }
    }

    func endSession(abandon: Bool) async {
        guard let handle = currentHandle else { return }
        // The app protocol carries no minutes; on-device tending-time capture is
        // E-phase. Close with 0 for the smoke path.
        do {
            if abandon { try await handle.abandon() } else { try await handle.close(minutes: 0) }
        } catch {
            NSLog("[Athanor] endSession error: \(error)")
        }
        currentHandle = nil
        currentBridge = nil
    }

    // MARK: Reads (FFI record -> app projection)

    func furnaceState() -> FireState {
        guard let f = try? engine.furnaceState() else {
            return FireState(wisdomDays: 0, lastTendedDay: nil, tendedToday: false, recent: [])
        }
        return FireState(
            wisdomDays: Int(f.wisdomDays),
            lastTendedDay: f.lastTendedDay.flatMap(Self.parseDay),
            tendedToday: f.tendedToday,
            // FFI projects tended DAYS, not trace lines; surface the day strings
            // until core projects recent trace text (coordination note for E2).
            recent: f.recent.map(\.day)
        )
    }

    func grimoire() -> [Realization] {
        ((try? engine.grimoire()) ?? []).map { g in
            Realization(
                id: g.id,
                text: g.text,
                domains: g.domains,
                date: Date(timeIntervalSince1970: Double(g.date)),
                threadId: g.threadId,
                childThreadId: g.childThreadId
            )
        }
    }

    func mercury() -> [Thread] {
        ((try? engine.mercury()) ?? []).map { t in
            Thread(
                id: t.id,
                prompt: t.prompt,
                domain: t.domainId ?? "",
                state: ThreadState(rawValue: t.state) ?? .volatile,
                born: Date(timeIntervalSince1970: Double(t.born)),
                lastWorked: t.lastWorked.map { Date(timeIntervalSince1970: Double($0)) }
            )
        }
    }

    func tabula() -> [TabulaPassage] {
        // FFI `tabula()` projects kindled passage bodies as [String]; the rich
        // number/title/kindledNote shape is E5 surface. Map minimally.
        let romans = ["I", "II", "III", "IV", "V", "VI", "VII"]
        return ((try? engine.tabula()) ?? []).enumerated().map { i, body in
            TabulaPassage(
                id: "passage-\(i)",
                number: i < romans.count ? romans[i] : "\(i + 1)",
                title: "",
                body: body,
                kindled: true,
                kindledNote: nil
            )
        }
    }

    // MARK: Helpers

    private static let dayFormatter: DateFormatter = {
        let f = DateFormatter()
        f.dateFormat = "yyyy-MM-dd"
        f.timeZone = TimeZone(identifier: "UTC")
        f.locale = Locale(identifier: "en_US_POSIX")
        return f
    }()

    private static func parseDay(_ s: String) -> Date? { dayFormatter.date(from: s) }
}

/// Foreign listener: FFI `SessionEvent` -> app `SessionEvent`, yielded into the
/// session's `AsyncStream`. May be invoked off the main thread (tokio) — only
/// touches the thread-safe continuation, hence `@unchecked Sendable`.
private final class SessionEventBridge: AthanorCoreFFI.SessionEventListener, @unchecked Sendable {
    private let continuation: AsyncStream<SessionEvent>.Continuation

    init(_ continuation: AsyncStream<SessionEvent>.Continuation) {
        self.continuation = continuation
    }

    func onEvent(event: AthanorCoreFFI.SessionEvent) {
        continuation.yield(Self.map(event))
    }

    private static func map(_ e: AthanorCoreFFI.SessionEvent) -> SessionEvent {
        switch e {
        case let .textDelta(text, register):
            return .textDelta(text, register: register == "serif" ? .serif : .quick)
        case let .toolCall(kind):
            return .toolCall(kind: kind)
        case let .condensation(realizationId, childThreadId):
            return .condensation(realizationId: realizationId, childThreadId: childThreadId ?? "")
        case .turnComplete:
            return .turnComplete
        case let .error(message):
            return .error(message: message)
        }
    }
}
#endif
