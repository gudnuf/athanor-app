//! `AthanorEngine` (Plan Phase 4, Task C1): the FFI entry point Swift
//! constructs once per app. Holds the `Arc<Store>` + a tokio runtime handle +
//! the injected `MystagogueEngine` (the real `GooseEngine` behind
//! `feature = "goose"` when a key is present, else a hermetic `MockEngine`
//! demo). Hands out the read projections (C1) and per-session `SessionHandle`s
//! (C2, `session.rs`).
//!
//! Key discipline (PROCESS.md / rmp invariant): the Anthropic key crosses the
//! boundary as a **constructor parameter** (from the iOS Keychain) — this crate
//! never reads it from the environment.

use std::sync::Arc;

use athanor_core::engine::{AcpUpdate, MockEngine, MystagogueEngine};
use athanor_core::Store;

use crate::records::{FurnaceState, GrimoireGrain, OpenThread, TabulaPassage};

/// Errors that cross the FFI boundary as a thrown error rather than a panic
/// (no panics across FFI). `flat_error`: Swift receives the variant plus its
/// `Display` message. The Anthropic key never appears in any of these strings.
#[derive(Debug, thiserror::Error, uniffi::Error)]
#[uniffi(flat_error)]
pub enum EngineError {
    /// The on-device store could not be opened (bad path, permissions, corrupt
    /// db). Recoverable by the host — surface, don't crash.
    #[error("failed to open store: {0}")]
    Store(String),
    /// The bridge's tokio runtime could not be started.
    #[error("failed to start the bridge runtime: {0}")]
    Runtime(String),
    /// A read projection failed (store lock/query error).
    #[error("read failed: {0}")]
    Read(String),
    /// A session could not be begun/driven/landed.
    #[error("session error: {0}")]
    Session(String),
}

/// Model-tier config crossing the boundary. Kept minimal: the only knob that
/// reaches the engine today is which Anthropic model to drive (`None` = goose's
/// default, `claude-sonnet-4-5`). A `Debug` is derived — no secret lives here
/// (the key is a separate constructor param).
#[derive(uniffi::Record, Clone, Debug, Default)]
pub struct TierConfig {
    /// The Anthropic model id to drive, or `None` for the engine default.
    pub model: Option<String>,
}

/// Builds the engine seam implementation for a `(key, tier)` pair. With the
/// `goose` feature AND a key present → the real `GooseEngine`; otherwise a
/// hermetic `MockEngine` demo (scripted-updates — see `demo_engine`).
#[cfg_attr(not(feature = "goose"), allow(unused_variables))]
fn build_engine(anthropic_key: Option<String>, tier: &TierConfig) -> Arc<dyn MystagogueEngine> {
    #[cfg(feature = "goose")]
    {
        if let Some(key) = anthropic_key {
            return Arc::new(athanor_core::engine::GooseEngine::new(
                key,
                tier.model.clone(),
            ));
        }
    }
    Arc::new(demo_engine())
}

/// The demo engine: a single canned, hermetic turn so a clean checkout (no key,
/// no `goose`) still streams *something* through the whole bridge. It drains
/// its script on the first turn — day-1 demo only; multi-turn demo replay is
/// out of scope (flagged in the C1/C2 report).
fn demo_engine() -> MockEngine {
    MockEngine::new(vec![
        AcpUpdate::TextDelta(
            "The furnace is warm. This is a demo — wire a key to hear the Mystagogue.".into(),
        ),
        AcpUpdate::TurnComplete,
    ])
}

/// The FFI entry point. One per app; `begin_session`/`begin_initiation`
/// (`session.rs`) hand out per-session `SessionHandle`s.
#[derive(uniffi::Object)]
pub struct AthanorEngine {
    pub(crate) store: Arc<Store>,
    pub(crate) engine: Arc<dyn MystagogueEngine>,
    /// Handle sessions clone to drive `run_turn` on the owned runtime. In
    /// production the engine owns the `Runtime` (`_runtime`, kept alive for the
    /// engine's lifetime); tests borrow the `#[tokio::test]` runtime.
    pub(crate) runtime_handle: tokio::runtime::Handle,
    _runtime: Option<Arc<tokio::runtime::Runtime>>,
}

#[uniffi::export]
impl AthanorEngine {
    /// Fallible across FFI (uniffi throwing constructor): opening the store or
    /// starting the runtime can fail on a real device, and a panic here would
    /// crash the host app instead of letting Swift handle it.
    ///
    /// `anthropic_key` is an opaque `String` from the iOS Keychain, `None` in
    /// demo mode. `tier` selects the model. The key is never logged and never
    /// read from the environment.
    #[uniffi::constructor]
    pub fn new(
        db_path: String,
        anthropic_key: Option<String>,
        tier: TierConfig,
    ) -> Result<Arc<Self>, EngineError> {
        let store =
            Store::open(&db_path, "device").map_err(|e| EngineError::Store(e.to_string()))?;
        let runtime = Arc::new(
            tokio::runtime::Runtime::new().map_err(|e| EngineError::Runtime(e.to_string()))?,
        );
        let runtime_handle = runtime.handle().clone();
        let engine = build_engine(anthropic_key, &tier);
        Ok(Arc::new(AthanorEngine {
            store: Arc::new(store),
            engine,
            runtime_handle,
            _runtime: Some(runtime),
        }))
    }

    /// The Furnace read: held heat + days tended (`Store::fire_state`).
    pub fn furnace_state(&self) -> Result<FurnaceState, EngineError> {
        self.store
            .fire_state()
            .map(FurnaceState::from)
            .map_err(|e| EngineError::Read(e.to_string()))
    }

    /// The Grimoire read: every fixed grain of salt, chronological
    /// (`Store::list_realizations`).
    pub fn grimoire(&self) -> Result<Vec<GrimoireGrain>, EngineError> {
        self.store
            .list_realizations()
            .map(|v| v.into_iter().map(GrimoireGrain::from).collect())
            .map_err(|e| EngineError::Read(e.to_string()))
    }

    /// The Mercury read: open threads (volatile + condensing)
    /// (`Store::open_threads`).
    pub fn mercury(&self) -> Result<Vec<OpenThread>, EngineError> {
        self.store
            .open_threads()
            .map(|v| v.into_iter().map(OpenThread::from).collect())
            .map_err(|e| EngineError::Read(e.to_string()))
    }

    /// The Tabula read: the seven canonical passages (number/title/body)
    /// projected against this learner's kindling state (`Store::tabula`).
    /// Always seven, in scroll order — dim until the learner's practice lights
    /// them.
    pub fn tabula(&self) -> Result<Vec<TabulaPassage>, EngineError> {
        self.store
            .tabula()
            .map(|v| v.into_iter().map(TabulaPassage::from).collect())
            .map_err(|e| EngineError::Read(e.to_string()))
    }
}

impl AthanorEngine {
    /// Test-only constructor injecting a store + a specific engine (never
    /// crosses FFI — no `#[uniffi::export]`, so it doesn't affect the generated
    /// Swift bindings). Borrows the calling `#[tokio::test]` runtime rather than
    /// spinning up a second one. `pub`, not `#[cfg(test)]`, because an
    /// integration-test binary compiles this crate as an ordinary dependency —
    /// `#[cfg(test)]` items would not exist for it to call.
    #[doc(hidden)]
    pub fn with_engine(store: Arc<Store>, engine: Arc<dyn MystagogueEngine>) -> Arc<Self> {
        Arc::new(AthanorEngine {
            store,
            engine,
            runtime_handle: tokio::runtime::Handle::current(),
            _runtime: None,
        })
    }
}
