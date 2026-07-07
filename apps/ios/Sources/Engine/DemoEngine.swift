import Foundation

// Pure-Swift stand-in engine (mirrors murmur-rmp's DemoWalkEngine pattern) so
// the whole shell is drivable before the FFI bridge exists. NO import of any
// FFI/bridge module anywhere in this file — that is the point: E1's demo path
// is FFI-free (plan review edit #3). Canned Mystagogue lines + a scripted
// condensation event; seeded furnace/grimoire/mercury data. Carries no
// business logic — no rules are evaluated, nothing here decides what
// condenses; it just plays back a fixed script (rmp invariant: no business
// logic in Swift, demo or real).
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
        Thread(id: "t-4", prompt: "What replaces the frame once it's broken?", domain: "rhetoric", state: .volatile,
               born: Calendar.current.date(byAdding: .day, value: -1, to: Date())!, lastWorked: nil),
        Thread(id: "t-5", prompt: "Is the correspondence load-bearing or decorative?", domain: "magnetism", state: .condensing,
               born: Calendar.current.date(byAdding: .day, value: -4, to: Date())!,
               lastWorked: Calendar.current.date(byAdding: .day, value: -2, to: Date())),
        Thread(id: "t-7", prompt: "Where does the vedic-cosmology thread actually want to go?", domain: "cosmology", state: .volatile,
               born: Calendar.current.date(byAdding: .day, value: -10, to: Date())!, lastWorked: nil),
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
    // A short scripted exchange ending in a condensation event, so E4's
    // Bellows/session UI and E5's Grimoire have something real to render
    // against without any live model call.
    private static let script: [SessionEvent] = [
        .textDelta("What's the thread you keep circling back to?", register: .quick),
        .textDelta(
            "Say more about that — when you noticed the frame break, what replaced it?",
            register: .serif
        ),
        .condensation(realizationId: "r-demo", childThreadId: "t-demo-child"),
        .textDelta("Salt fixed. That's dated, and it's yours now.", register: .quick),
        .turnComplete,
    ]

    private static let initiationScript: [SessionEvent] = [
        .textDelta("I don't know you yet. What's the thing you can't put down?", register: .serif),
        .textDelta("Good. We'll find out together whether that's sulfur or just noise.", register: .quick),
        .turnComplete,
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
        let script = scriptedInitiation ? Self.initiationScript : Self.script
        guard turnIndex < script.count else {
            continuation?.yield(.turnComplete)
            return
        }
        // Play back one canned beat per learner turn (a fixed script, not a
        // rule engine — no decisions are made here).
        continuation?.yield(script[turnIndex])
        turnIndex += 1
    }

    func endSession(abandon: Bool) async {
        continuation?.finish()
        continuation = nil
    }

    func furnaceState() -> FireState { seededFire }
    func grimoire() -> [Realization] { seededGrimoire }
    func mercury() -> [Thread] { seededMercury }
    func tabula() -> [TabulaPassage] { seededTabula }

    private func makeStream() -> AsyncStream<SessionEvent> {
        continuation?.finish()
        var cont: AsyncStream<SessionEvent>.Continuation!
        let stream = AsyncStream<SessionEvent> { cont = $0 }
        continuation = cont
        return stream
    }
}
