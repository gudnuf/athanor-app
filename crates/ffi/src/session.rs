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

use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Mutex as StdMutex, OnceLock};

use athanor_core::conductor::Conductor;
use athanor_core::engine::{AcpUpdate, MystagogueEngine};
use athanor_core::mask::{self, SharedMask};
use athanor_core::Store;
use futures::FutureExt;
use tokio::sync::Mutex as TokioMutex;

use crate::engine::{AthanorEngine, EngineError};
use crate::events::{ReplyRegister, SessionEvent, SessionEventListener};

/// The last panic's `file:line (message)`, captured by our panic hook so the
/// unwind-catcher can put the real fault location into the in-band error event
/// (a caught unwind's payload alone is just the message). Process-global, like
/// the hook itself; sessions drive turns serially so a captured value belongs to
/// the turn that just unwound.
static LAST_PANIC: StdMutex<Option<String>> = StdMutex::new(None);
static PANIC_HOOK: OnceLock<()> = OnceLock::new();

/// Installs the FFI-seam panic hook exactly once. The hook records the panic's
/// location + message into `LAST_PANIC` (so a subsequent `catch_unwind` can
/// surface the exact fault line) and then chains to the previous hook (so
/// existing logging/backtrace behavior is preserved). Idempotent via `OnceLock`
/// — safe to call from every `AthanorEngine::new`.
pub(crate) fn install_panic_hook() {
    PANIC_HOOK.get_or_init(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let loc = info
                .location()
                .map(|l| format!("{}:{}", l.file(), l.line()))
                .unwrap_or_else(|| "unknown location".to_string());
            let msg = info
                .payload()
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| info.payload().downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".to_string());
            *LAST_PANIC.lock().unwrap() = Some(format!("{loc}: {msg}"));
            prev(info);
        }));
    });
}

