//! `SessionHandle` (Plan Phase 4, Task C2): one session's bridge state. Drives
//! the core `Conductor` a turn at a time on the engine's runtime, projecting
//! each `AcpUpdate` into a `SessionEvent` for a `with_foreign` listener.
//!
//! Event synthesis (review edit #4 — `AcpUpdate` has no such variants):
//! - **`Condensation`** is derived by observing a `fix_salt` `ToolCall` during
//!   the turn and, once the turn's tool dispatch has landed the realization,
//!   reading the newest grain back out of the store (`list_realizations` is
//!   `date ASC` → the last entry is the one just fixed) for its
//!   `realization_id`/`child_thread_id`. It is emitted *before* `TurnComplete`.
//! - **`Error`** is derived from a `run_turn` that returns `Err` — surfaced,
//!   never unwound across the FFI boundary.

use std::sync::{Arc, Mutex as StdMutex};

use athanor_core::conductor::Conductor;
use athanor_core::engine::{AcpUpdate, MystagogueEngine};
use athanor_core::Store;
use tokio::sync::Mutex as TokioMutex;

use crate::engine::{AthanorEngine, EngineError};
use crate::events::{ReplyRegister, SessionEvent, SessionEventListener};

/// Default voice/work-mode when the caller doesn't pick one.
const DEFAULT_MASK: &str = "philosophus";
const DEFAULT_MODE: &str = "explain";

/// One session's bridge state. The `Conductor` is not `Clone` and its
/// `close`/`abandon` consume `self`, so it lives behind a `TokioMutex<Option>`:
/// `send_turn` locks it `&mut` across the `run_turn` await, and `close`/
/// `abandon` `take()` it out.
#[derive(uniffi::Object)]
pub struct SessionHandle {
    conductor: TokioMutex<Option<Conductor>>,
    store: Arc<Store>,
    engine: Arc<dyn MystagogueEngine>,
    listener: StdMutex<Option<Arc<dyn SessionEventListener>>>,
    #[allow(dead_code)]
    runtime_handle: tokio::runtime::Handle,
}

impl SessionHandle {
    fn new(
        conductor: Conductor,
        store: Arc<Store>,
        engine: Arc<dyn MystagogueEngine>,
        runtime_handle: tokio::runtime::Handle,
    ) -> Arc<Self> {
        Arc::new(SessionHandle {
            conductor: TokioMutex::new(Some(conductor)),
            store,
            engine,
            listener: StdMutex::new(None),
            runtime_handle,
        })
    }
}

#[uniffi::export]
impl AthanorEngine {
    /// Opens an ordinary session against `(mask, mode, thread_id)` — each
    /// `None` falls back to the default voice/mode/no-thread. Fallible across
    /// FFI (no panic): a store error surfaces as `EngineError::Session`.
    pub fn begin_session(
        &self,
        mask: Option<String>,
        mode: Option<String>,
        thread_id: Option<String>,
    ) -> Result<Arc<SessionHandle>, EngineError> {
        let mask = mask.unwrap_or_else(|| DEFAULT_MASK.to_string());
        let mode = mode.unwrap_or_else(|| DEFAULT_MODE.to_string());
        let conductor =
            Conductor::begin(Arc::clone(&self.store), &mask, &mode, thread_id.as_deref())
                .map_err(|e| EngineError::Session(e.to_string()))?;
        Ok(SessionHandle::new(
            conductor,
            Arc::clone(&self.store),
            Arc::clone(&self.engine),
            self.runtime_handle.clone(),
        ))
    }

    /// Opens the first-launch initiation session (cold start).
    pub fn begin_initiation(&self) -> Result<Arc<SessionHandle>, EngineError> {
        let conductor = Conductor::begin_initiation(Arc::clone(&self.store))
            .map_err(|e| EngineError::Session(e.to_string()))?;
        Ok(SessionHandle::new(
            conductor,
            Arc::clone(&self.store),
            Arc::clone(&self.engine),
            self.runtime_handle.clone(),
        ))
    }
}

#[uniffi::export(async_runtime = "tokio")]
impl SessionHandle {
    /// Stores the per-session listener (fresh per session).
    pub fn set_listener(&self, listener: Arc<dyn SessionEventListener>) {
        *self.listener.lock().unwrap() = Some(listener);
    }

    /// Drives one learner turn through the `Conductor`, streaming projected
    /// `SessionEvent`s to the listener as `AcpUpdate`s arrive. Never panics
    /// across FFI: a failed turn (or a call after the session ended) surfaces
    /// as a `SessionEvent::Error`.
    pub async fn send_turn(&self, text: String) {
        let listener = self.listener.lock().unwrap().clone();
        let mut guard = self.conductor.lock().await;
        let Some(conductor) = guard.as_mut() else {
            emit(
                &listener,
                SessionEvent::Error {
                    message: "session already ended".to_string(),
                },
            );
            return;
        };

        let store = Arc::clone(&self.store);
        let mut cond = CondensationState::default();
        let result = conductor
            .run_turn(
                self.engine.as_ref(),
                Some(&text),
                &mut update_sink(&listener, &store, &mut cond),
            )
            .await;
        finish(&listener, &store, &cond, result);
    }

