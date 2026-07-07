//! Domain types for the tria prima store (spec: sulfur/domains, mercury/threads,
//! salt/realizations, fire/tending, plus the supporting profile/traces/
//! kindling/correspondences/sessions tables). Pure data — no store logic here.

use serde::{Deserialize, Serialize};

use crate::error::CoreError;

/// Sulfur: a domain of desire/interest, seeded by pull-notes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Domain {
    pub id: String,
    pub name: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub device_id: String,
    pub deleted_at: Option<u64>,
}

/// A raw pull toward (or away from) a domain, captured before it's named.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullNote {
    pub id: String,
    pub domain_id: Option<String>,
    pub text: String,
    pub created_at: u64,
    pub device_id: String,
}

/// Mercury: an open question. Lifecycle enforced in `session.rs` (Task 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThreadState {
    Volatile,
    Condensing,
    Fixed,
    Evaporated,
}

impl ThreadState {
    pub fn as_str(self) -> &'static str {
        match self {
            ThreadState::Volatile => "volatile",
            ThreadState::Condensing => "condensing",
            ThreadState::Fixed => "fixed",
            ThreadState::Evaporated => "evaporated",
        }
    }
}

impl std::str::FromStr for ThreadState {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "volatile" => Ok(ThreadState::Volatile),
            "condensing" => Ok(ThreadState::Condensing),
            "fixed" => Ok(ThreadState::Fixed),
            "evaporated" => Ok(ThreadState::Evaporated),
            other => Err(CoreError::BadState(format!(
                "unknown thread state: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Thread {
    pub id: String,
    pub prompt: String,
    pub domain_id: Option<String>,
    pub state: ThreadState,
    pub born: u64,
    pub last_worked: Option<u64>,
    /// The spiral link: set when a realization's child_thread_id points back
    /// here (Task 9 `fix_salt`). NULL for threads not born of a realization.
    pub parent_realization_id: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
    pub device_id: String,
    pub deleted_at: Option<u64>,
}

/// Salt: an immutable realization. No updated_at/deleted_at — once written,
/// never mutated or tombstoned. `child_thread_id` is the spiral link: the
/// next open thread this realization gives birth to (set by `fix_salt`,
/// Task 9, which is the sole writer of this table).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Realization {
    pub id: String,
    pub text: String,
    pub date: u64,
    pub thread_id: String,
    pub child_thread_id: Option<String>,
    pub created_at: u64,
    pub device_id: String,
}

/// Fire: one row per UTC day tended. Append-only; wisdom = count(*).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tending {
    pub day: String,
    pub minutes: u32,
    pub thread_ids: Vec<String>,
    pub created_at: u64,
    pub device_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Correspondence {
    pub id: String,
    pub domain_a: String,
    pub domain_b: String,
    pub note: String,
    pub created_at: u64,
    pub device_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub thread_id: Option<String>,
    pub mask: String,
    pub mode: String,
    pub state: String,
    pub transcript: String,
    pub started_at: u64,
    pub ended_at: Option<u64>,
    pub created_at: u64,
    pub updated_at: u64,
    pub device_id: String,
}
