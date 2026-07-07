import Foundation

// Pure-Swift stand-in engine (mirrors murmur-rmp's DemoWalkEngine pattern) so
// the whole shell is drivable before the FFI bridge exists. NO import of any
// FFI/bridge module anywhere in this file — that is the point: E1's demo path
// is FFI-free (plan review edit #3). Canned Mystagogue turns, streamed as
// word-level deltas with jittered pacing (not handed to the UI whole), plus a
// scripted condensation event; seeded furnace/grimoire/mercury data. Carries
// no business logic — no rules are evaluated, nothing here decides what
// condenses; it just plays back a fixed script (rmp invariant: no business
// logic in Swift, demo or real). The word-by-word streaming exists so the
// UI's token-by-token render path — and its "text never jumps" contract — is
// exercised from day one, the same shape a real model reply will arrive in.
//
// Real-engine seam: when D2 lands the AthanorCoreFFI xcframework, a new
// `AthanorCoreEngine: AthanorEngineProtocol` wraps the generated
// `uniffi::Object` types and is selected at launch when a key is present
// (see AthanorApp.swift `resolveEngine`). This file does not change.

@MainActor
final class DemoEngine: AthanorEngineProtocol {

    private var continuation: AsyncStream<SessionEvent>.Continuation?
    private var turnIndex = 0
    private var scriptedInitiation = false
    private var streamTask: Task<Void, Never>?

    // MARK: Seeded read data

    private let seededFire = FireState(
        wisdomDays: 7,
        lastTendedDay: Calendar.current.date(byAdding: .day, value: -1, to: Date()),
        tendedToday: false,
        recent: [
            "you named the pull you couldn't put down",
            "two domains rhymed, unprompted",
            "you said \"I do not know\" and meant it",
        ]
    )

    private let seededGrimoire: [Realization] = [
        // The session script's own condensation (DemoTurn.condensationBefore
        // yields realizationId "r-demo") resolves here — SessionScreen looks
        // this up by id when the event fires so it can show the learner's
        // actual words, not just the ids the event itself carries (matching
        // the real bridge's Condensation{realization_id, child_thread_id}
        // shape — the text always comes from a read, never off the event).
        Realization(
            id: "r-demo",
            text: "The frame isn't wrong until it breaks — and it only breaks against a case I didn't think to check.",
            domains: ["rhetoric"],
            date: Date(),
            threadId: "t-3",
            childThreadId: "t-demo-child"
        ),
        Realization(
            id: "r-3",
            text: "A frame breaks when the exception it's protecting against finally happens to you.",
            domains: ["rhetoric"],
            date: Calendar.current.date(byAdding: .day, value: -1, to: Date())!,
            threadId: "t-3",
            childThreadId: "t-4"
        ),
        Realization(
            id: "r-2",
            text: "Correspondence isn't proof — it's a hypothesis wearing a coincidence's clothes.",
            domains: ["magnetism", "yoga"],
            date: Calendar.current.date(byAdding: .day, value: -4, to: Date())!,
            threadId: "t-2",
            childThreadId: "t-5"
        ),
        Realization(
            id: "r-1",
            text: "The fire is low is not the same claim as the fire is out.",
            domains: ["content-production"],
            date: Calendar.current.date(byAdding: .day, value: -7, to: Date())!,
            threadId: "t-1",
            childThreadId: "t-6"
        ),
    ]

    private let seededMercury: [Thread] = [
        Thread(id: "t-demo-child", prompt: "Once the frame's gone, which parts of the old view still hold?", domain: "rhetoric", state: .volatile,
               born: Date(), lastWorked: nil),
        Thread(id: "t-4", prompt: "What replaces the frame once it's broken?", domain: "rhetoric", state: .volatile,
               born: Calendar.current.date(byAdding: .day, value: -1, to: Date())!, lastWorked: nil),
        Thread(id: "t-5", prompt: "Is the correspondence load-bearing or decorative?", domain: "magnetism", state: .condensing,
               born: Calendar.current.date(byAdding: .day, value: -4, to: Date())!,
               lastWorked: Calendar.current.date(byAdding: .day, value: -2, to: Date()), isRipe: true),
        Thread(id: "t-7", prompt: "Where does the vedic-cosmology thread actually want to go?", domain: "cosmology", state: .volatile,
               born: Calendar.current.date(byAdding: .day, value: -10, to: Date())!, lastWorked: nil),
        // r-1's child (Grimoire's spiral link) — kept in the open list so
        // Grimoire's "↳ opened" line always resolves to a real thread.
        Thread(id: "t-6", prompt: "Is the fire truly out, or just banked?", domain: "content-production", state: .volatile,
               born: Calendar.current.date(byAdding: .day, value: -7, to: Date())!, lastWorked: nil),
    ]

