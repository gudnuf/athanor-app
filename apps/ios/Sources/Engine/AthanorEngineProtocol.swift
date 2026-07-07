import Foundation

// The seam between the SwiftUI shell and the athanor-core engine.
//
// Mirrors the shape crates/ffi will export (see plan Phase C: `AthanorEngine`
// uniffi::Object with furnace_state()/grimoire()/mercury() read projections,
// plus `SessionHandle` streaming SessionEvents). This protocol is FFI-FREE —
// no import of any generated bridge module — so E1 through E5 build and run
// entirely against `DemoEngine` (pure Swift, canned data) while D2/D3 land the
// real xcframework in parallel. When the real engine lands, a thin
// `AthanorCoreEngine: AthanorEngineProtocol` wraps the generated FFI types;
// nothing above this protocol changes.

// MARK: - Value types (Swift-side projections; mirror B2/C1 read shapes)

struct FireState: Equatable {
    var wisdomDays: Int
    var lastTendedDay: Date?
    var tendedToday: Bool
    var recent: [String] // recent trace lines, most-recent first
}

struct Realization: Identifiable, Equatable {
    var id: String
    var text: String
    var domains: [String]
    var date: Date
    var threadId: String
    var childThreadId: String?
}

enum ThreadState: String, Equatable {
    case volatile, condensing, fixed, evaporated
}

struct Thread: Identifiable, Equatable {
    var id: String
    var prompt: String
    var domain: String
    var state: ThreadState
    var born: Date
    var lastWorked: Date?
    /// Mirrors the plan's B2 `ripe_threads` read — "one thread of mercury
    /// judged ripe" for the next session (spec: "The Mystagogue conducts
    /// each session"). Not derived from `state`; the core decides ripeness
    /// independently of the volatile/condensing/fixed/evaporated lifecycle.
    var isRipe: Bool = false
}

struct TabulaPassage: Identifiable, Equatable {
    var id: String
    var number: String   // "I" ... "VII"
    var title: String
    var body: String
    var kindled: Bool
    var kindledNote: String?
}

/// Reply register hint carried on session events — quick conversational sans
/// vs. the full-width serif reading voice (spec: "Reply register").
enum ReplyRegister: Equatable {
    case quick
    case serif
}

/// Streamed session events. Mirrors the plan's C2 `SessionEvent` enum
/// (`TextDelta`, `ToolCall`, `Condensation`, `TurnComplete`, `Error`) so the
/// bridge, when it lands, is a thin adapter rather than a redesign.
enum SessionEvent: Equatable {
    case textDelta(String, register: ReplyRegister)
    case toolCall(kind: String)
    case condensation(realizationId: String, childThreadId: String)
    case turnComplete
    case error(message: String)
}

// MARK: - Engine protocol

@MainActor
protocol AthanorEngineProtocol: AnyObject {
    /// Begin a session against a chosen thread (mask/mode may be nil to let
    /// the Mystagogue choose). Returns THIS session's event stream; a fresh
    /// `beginSession`/`beginInitiation` call hands out a fresh stream.
    func beginSession(threadId: String?) throws -> AsyncStream<SessionEvent>

    /// First-launch: the Mystagogue's first session, about the learner.
    func beginInitiation() throws -> AsyncStream<SessionEvent>

    /// Send one learner turn (voice-finalized text or typed fallback).
    func sendTurn(_ text: String)

    /// Close the session cleanly (persists transcript, closes/abandons thread).
    func endSession(abandon: Bool) async

    // Read surface (B2/C1 projections)
    func furnaceState() -> FireState
    func grimoire() -> [Realization]
    func mercury() -> [Thread]
    func tabula() -> [TabulaPassage]
}