/// The calm, in-band error message for a contained panic — the exact fault
/// captured by the hook, or a generic line if (somehow) nothing was recorded.
fn contained_panic_message() -> String {
    let detail = LAST_PANIC
        .lock()
        .unwrap()
        .take()
        .unwrap_or_else(|| "internal fault".to_string());
    format!("internal fault (contained): {detail}")
}

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
    /// The session's live `(mask, mode, pinned)` register — shared with the
    /// `Conductor` + the `shift_mask` tool (lane 13). Read for the honest header
    /// and written by the pin escape hatch.
    mask_state: SharedMask,
    /// This session's id — needed to persist the mask when the learner pins.
    session_id: String,
    /// The last `(mask, mode)` surfaced to the listener as a `MaskShifted`, so a
    /// turn only emits the event when the register actually changed.
    last_emitted_mask: StdMutex<Option<(String, String)>>,
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
        let mask_state = conductor.mask_state();
        let session_id = conductor.session_id().to_string();
        Arc::new(SessionHandle {
            conductor: TokioMutex::new(Some(conductor)),
            store,
            engine,
            listener: StdMutex::new(None),
            mask_state,
            session_id,
            last_emitted_mask: StdMutex::new(None),
            runtime_handle,
        })
    }

    /// Surfaces the session's current `(mask, mode)` to the listener as a
    /// `MaskShifted` — but only when it differs from what was last surfaced. Run
    /// at the top of every turn: the first turn emits the opening register (so
    /// the header is truthful from the first reply), and a mid-session shift is
    /// surfaced on the first turn that runs under the new register.
    fn emit_mask_if_changed(&self, listener: &Option<Arc<dyn SessionEventListener>>) {
        let (mask, mode) = mask::current(&self.mask_state);
        let mut last = self.last_emitted_mask.lock().unwrap();
        if last.as_ref() != Some(&(mask.clone(), mode.clone())) {
            *last = Some((mask.clone(), mode.clone()));
            emit(listener, SessionEvent::MaskShifted { mask, mode });
        }
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

    /// This session's id — so the shell can read its detail/history back after
    /// close (e.g. to surface the freshly-condensed note).
    pub fn session_id(&self) -> String {
        self.session_id.clone()
    }

    /// The session's current mask id — for the honest header (lane 13).
    pub fn current_mask(&self) -> String {
        mask::current(&self.mask_state).0
    }

    /// The session's current mode id — for the honest header (lane 13).
    pub fn current_mode(&self) -> String {
        mask::current(&self.mask_state).1
    }

    /// The escape hatch: the learner pins a mask (header tap → picker). Pins the
    /// shared cell (so `shift_mask` no-ops for the rest of the session), persists
    /// it to the session row, and surfaces the choice to the listener as a
    /// `MaskShifted` so the header reflects it at once.
    pub fn pin_mask(&self, chosen: String) {
        mask::pin(&self.mask_state, &chosen);
        let mode = mask::current(&self.mask_state).1;
        // Persist the learner's choice; a store hiccup must never panic across
        // FFI, so it's logged-by-omission (the live cell is already authoritative).
        let _ = self
            .store
            .set_session_mask_mode(&self.session_id, &chosen, &mode);
        let listener = self.listener.lock().unwrap().clone();
        // Force the header to update now, even though no turn is running.
        {
            let mut last = self.last_emitted_mask.lock().unwrap();
            *last = Some((chosen.clone(), mode.clone()));
        }
        emit(&listener, SessionEvent::MaskShifted { mask: chosen, mode });
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

        // Surface the register this turn runs under (the opening pair on turn 1,
        // or a pair a prior turn's shift_mask moved us to) before any text.
        self.emit_mask_if_changed(&listener);

        let store = Arc::clone(&self.store);
        let mut cond = CondensationState::default();
        // Contain any panic from deep inside the engine at the FFI seam: catch
        // the unwind here and surface it as an in-band `Error` event rather than
        // letting it cross the uniffi boundary (a `try!` in the generated Swift)
        // and abort the host app.
        let driven = AssertUnwindSafe(conductor.run_turn(
            self.engine.as_ref(),
            Some(&text),
            &mut update_sink(&listener, &store, &mut cond),
        ))
        .catch_unwind()
        .await;
        finish_or_contain(&listener, &store, &cond, driven);
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

        self.emit_mask_if_changed(&listener);

        let store = Arc::clone(&self.store);
        let mut cond = CondensationState::default();
        // Same panic containment as `send_turn` (see there).
        let driven = AssertUnwindSafe(conductor.open_turn(
            self.engine.as_ref(),
            &mut update_sink(&listener, &store, &mut cond),
        ))
        .catch_unwind()
        .await;
        finish_or_contain(&listener, &store, &cond, driven);
    }

    /// Lands the session, in two steps whose ordering is the "nothing is lost"
    /// contract:
    /// 1. **Condensation (best-effort).** One final distillation turn through
    ///    the engine (`Conductor::condense`) writes the durable session note +
    ///    any profile refinements. Its error — network, key, or even a panic
    ///    from deep in the engine — is CONTAINED here and discarded: closing is
    ///    not best-effort, so nothing this step does can stop step 2.
    /// 2. **Close (always).** `close_session` (records tending — the only place
    ///    wisdom advances) + the one-line trace. Consumes the conductor.
    pub async fn close(&self, minutes: u32) -> Result<(), EngineError> {
        let mut guard = self.conductor.lock().await;
        // Step 1 — best-effort condensation, panic-contained at the FFI seam so
        // a fault in the distillation turn can never abort the host app (a
        // `try!` in the generated Swift) nor prevent the close below.
        if let Some(conductor) = guard.as_mut() {
            let driven = AssertUnwindSafe(conductor.condense(self.engine.as_ref()))
                .catch_unwind()
                .await;
            if driven.is_err() {
                // Drain the hook's capture so a contained condensation panic
                // doesn't mislabel a later turn's error.
                let _ = contained_panic_message();
            }
        }
        // Step 2 — the close itself, which ALWAYS runs.
        let conductor = guard
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

/// Shared tail of `send_turn`/`open`: reconciles the *caught-unwind* result of
/// driving a turn. Two failure modes, both surfaced as an in-band `Error` event
/// (never unwinding across FFI):
/// - `Ok(Err(e))` — an ordinary `ConductorError` the turn returned;
/// - `Err(_)` — a panic the engine raised, caught by `catch_unwind`; the exact
///   fault `file:line` + message come from the panic hook (`LAST_PANIC`).
///
/// The Condensation moment is fired inline from `fix_salt`'s `ToolResult`, so
/// there's nothing to do here on success.
fn finish_or_contain(
    listener: &Option<Arc<dyn SessionEventListener>>,
    _store: &Store,
    _cond: &CondensationState,
    result: Result<
        Result<(), athanor_core::conductor::ConductorError>,
        Box<dyn std::any::Any + Send>,
    >,
) {
    let message = match result {
        Ok(Ok(())) => return,
        Ok(Err(e)) => e.to_string(),
        Err(_panic) => contained_panic_message(),
    };
    emit(listener, SessionEvent::Error { message });
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