    private let seededTabula: [TabulaPassage] = [
        TabulaPassage(id: "I", number: "I", title: "The Furnace", body: "The fire you carry, not the fire you're given.",
                      kindled: true, kindledNote: "kindled · the fire is lit"),
        TabulaPassage(id: "II", number: "II", title: "The Three Principles", body: "Sulfur, mercury, salt — the pull, the volatile, the fixed.",
                      kindled: true, kindledNote: "kindled · you began with only yourself to burn"),
        TabulaPassage(id: "III", number: "III", title: "The Four Gates", body: "Trace, explain, predict, challenge, design.",
                      kindled: false, kindledNote: nil),
        TabulaPassage(id: "IV", number: "IV", title: "The Ministers", body: "Adamas, Philosophus, Solve, Azoth — one mind, many registers.",
                      kindled: false, kindledNote: nil),
        TabulaPassage(id: "V", number: "V", title: "The Grimoire", body: "The salt shelf. A spiral staircase, not a trophy case.",
                      kindled: true, kindledNote: "kindled · first salt fixed"),
        TabulaPassage(id: "VI", number: "VI", title: "Sources", body: "A truth spoken without source is Mercury unbound.",
                      kindled: false, kindledNote: nil),
        TabulaPassage(id: "VII", number: "VII", title: "The World", body: "The Work never closes; it is only put down cleanly.",
                      kindled: false, kindledNote: nil),
    ]

    // MARK: Canned session script
    //
    // One `DemoTurn` per learner turn. The reply text is streamed as
    // word-level `.textDelta` chunks with jittered pacing (below) — never
    // handed to the UI as a whole string — so the streaming-render path (and
    // its "never jumps" contract) is exercised from day one, the same way a
    // real token-by-token model reply would arrive. `condensationBefore`, if
    // set, is emitted once before the reply starts streaming (matches the
    // spec: the Mystagogue offers condensation, THEN speaks the coda line).
    private struct DemoTurn {
        let reply: String
        let register: ReplyRegister
        var condensationBefore: (realizationId: String, childThreadId: String)? = nil
    }

    private static let turns: [DemoTurn] = [
        DemoTurn(reply: "What's the thread you keep circling back to?", register: .quick),
        DemoTurn(
            reply: "Say more about that — when you noticed the frame break, what replaced it?",
            register: .serif
        ),
        DemoTurn(
            reply: "Salt fixed. That's dated, and it's yours now.",
            register: .quick,
            condensationBefore: (realizationId: "r-demo", childThreadId: "t-demo-child")
        ),
    ]

    private static let initiationTurns: [DemoTurn] = [
        DemoTurn(reply: "I don't know you yet. What's the thing you can't put down?", register: .serif),
        DemoTurn(reply: "Good. We'll find out together whether that's sulfur or just noise.", register: .quick),
    ]

    // MARK: AthanorEngineProtocol

    func beginSession(threadId: String?) throws -> AsyncStream<SessionEvent> {
        turnIndex = 0
        scriptedInitiation = false
        return makeStream()
    }

    func beginInitiation() throws -> AsyncStream<SessionEvent> {
        turnIndex = 0
        scriptedInitiation = true
        return makeStream()
    }

    func sendTurn(_ text: String) {
        let script = scriptedInitiation ? Self.initiationTurns : Self.turns
        guard turnIndex < script.count else {
            continuation?.yield(.turnComplete)
            return
        }
        // Play back one canned beat per learner turn (a fixed script, not a
        // rule engine — no decisions are made here); the beat itself streams
        // token-by-token below.
        let turn = script[turnIndex]
        turnIndex += 1
        streamTask?.cancel()
        streamTask = Task { [weak self] in
            guard let self else { return }
            if let c = turn.condensationBefore {
                self.continuation?.yield(.condensation(realizationId: c.realizationId, childThreadId: c.childThreadId))
                try? await Task.sleep(nanoseconds: 350_000_000)
            }
            await self.stream(turn.reply, register: turn.register)
            guard !Task.isCancelled else { return }
            self.continuation?.yield(.turnComplete)
        }
    }

    /// Streams `text` as word-level deltas with jittered pacing — a stand-in
    /// for real per-token arrival, not a whole-string handoff. Each chunk
    /// carries its own leading space so the UI can append-concatenate
    /// without re-deriving word boundaries.
    private func stream(_ text: String, register: ReplyRegister) async {
        let words = text.split(separator: " ", omittingEmptySubsequences: false)
        for (index, word) in words.enumerated() {
            guard !Task.isCancelled else { return }
            let chunk = index == 0 ? String(word) : " " + word
            continuation?.yield(.textDelta(chunk, register: register))
            try? await Task.sleep(nanoseconds: UInt64.random(in: 45_000_000...95_000_000))
        }
    }

    func endSession(abandon: Bool) async {
        streamTask?.cancel()
        streamTask = nil
        continuation?.finish()
        continuation = nil
    }

    func furnaceState() -> FireState { seededFire }
    func grimoire() -> [Realization] { seededGrimoire }
    func mercury() -> [Thread] { seededMercury }
    func tabula() -> [TabulaPassage] { seededTabula }

    private func makeStream() -> AsyncStream<SessionEvent> {
        streamTask?.cancel()
        streamTask = nil
        continuation?.finish()
        var cont: AsyncStream<SessionEvent>.Continuation!
        let stream = AsyncStream<SessionEvent> { cont = $0 }
        continuation = cont
        return stream
    }
}
