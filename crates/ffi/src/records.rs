//! FFI read projections (Plan Phase 4, Task C1). Thin `uniffi::Record`
//! dictionaries over the B2 store reads (`fire_state`, `list_realizations`,
//! `open_threads`, `kindled`). Projection-only — no business logic, no core
//! type crosses the boundary, no `serde_json::Value`.

use athanor_core::domain::{FireState, GrimoireEntry, Tending, Thread};
use athanor_core::heat::HomeHeat as CoreHomeHeat;
use athanor_core::tabula::TabulaPassage as CoreTabulaPassage;

/// The home screen's per-door heat (lane 14) — a projection of core `HomeHeat`.
/// Each field is a 0..1 temperature the glyph renderer draws; heat is COMPUTED
/// in core from real store facts, so the UI never invents a number.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct HomeHeat {
    pub furnace: f32,
    pub bellows: f32,
    pub mercury: f32,
    pub grimoire: f32,
    pub tabula: f32,
    pub adamas: f32,
    pub philosophus: f32,
    pub solve: f32,
    pub azoth: f32,
}

impl From<CoreHomeHeat> for HomeHeat {
    fn from(h: CoreHomeHeat) -> Self {
        HomeHeat {
            furnace: h.furnace,
            bellows: h.bellows,
            mercury: h.mercury,
            grimoire: h.grimoire,
            tabula: h.tabula,
            adamas: h.adamas,
            philosophus: h.philosophus,
            solve: h.solve,
            azoth: h.azoth,
        }
    }
}

/// Furnace heat for the home screen — a projection of core `FireState`.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct FurnaceState {
    /// Distinct days tended — "did I show up," not total minutes.
    pub wisdom_days: u64,
    /// The most recent tended UTC day (`YYYY-MM-DD`), if any.
    pub last_tended_day: Option<String>,
    /// Whether the fire was fed today (honors the store's clock).
    pub tended_today: bool,
    /// A small most-recent-first window for the Furnace's recency copy.
    pub recent: Vec<TendingDay>,
}

/// One tended day — a projection of core `Tending`.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct TendingDay {
    pub day: String,
    pub minutes: u32,
    pub thread_ids: Vec<String>,
}

/// One grain of salt on the Grimoire shelf — a projection of core
/// `GrimoireEntry` (realization + linked domain names). The realization's
/// `child_thread_id` is the spiral link.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct GrimoireGrain {
    pub id: String,
    pub text: String,
    pub date: u64,
    pub thread_id: String,
    pub child_thread_id: Option<String>,
    pub domains: Vec<String>,
}

/// One open thread on Mercury — a projection of core `Thread`.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct OpenThread {
    pub id: String,
    pub prompt: String,
    pub domain_id: Option<String>,
    /// The domain's human NAME (resolved from `domain_id` in `mercury()`), for
    /// display. `None` when the thread has no domain. The UI shows this, never
    /// the raw id.
    pub domain_name: Option<String>,
    /// The thread lifecycle state, lower-cased (`volatile`/`condensing`/…).
    pub state: String,
    pub born: u64,
    pub last_worked: Option<u64>,
    /// The spiral back-link: set when this thread was born of a realization.
    pub parent_realization_id: Option<String>,
}

/// One passage of the Tabula scroll — a projection of core `TabulaPassage`
/// (canonical content + this learner's kindling state). Always seven, in scroll
/// order; `kindled_note` is set only when the passage has been kindled.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct TabulaPassage {
    /// Stable passage key (identity for the UI list; never shown).
    pub key: String,
    /// Roman numeral shown in the scroll ("I"…"VII").
    pub number: String,
    pub title: String,
    pub body: String,
    pub kindled: bool,
    pub kindled_note: Option<String>,
}

/// One past session in a history list (a thread's fires, or the "past fires"
/// surface) — enough to show a row and push into the reading view. `excerpt` is
/// the session's condensation NOTE if it has one, else its one-line trace,
/// whitespace-collapsed; empty when it left neither.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct SessionSummary {
    pub id: String,
    pub thread_id: Option<String>,
    /// The session's created_at, epoch seconds (the UI formats the date).
    pub date: u64,
    pub mask: String,
    pub mode: String,
    pub excerpt: String,
}

/// One role-tagged turn of a session's full transcript (the reading view).
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct TranscriptTurn {
    /// `"learner"` or `"mystagogue"` (from `athanor_core::transcript`).
    pub role: String,
    pub text: String,
}

/// A single session's full detail: its role-tagged transcript (both sides) plus
/// the condensation note, for the transcript reading view.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct SessionDetail {
    pub id: String,
    pub thread_id: Option<String>,
    pub date: u64,
    pub mask: String,
    pub mode: String,
    /// The condensation residue, if the session distilled one.
    pub note: Option<String>,
    /// The full dialogue, oldest-first, both roles.
    pub turns: Vec<TranscriptTurn>,
}

impl From<CoreTabulaPassage> for TabulaPassage {
    fn from(p: CoreTabulaPassage) -> Self {
        TabulaPassage {
            key: p.key,
            number: p.number,
            title: p.title,
            body: p.body,
            kindled: p.kindled,
            kindled_note: p.kindled_note,
        }
    }
}

impl From<&Tending> for TendingDay {
    fn from(t: &Tending) -> Self {
        TendingDay {
            day: t.day.clone(),
            minutes: t.minutes,
            thread_ids: t.thread_ids.clone(),
        }
    }
}

impl From<FireState> for FurnaceState {
    fn from(f: FireState) -> Self {
        FurnaceState {
            wisdom_days: f.wisdom_days,
            last_tended_day: f.last_tended_day,
            tended_today: f.tended_today,
            recent: f.recent.iter().map(TendingDay::from).collect(),
        }
    }
}

impl From<GrimoireEntry> for GrimoireGrain {
    fn from(e: GrimoireEntry) -> Self {
        GrimoireGrain {
            id: e.realization.id,
            text: e.realization.text,
            date: e.realization.date,
            thread_id: e.realization.thread_id,
            child_thread_id: e.realization.child_thread_id,
            domains: e.domains,
        }
    }
}

impl From<Thread> for OpenThread {
    fn from(t: Thread) -> Self {
        OpenThread {
            id: t.id,
            prompt: t.prompt,
            domain_id: t.domain_id,
            // Resolved by `mercury()` (needs the domain table); the raw
            // `From` leaves it None.
            domain_name: None,
            state: t.state.as_str().to_string(),
            born: t.born,
            last_worked: t.last_worked,
            parent_realization_id: t.parent_realization_id,
        }
    }
}
