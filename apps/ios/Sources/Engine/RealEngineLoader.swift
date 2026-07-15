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
        // QA/screenshot hook only (same launch-arg family as `screen=` /
        // `autoplay=`): force the canned DemoEngine even when a real key is
        // present, so a deterministic scripted beat (e.g. the reading-register
        // lesson) can be captured without depending on a live model reply.
        if ProcessInfo.processInfo.arguments.contains("force-demo=1") {
            return DemoEngine()
        }
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
            // Pin the Mystagogue to Fable 5; nil would fall back to goose's
            // Anthropic default (claude-sonnet-4-5).
            let engine = try AthanorCoreEngine(dbPath: databasePath(), apiKey: key, model: "claude-fable-5")
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
        let live = dir.appendingPathComponent("athanor.sqlite")

        #if DEBUG
        seedFromDevFixtureIfNeeded(liveDB: live, fileManager: fm)
        #endif

        return live.path
    }

    #if DEBUG
    /// Dev-only lived-in default: on a FRESH real build (no live db yet), copy
    /// the operator's local seed fixture into place so every screen opens
    /// inhabited with real academy material — no `seed-db=` launch arg to
    /// remember. Strictly guarded:
    ///   - `#if DEBUG` only: release builds never compile this.
    ///   - No-clobber: if a live db already exists, it is left untouched (a real
    ///     session's history is never overwritten).
    ///   - Opt-in by presence: does nothing unless generate.sh baked a fixture
    ///     path (`ATHANOR_DEV_SEED_DB`) AND that file exists on disk (so a
    ///     device/CI build with no host fixture simply falls through to empty).
    ///   - Profile select: `seed-profile=normy` at launch prefers the committed
    ///     "normy" demo persona (`ATHANOR_DEV_SEED_DB_NORMY`) over the default,
    ///     so the everyday-learner demo is one launch arg away.
    /// The seeded data lives only in the gitignored fixture; nothing is committed.
    private static func seedFromDevFixtureIfNeeded(liveDB: URL, fileManager fm: FileManager) {
        guard !fm.fileExists(atPath: liveDB.path) else { return } // never clobber
        guard let fixture = resolveDevFixture(fileManager: fm) else { return }
        do {
            try fm.copyItem(atPath: fixture, toPath: liveDB.path)
            // Bring along the sqlite sidecars if the fixture carries them (WAL
            // mode) so the copy opens consistent; absent ones are simply skipped.
            for suffix in ["-wal", "-shm"] {
                let src = fixture + suffix
                if fm.fileExists(atPath: src) {
                    try? fm.copyItem(atPath: src, toPath: liveDB.path + suffix)
                }
            }
            // A seeded install represents an ALREADY-established practice (the
            // fixture carries months of history), so skip the first-launch
            // initiation — landing straight on the inhabited Furnace instead of
            // "I don't know you yet." This runs during engine construction,
            // before AppModel reads the flag. Key mirrors AppModel.initiationKey;
            // dev-only, only on the seed path.
            UserDefaults.standard.set(true, forKey: "athanor.hasCompletedInitiation")
            NSLog("[Athanor] dev: seeded a fresh live db from the dev fixture")
        } catch {
            NSLog("[Athanor] dev: could not seed from fixture (\(error)); starting empty")
        }
    }

    /// Which baked fixture to seed from: the committed "normy" demo persona when
    /// `seed-profile=normy` is passed and its fixture exists, otherwise the
    /// default (`ATHANOR_DEV_SEED_DB` — the lived seed if present, else normy).
    /// Returns nil when the chosen fixture isn't on disk (device/CI → empty).
    private static func resolveDevFixture(fileManager fm: FileManager) -> String? {
        func baked(_ key: String) -> String? {
            guard let v = Bundle.main.object(forInfoDictionaryKey: key) as? String,
                  !v.isEmpty, fm.fileExists(atPath: v) else { return nil }
            return v
        }
        if ProcessInfo.processInfo.arguments.contains("seed-profile=normy"),
           let normy = baked("ATHANOR_DEV_SEED_DB_NORMY") {
            return normy
        }
        return baked("ATHANOR_DEV_SEED_DB")
    }
    #endif // DEBUG
    #endif // canImport(AthanorCoreFFI)
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

    func beginSession(threadId: String?, mask: String?) throws -> AsyncStream<SessionEvent> {
        // A pre-chosen mask opens the session under that voice (mode left to the
        // default) but does NOT pin it — the Mystagogue can still shift.
        attach(try engine.beginSession(mask: mask, mode: nil, threadId: threadId))
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

    // MARK: Mask register (lane 13)

    func currentMask() -> String { currentHandle?.currentMask() ?? "philosophus" }
    func currentMode() -> String { currentHandle?.currentMode() ?? "explain" }

    /// The escape hatch: pin the learner's chosen mask on the live session (core
    /// persists it and the Mystagogue's shift_mask no-ops for the rest of the
    /// session). A no-op if no session is open.
    func pinMask(_ mask: String) {
        currentHandle?.pinMask(chosen: mask)
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

    func homeHeat() -> HomeHeatValues {
        guard let h = try? engine.homeHeat() else { return HomeHeatValues() }
        return HomeHeatValues(
            furnace: Double(h.furnace),
            bellows: Double(h.bellows),
            mercury: Double(h.mercury),
            grimoire: Double(h.grimoire),
            tabula: Double(h.tabula),
            adamas: Double(h.adamas),
            philosophus: Double(h.philosophus),
            solve: Double(h.solve),
            azoth: Double(h.azoth)
        )
    }

    func furnaceState() -> FireState {
        guard let f = try? engine.furnaceState() else {
            return FireState(wisdomDays: 0, lastTendedDay: nil, tendedToday: false, recent: [])
        }
        return FireState(
            wisdomDays: Int(f.wisdomDays),
            lastTendedDay: f.lastTendedDay.flatMap(Self.parseDay),
            tendedToday: f.tendedToday,
            // The recency window, day + minutes, most-recent first (core projects
            // it as `Tending`). Malformed day strings are dropped, not faked.
            recent: f.recent.compactMap { t in
                Self.parseDay(t.day).map { TendedDay(day: $0, minutes: Int(t.minutes)) }
            }
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
                // The domain's human NAME (resolved core-side), never the raw
                // id — the Mercury row shows this as the domain tag.
                domain: t.domainName ?? "",
                state: ThreadState(rawValue: t.state) ?? .volatile,
                born: Date(timeIntervalSince1970: Double(t.born)),
                lastWorked: t.lastWorked.map { Date(timeIntervalSince1970: Double($0)) },
                parentRealizationId: t.parentRealizationId
            )
        }
    }

    func tabula() -> [TabulaPassage] {
        // FFI `tabula()` now projects the seven canonical passages
        // (number/title/body) against this learner's kindling state — a
        // straight field map, no synthesis. Dim passages carry no note.
        ((try? engine.tabula()) ?? []).map { p in
            TabulaPassage(
                id: p.key,
                number: p.number,
                title: p.title,
                body: p.body,
                kindled: p.kindled,
                kindledNote: p.kindledNote
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
            // Core's register enum -> the app's voice enum: reading passages
            // render in the serif reading voice, everything else stays quick.
            return .textDelta(text, register: register == .reading ? .serif : .quick)
        case let .toolCall(kind):
            return .toolCall(kind: kind)
        case let .maskShifted(mask, mode):
            return .maskShifted(mask: mask, mode: mode)
        case let .condensation(realizationId, childThreadId, text):
            return .condensation(realizationId: realizationId, childThreadId: childThreadId ?? "", text: text)
        case .turnComplete:
            return .turnComplete
        case let .error(message):
            return .error(message: message)
        }
    }
}
#endif
