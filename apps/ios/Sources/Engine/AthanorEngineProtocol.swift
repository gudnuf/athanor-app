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
    /// The recent tended-days window (day + minutes), most-recent first — the
    /// honest projection of core `Tending`. The Furnace grounds its recency
    /// copy in the first (latest) entry.
    var recent: [TendedDay]
}

/// One tended day: a UTC day and the minutes spent that day.
struct TendedDay: Equatable {
    var day: Date
    var minutes: Int
}

/// The home screen's per-door heat (lane 14), 0..1 each — computed in core from
/// real store facts (`home_heat`), never invented in the UI. `subscript` reads
/// a door's heat by its glyph key so the dial can iterate the eight orbiters.
struct HomeHeatValues: Equatable {
    var furnace: Double = 0.30
    var bellows: Double = 0.30
    var mercury: Double = 0.30
    var grimoire: Double = 0.30
    var tabula: Double = 0.30
    var adamas: Double = 0
    var philosophus: Double = 0
    var solve: Double = 0
    var azoth: Double = 0

    subscript(_ key: GlyphKey) -> Double {
        switch key {
        case .furnace: return furnace
        case .bellows: return bellows
        case .mercury: return mercury
        case .grimoire: return grimoire
        case .tabula: return tabula
        case .adamas: return adamas
        case .philosophus: return philosophus
        case .solve: return solve
        case .azoth: return azoth
        }
    }
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
    /// The realization this thread spiralled from, if it was born of a
    /// `fix_salt` (the spiral back-link). Lets a surface show what a thread
    /// opened from when its own prompt is still the bare placeholder.
    var parentRealizationId: String? = nil

    /// The core's default child-question (`DEFAULT_CHILD_QUESTION` in
    /// athanor-core). A spiral child still carrying this hasn't been given a
    /// real next-question yet, so surfaces show what it spiralled FROM instead
    /// of a wall of identical placeholders. Kept in sync with core by hand —
    /// it's a display heuristic, not a contract.
    static let defaultChildQuestion = "what does this open?"

    /// True when this is a spiral child still on the placeholder prompt.
    var isPlaceholderSpiral: Bool {
        prompt == Thread.defaultChildQuestion && parentRealizationId != nil
    }
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
    /// The session's current `(mask, mode)` register (lane 13) — surfaced at the
    /// top of a turn when it changes (the opening pair, or the new pair after a
    /// quiet mid-session shift or a learner pin). Drives the honest header.
    case maskShifted(mask: String, mode: String)
    case condensation(realizationId: String, childThreadId: String, text: String)
    case turnComplete
    case error(message: String)
}

// MARK: - Engine protocol

@MainActor
protocol AthanorEngineProtocol: AnyObject {
    /// True for `AthanorCoreEngine` (real athanor-core, post-D2/D3), false for
    /// `DemoEngine`. SessionScreen reads this — not a type check — to decide
    /// whether the real Bellows (mic capture + BellowsHandle) is even worth
    /// attempting; DemoEngine's sine-stub bed never depends on it.
    var isReal: Bool { get }

    /// Begin a session against a chosen thread, optionally opening under a
    /// chosen mask (lane 14: tapping a mask glyph pre-chooses its voice — the
    /// session OPENS under it but it is NOT pinned, so the Mystagogue may still
    /// shift as fitting). `nil` mask lets the opening default stand. Returns
    /// THIS session's event stream; a fresh call hands out a fresh stream.
    func beginSession(threadId: String?, mask: String?) throws -> AsyncStream<SessionEvent>

    /// First-launch: the Mystagogue's first session, about the learner.
    func beginInitiation() throws -> AsyncStream<SessionEvent>

    /// Send one learner turn (voice-finalized text or typed fallback).
    func sendTurn(_ text: String)

    /// The session's opening register — for the header's first, truthful paint
    /// before any `maskShifted` event arrives. `("philosophus", "explain")` by
    /// default (what a nil-mask session opens under).
    func currentMask() -> String
    func currentMode() -> String

    /// The escape hatch (lane 13): the learner pins a mask for the rest of the
    /// session. The Mystagogue's `shift_mask` then no-ops until the session ends.
    func pinMask(_ mask: String)

    /// Close the session cleanly (persists transcript, closes/abandons thread).
    func endSession(abandon: Bool) async

    // Read surface (B2/C1 projections)
    func furnaceState() -> FireState
    /// The home screen's per-door heat (lane 14), computed in core.
    func homeHeat() -> HomeHeatValues
    func grimoire() -> [Realization]
    func mercury() -> [Thread]
    func tabula() -> [TabulaPassage]
}
