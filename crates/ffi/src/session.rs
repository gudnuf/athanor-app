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
use crate::events::{SessionEvent, SessionEventListener, DEFAULT_REGISTER};

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
        let mut saw_fix_salt = false;
        let result = conductor
            .run_turn(
                self.engine.as_ref(),
                Some(&text),
                &mut update_sink(&listener, &store, &mut saw_fix_salt),
            )
            .await;
        finish(&listener, &store, saw_fix_salt, result);
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
        let mut saw_fix_salt = false;
        let result = conductor
            .open_turn(
                self.engine.as_ref(),
                &mut update_sink(&listener, &store, &mut saw_fix_salt),
            )
            .await;
        finish(&listener, &store, saw_fix_salt, result);
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

/// Builds the per-turn `AcpUpdate` sink shared by `send_turn` and `open`:
/// projects deltas/tool-calls/completion to `SessionEvent`s. Emits the turn's
/// `Condensation` ahead of `TurnComplete` (the ordering is delta*/toolcall/
/// condensation/complete — the engine awaits tool dispatch before it streams
/// `TurnComplete`, so the realization is already readable), tracking whether
/// a `fix_salt` landed via `saw_fix_salt` so the caller's degraded-path check
/// (a `fix_salt` with no trailing `TurnComplete`) knows whether it already
/// fired.
fn update_sink<'a>(
    listener: &'a Option<Arc<dyn SessionEventListener>>,
    store: &'a Store,
    saw_fix_salt: &'a mut bool,
) -> impl FnMut(AcpUpdate) + Send + 'a {
    move |update| match update {
        AcpUpdate::TextDelta(t) => emit(
            listener,
            SessionEvent::TextDelta {
                text: t,
                register: DEFAULT_REGISTER.to_string(),
            },
        ),
        AcpUpdate::ToolCall(call) => {
            if call.name == "fix_salt" {
                *saw_fix_salt = true;
            }
            emit(listener, SessionEvent::ToolCall { kind: call.name });
        }
        AcpUpdate::TurnComplete => {
            if *saw_fix_salt {
                emit_condensation(listener, store);
                *saw_fix_salt = false;
            }
            emit(listener, SessionEvent::TurnComplete);
        }
    }
}

/// Shared tail of `send_turn`/`open`: emits the turn's `Condensation` (if a
/// `fix_salt` landed and `TurnComplete` never streamed — the degraded path,
/// dispatch still landed regardless) or an `Error` event, given the
/// `Conductor::run_turn`/`open_turn` result.
fn finish(
    listener: &Option<Arc<dyn SessionEventListener>>,
    store: &Store,
    saw_fix_salt: bool,
    result: Result<(), athanor_core::conductor::ConductorError>,
) {
    match result {
        Ok(()) => {
            if saw_fix_salt {
                emit_condensation(listener, store);
            }
        }
        Err(e) => emit(
            listener,
            SessionEvent::Error {
                message: e.to_string(),
            },
        ),
    }
}

/// Reads the just-fixed grain back out of the store and emits the synthesized
/// `Condensation`. `list_realizations` is `date ASC, created_at ASC`, so the
/// last entry is the newest — the one this turn's `fix_salt` just wrote. A read
/// failure degrades to no event rather than unwinding across FFI.
fn emit_condensation(listener: &Option<Arc<dyn SessionEventListener>>, store: &Store) {
    let Ok(grains) = store.list_realizations() else {
        return;
    };
    let Some(last) = grains.last() else { return };
    emit(
        listener,
        SessionEvent::Condensation {
            realization_id: last.realization.id.clone(),
            child_thread_id: last.realization.child_thread_id.clone(),
        },
    );
}
