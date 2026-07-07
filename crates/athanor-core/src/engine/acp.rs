//! ACP-shaped seam types.
//!
//! These are OUR structs, not the `agent-client-protocol` crate's. They
//! mirror ACP wire shapes closely enough that `engine/goose.rs` can convert
//! to/from the real goose/ACP types at the boundary, but no other module in
//! this codebase should ever name a goose or `agent-client-protocol` type
//! directly — that isolation is the entire point of this seam. When the
//! engine underneath churns (goose upgrade, or the goosed-on-tailnet RED
//! path), only `engine/goose.rs` (or its replacement) needs to change.

use serde_json::Value;

/// Which voice a run of the Mystagogue's reply is spoken in (identity.md §6).
/// The default is [`Register::Quick`] — the conversational sans voice. A run is
/// [`Register::Reading`] only inside a passage the model marked as a deeper
/// lesson (see `crate::register`). Engines emit raw text as `Quick`; the
/// register is *assigned* one layer up, by the `Conductor`'s marker parser.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Register {
    /// The conversational default: quick, plain, spoken lines.
    #[default]
    Quick,
    /// The reading voice: a measured lesson, rendered with more weight and air.
    Reading,
}

/// Who spoke a given [`AcpTurn`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcpRole {
    /// The human on the other end of the session.
    Learner,
    /// The engine's own prior reply, fed back so a multi-turn session can
    /// see what it already said (SHOULD-FIX-4: without this, the model
    /// re-derives each turn from the learner's turns + store state alone,
    /// and can repeat or contradict its own earlier framing).
    Mystagogue,
}

/// One turn of dialogue, tagged with who said it.
#[derive(Debug, Clone, PartialEq)]
pub struct AcpTurn {
    pub role: AcpRole,
    pub text: String,
}

/// One turn's worth of input to the engine: system prompt, the full dialogue
/// history so far (both sides — see [`AcpRole`]), and the tools available for
/// this turn.
#[derive(Debug, Clone, PartialEq)]
pub struct AcpPrompt {
    pub system: String,
    pub turns: Vec<AcpTurn>,
    pub tools: Vec<AcpToolSpec>,
}

/// A tool made available to the engine for this turn.
#[derive(Debug, Clone, PartialEq)]
pub struct AcpToolSpec {
    pub name: String,
    pub json_schema: Value,
}

/// A single streamed update from the engine while it drives a turn.
#[derive(Debug, Clone, PartialEq)]
pub enum AcpUpdate {
    /// A chunk of the model's reply text, tagged with the register it is spoken
    /// in. Engines produce raw text as [`Register::Quick`] (they don't parse
    /// markers) via [`AcpUpdate::text_delta`]; the `Conductor`'s register parser
    /// re-tags reading passages before the delta reaches the caller's sink.
    TextDelta {
        text: String,
        register: Register,
    },
    ToolCall(AcpToolCall),
    /// The dispatched result of a `ToolCall`, streamed right after the engine
    /// resolves it (same `id` as the call). Carries the tool's return value —
    /// e.g. `fix_salt`'s `{realization_id, child_thread_id}` — so the bridge can
    /// synthesize the Condensation moment from the REAL fixed salt rather than
    /// guessing the newest grain out of the store.
    ToolResult(AcpToolResult),
    TurnComplete,
}

impl AcpUpdate {
    /// Builds a raw [`AcpUpdate::TextDelta`] in the default [`Register::Quick`].
    /// Engines and test scripts use this — register is assigned downstream by
    /// the `Conductor`'s marker parser, never at the point text is produced.
    pub fn text_delta(text: impl Into<String>) -> Self {
        AcpUpdate::TextDelta {
            text: text.into(),
            register: Register::Quick,
        }
    }
}

/// A tool invocation requested by the engine, to be resolved via
/// `ToolDispatch`.
#[derive(Debug, Clone, PartialEq)]
pub struct AcpToolCall {
    pub id: String,
    pub name: String,
    pub args: Value,
}

/// The result of dispatching a tool call, handed back to the engine.
#[derive(Debug, Clone, PartialEq)]
pub struct AcpToolResult {
    pub id: String,
    pub value: Value,
}