    /// Runs the ritual opening turn (BLOCKER-1 deep fix): the Mystagogue
    /// speaks first, primed by the versioned prompt pack's synthesized
    /// learner-arrival marker rather than any real learner utterance. Call
    /// once, right after `set_listener`, before any `send_turn` — in
    /// practice, only meaningful for a session opened via `begin_initiation`
    /// (initiation is the one flow with no other first-speaker channel; see
    /// `Conductor::open_turn`). Never panics across FFI: a failed turn (or a
    /// call after the session ended) surfaces as a `SessionEvent::Error`.
    pub async fn open(&self) {
        let listener = self.listener.lock().unwrap().clone();
        let mut guard = self.conductor.lock().await;
        let Some(conductor) = guard.as_mut() else {
            emit(
                &listener,
                SessionEvent::Error {
                    message: "session already ended".to_string(),
                },
            );
            return;
        };

        let store = Arc::clone(&self.store);
        let mut cond = CondensationState::default();
        let result = conductor
            .open_turn(
                self.engine.as_ref(),
                &mut update_sink(&listener, &store, &mut cond),
            )
            .await;
        finish(&listener, &store, &cond, result);
    }

    /// Lands the session: `close_session` (records tending — the only place
    /// wisdom advances) + writes the one-line trace. Consumes the conductor.
    pub async fn close(&self, minutes: u32) -> Result<(), EngineError> {
        let conductor = self
            .conductor
            .lock()
            .await
            .take()
            .ok_or_else(|| EngineError::Session("session already ended".to_string()))?;
        conductor
            .close(minutes)
            .map(|_| ())
            .map_err(|e| EngineError::Session(e.to_string()))
    }

    /// Abandons the session: returns its thread (if any) to volatile, writes no
    /// trace. Consumes the conductor.
    pub async fn abandon(&self) -> Result<(), EngineError> {
        let conductor = self
            .conductor
            .lock()
            .await
            .take()
            .ok_or_else(|| EngineError::Session("session already ended".to_string()))?;
        conductor
            .abandon()
            .map(|_| ())
            .map_err(|e| EngineError::Session(e.to_string()))
    }
}

/// Emits one event to the listener, if a listener is set. A `None` listener is
/// a no-op — the turn still drives the conductor (so close/abandon semantics
/// hold) even when nothing is watching.
fn emit(listener: &Option<Arc<dyn SessionEventListener>>, event: SessionEvent) {
    if let Some(l) = listener {
        l.on_event(event);
    }
}

/// Per-turn condensation bookkeeping: the id of a pending `fix_salt` tool call,
/// so its `ToolResult` (which carries the real realization id) can be matched.
#[derive(Default)]
struct CondensationState {
    pending_fix_salt_id: Option<String>,
}

/// Builds the per-turn `AcpUpdate` sink shared by `send_turn` and `open`:
/// projects deltas/tool-calls/completion to `SessionEvent`s. The Condensation
/// moment fires from `fix_salt`'s own `ToolResult` (the REAL realization id +
/// the fixed salt's text), before `TurnComplete` — so the ordering the Session
/// screen sees is delta*/toolcall/condensation/complete. A `fix_salt` whose
/// result never arrives degrades to the store's newest grain at `TurnComplete`
/// (or in `finish`), so the moment is never simply dropped.
fn update_sink<'a>(
    listener: &'a Option<Arc<dyn SessionEventListener>>,
    store: &'a Store,
    cond: &'a mut CondensationState,
) -> impl FnMut(AcpUpdate) + Send + 'a {
    move |update| match update {
        AcpUpdate::TextDelta { text, register } => emit(
            listener,
            SessionEvent::TextDelta {
                text,
                register: ReplyRegister::from(register),
            },
        ),
        AcpUpdate::ToolCall(call) => {
            if call.name == "fix_salt" {
                cond.pending_fix_salt_id = Some(call.id.clone());
            }
            emit(listener, SessionEvent::ToolCall { kind: call.name });
        }
        AcpUpdate::ToolResult(result) => {
            // Fire the moment ONLY for a fix_salt that actually fixed a salt —
            // a result carrying a real `realization_id`. A fix_salt that ERRORED
            // (bad thread, already-fixed) returns `{error: …}` and must NOT
            // condense a stale grain as if it were new.
            if cond.pending_fix_salt_id.as_deref() == Some(result.id.as_str()) {
                cond.pending_fix_salt_id = None;
                emit_condensation_from_result(listener, store, &result);
            }
        }
        AcpUpdate::TurnComplete => emit(listener, SessionEvent::TurnComplete),
    }
}

/// Shared tail of `send_turn`/`open`: surfaces an `Error` event (never unwinds
/// across FFI). The Condensation moment is fired inline from `fix_salt`'s
/// `ToolResult`, so nothing to do here on success.
fn finish(
    listener: &Option<Arc<dyn SessionEventListener>>,
    _store: &Store,
    _cond: &CondensationState,
    result: Result<(), athanor_core::conductor::ConductorError>,
) {
    if let Err(e) = result {
        emit(
            listener,
            SessionEvent::Error {
                message: e.to_string(),
            },
        );
    }
}

/// Emits the Condensation from `fix_salt`'s own result value
/// (`{realization_id, child_thread_id}`), reading the fixed salt's TEXT back by
/// that exact id. An error result (no `realization_id`) emits nothing — no
/// salt was fixed, so no moment.
fn emit_condensation_from_result(
    listener: &Option<Arc<dyn SessionEventListener>>,
    store: &Store,
    result: &athanor_core::engine::AcpToolResult,
) {
    let Some(rid) = result.value.get("realization_id").and_then(|v| v.as_str()) else {
        return;
    };
    let child = result
        .value
        .get("child_thread_id")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    emit(
        listener,
        SessionEvent::Condensation {
            realization_id: rid.to_string(),
            child_thread_id: child,
            text: realization_text(store, rid),
        },
    );
}

/// The text of the realization with `id`, or empty if not found.
fn realization_text(store: &Store, id: &str) -> String {
    store
        .list_realizations()
        .ok()
        .and_then(|grains| {
            grains
                .into_iter()
                .find(|g| g.realization.id == id)
                .map(|g| g.realization.text)
        })
        .unwrap_or_default()
}
