//! The Mystagogue extension: the seven tools the model wields over the `Store`.
//!
//! `Mystagogue` is a `ToolDispatch` impl (the engine seam, Task 8) whose tools
//! each write to the tria prima store (or, for `shift_mask`, the session's live
//! register):
//!
//! | tool              | store effect                                            |
//! |-------------------|---------------------------------------------------------|
//! | `fix_salt`        | immutable realization + auto-born child thread; SALT    |
//! | `open_thread`     | a new volatile thread (open question)                   |
//! | `evaporate_thread`| mark a thread evaporated                                 |
//! | `kindle_passage`  | kindle a Tabula passage (first-wins)                    |
//! | `weave_domains`   | a correspondence; kindle CITRINITAS + AZOTH             |
//! | `update_memory`   | set a learner-profile section                           |
//! | `shift_mask`      | move the session's `(mask, mode)` register (lane 13)    |
//!
//! The spiral invariant lives in `Store::fix_salt` (one transaction: the
//! realization AND its child thread, or neither). The model speaks in domain
//! NAMES; the tools upsert names → ids at the boundary.

pub use crate::engine::{AcpToolCall, AcpToolResult, AcpToolSpec, ToolDispatch};

use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::domain::ThreadState;
use crate::error::CoreError;
use crate::mask::{self, SharedMask};
use crate::prompt::assets::{MASK_IDS, MODE_IDS};
use crate::store::Store;

pub struct Mystagogue {
    store: Arc<Store>,
    /// The session's focal thread, if it opened on one — the fallback target for
    /// `fix_salt` when the model fumbles the `thread_id` (see
    /// `resolve_salt_thread`). Set by the `Conductor` at session start.
    focal_thread: Option<String>,
    /// The session's live `(mask, mode)` cell + the id of the session it belongs
    /// to — set by the `Conductor` so the `shift_mask` tool can move the register
    /// (and persist it). `None` outside a real session (bare tool-dispatch tests).
    mask_state: Option<SharedMask>,
    session_id: Option<String>,
}

impl Mystagogue {
    pub fn new(store: Arc<Store>) -> Self {
        Self {
            store,
            focal_thread: None,
            mask_state: None,
            session_id: None,
        }
    }

    /// Records the session's focal thread so `fix_salt` can fall back to it.
    pub fn with_focal_thread(mut self, focal_thread: Option<String>) -> Self {
        self.focal_thread = focal_thread;
        self
    }

    /// Wires the session's shared mask cell + id so `shift_mask` can move the
    /// register mid-session and persist it to the session row.
    pub fn with_mask_state(mut self, mask_state: SharedMask, session_id: String) -> Self {
        self.mask_state = Some(mask_state);
        self.session_id = Some(session_id);
        self
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    /// Chooses the thread a `fix_salt` should condense, tolerating the model
    /// fumbling the id (it can only see thread PROMPTS in the assembled prompt,
    /// not uuids, and reliably invents refs like `ripe_mercury[0]`). In order:
    /// the given id if it's a real, still-open thread; else the session's focal
    /// thread; else the single ripest open thread; else a crisp error naming the
    /// open threads. Salt is never dropped on the floor because of a bad id.
    fn resolve_salt_thread(&self, given: Option<&str>) -> Result<String, CoreError> {
        let fixable = |id: &str| -> bool {
            matches!(
                self.store.get_thread(id).map(|t| t.state),
                Ok(ThreadState::Volatile) | Ok(ThreadState::Condensing)
            )
        };
        if let Some(id) = given.map(str::trim).filter(|s| !s.is_empty()) {
            if fixable(id) {
                return Ok(id.to_string());
            }
        }
        if let Some(focal) = self.focal_thread.as_deref() {
            if fixable(focal) {
                return Ok(focal.to_string());
            }
        }
        if let Some(ripe) = self.store.ripe_threads(1)?.into_iter().next() {
            return Ok(ripe.id);
        }
        let open: Vec<String> = self
            .store
            .open_threads()?
            .into_iter()
            .map(|t| t.prompt)
            .collect();
        Err(CoreError::BadState(format!(
            "no open thread to fix salt against{}",
            if open.is_empty() {
                " (none are open)".to_string()
            } else {
                format!("; open threads: {}", open.join("; "))
            }
        )))
    }

    /// Moves the session's register (lane 13). Validates against the real mask
    /// (and, if given, mode) rosters — an invalid id returns a crisp error value
    /// naming the valid ones rather than silently doing nothing. When the learner
    /// has pinned a mask, this no-ops and tells the model so, so it cooperates
    /// with the human's choice rather than fighting it. On success the shared
    /// cell is updated (taking effect on the Conductor's NEXT assemble) and the
    /// session row is persisted.
    fn shift_mask(&self, mask: &str, mode: Option<&str>) -> Result<Value, CoreError> {
        let Some(state) = self.mask_state.as_ref() else {
            return Err(CoreError::BadState(
                "shift_mask is only available inside a live session".into(),
            ));
        };
        if !MASK_IDS.contains(&mask) {
            return Err(CoreError::BadState(format!(
                "unknown mask '{mask}'; valid masks: {}",
                MASK_IDS.join(", ")
            )));
        }
        if let Some(m) = mode {
            if !MODE_IDS.contains(&m) {
                return Err(CoreError::BadState(format!(
                    "unknown mode '{m}'; valid modes: {}",
                    MODE_IDS.join(", ")
                )));
            }
        }
        // The learner's pin wins: don't shift, and say why, so the model stops
        // trying to steer the register itself.
        if mask::is_pinned(state) {
            let (pinned_mask, _) = mask::current(state);
            return Ok(json!({
                "shifted": false,
                "note": format!(
                    "the learner has chosen the {pinned_mask} register for this session — stay in it"
                ),
            }));
        }

        let (new_mask, new_mode) = {
            let mut s = state.lock().unwrap();
            s.mask = mask.to_string();
            if let Some(m) = mode {
                s.mode = m.to_string();
            }
            (s.mask.clone(), s.mode.clone())
        };
        if let Some(id) = self.session_id.as_deref() {
            self.store.set_session_mask_mode(id, &new_mask, &new_mode)?;
        }
        Ok(json!({ "shifted": true, "mask": new_mask, "mode": new_mode }))
    }

    /// The tool specs exposed to the engine for prompt assembly (Task 10).
    /// Names are stable; the JSON schemas describe each tool's arguments.
    pub fn tool_specs() -> Vec<AcpToolSpec> {
        vec![
            AcpToolSpec {
                name: "fix_salt".into(),
                json_schema: json!({
                    "type": "object",
                    "properties": {
                        "realization": { "type": "string", "description": "the immutable insight, in the learner's own words" },
                        "thread_id": { "type": "string", "description": "optional — the thread this realization condenses; omit to condense the current ripe thread (you see thread prompts, not ids, so prefer omitting it)" },
                        "domains": { "type": "array", "items": { "type": "string" }, "description": "domain names this realization touches" },
                        "child_question": { "type": "string", "description": "the next question this opens (optional; one is synthesized if absent)" }
                    },
                    "required": ["realization"]
                }),
            },
            AcpToolSpec {
                name: "open_thread".into(),
                json_schema: json!({
                    "type": "object",
                    "properties": {
                        "question": { "type": "string" },
                        "domain": { "type": "string", "description": "domain name (optional)" }
                    },
                    "required": ["question"]
                }),
            },
            AcpToolSpec {
                name: "evaporate_thread".into(),
                json_schema: json!({
                    "type": "object",
                    "properties": { "id": { "type": "string" } },
                    "required": ["id"]
                }),
            },
            AcpToolSpec {
                name: "kindle_passage".into(),
                json_schema: json!({
                    "type": "object",
                    "properties": { "term": { "type": "string", "description": "the passage key to kindle" } },
                    "required": ["term"]
                }),
            },
            AcpToolSpec {
                name: "weave_domains".into(),
                json_schema: json!({
                    "type": "object",
                    "properties": {
                        "a": { "type": "string" },
                        "b": { "type": "string" },
                        "note": { "type": "string" }
                    },
                    "required": ["a", "b", "note"]
                }),
            },
            AcpToolSpec {
                name: "update_memory".into(),
                json_schema: json!({
                    "type": "object",
                    "properties": {
                        "section": { "type": "string" },
                        "content": { "type": "string" }
                    },
                    "required": ["section", "content"]
                }),
            },
            AcpToolSpec {
                name: "shift_mask".into(),
                json_schema: json!({
                    "type": "object",
                    "description": "Change the mask (and optionally the mode) you wear, when the moment calls for a different register. Takes effect on your NEXT reply. Shift QUIETLY — never announce it, never perform it; the change in register is itself the signal. Masks: philosophus (the midwife — only asks), adamas (the diamond — presses, holds rigor), solve (the frame-breaker — enters when stuck).",
                    "properties": {
                        "mask": { "type": "string", "enum": ["philosophus", "adamas", "solve"], "description": "the mask to wear from your next reply on" },
                        "mode": { "type": "string", "enum": ["trace", "explain", "predict", "challenge", "design"], "description": "optional — the work mode to shift to alongside the mask" }
                    },
                    "required": ["mask"]
                }),
            },
        ]
    }

    /// Runs a tool by name over the store, returning the JSON value the engine
    /// hands back. Errors surface as values (`dispatch` never fails the turn).
    fn run(&self, name: &str, args: Value) -> Result<Value, CoreError> {
        match name {
            "fix_salt" => {
                let a: FixSaltArgs = serde_json::from_value(args)?;
                // Tolerate a fumbled/omitted thread id — the model can't see
                // uuids, only prompts. Resolve to the real thread this salt
                // condenses (given → focal → ripest) so a good realization is
                // never lost to a bad reference.
                let thread_id = self.resolve_salt_thread(a.thread_id.as_deref())?;
                let realization = self.store.fix_salt(
                    &thread_id,
                    &a.realization,
                    &a.domains,
                    a.child_question.as_deref(),
                )?;
                Ok(json!({
                    "realization_id": realization.id,
                    "child_thread_id": realization.child_thread_id,
                }))
            }
            "open_thread" => {
                let a: OpenThreadArgs = serde_json::from_value(args)?;
                // The model speaks in domain names; resolve to an id if given.
                let domain_id = match a.domain.as_deref() {
                    Some(name) if !name.trim().is_empty() => {
                        Some(self.store.upsert_domain(name)?.id)
                    }
                    _ => None,
                };
                let thread = self
                    .store
                    .open_thread(&a.question, domain_id.as_deref(), None)?;
                Ok(json!({ "thread_id": thread.id }))
            }
            "evaporate_thread" => {
                let a: EvaporateArgs = serde_json::from_value(args)?;
                self.store.evaporate_thread(&a.id)?;
                Ok(json!({ "thread_id": a.id, "state": "evaporated" }))
            }
            "kindle_passage" => {
                let a: KindleArgs = serde_json::from_value(args)?;
                let kindled = self.store.kindle_passage(&a.term, None)?;
                Ok(json!({ "term": a.term, "kindled": kindled }))
            }
            "weave_domains" => {
                let a: WeaveArgs = serde_json::from_value(args)?;
                let corr = self.store.weave_domains(&a.a, &a.b, &a.note)?;
                // A correspondence lights the yellowing and the union.
                self.store.kindle_passage("CITRINITAS", Some(&corr.id))?;
                self.store.kindle_passage("AZOTH", Some(&corr.id))?;
                Ok(json!({ "correspondence_id": corr.id }))
            }
            "update_memory" => {
                let a: UpdateMemoryArgs = serde_json::from_value(args)?;
                self.store.set_profile_section(&a.section, &a.content)?;
                Ok(json!({ "section": a.section, "ok": true }))
            }
            "shift_mask" => {
                let a: ShiftMaskArgs = serde_json::from_value(args)?;
                self.shift_mask(&a.mask, a.mode.as_deref())
            }
            other => Err(CoreError::BadState(format!("unknown tool: {other}"))),
        }
    }
}

#[async_trait::async_trait]
impl ToolDispatch for Mystagogue {
    async fn dispatch(&self, call: AcpToolCall) -> AcpToolResult {
        let value = match self.run(&call.name, call.args) {
            Ok(value) => value,
            Err(err) => json!({ "error": err.to_string() }),
        };
        AcpToolResult { id: call.id, value }
    }
}

#[derive(Deserialize)]
struct FixSaltArgs {
    realization: String,
    /// Optional: the model only sees thread prompts, not uuids, so a missing or
    /// fumbled id resolves to the session's focal/ripest thread
    /// (`resolve_salt_thread`).
    #[serde(default)]
    thread_id: Option<String>,
    #[serde(default)]
    domains: Vec<String>,
    #[serde(default)]
    child_question: Option<String>,
}

#[derive(Deserialize)]
struct OpenThreadArgs {
    question: String,
    #[serde(default)]
    domain: Option<String>,
}

#[derive(Deserialize)]
struct EvaporateArgs {
    id: String,
}

#[derive(Deserialize)]
struct KindleArgs {
    term: String,
}

#[derive(Deserialize)]
struct WeaveArgs {
    a: String,
    b: String,
    note: String,
}

#[derive(Deserialize)]
struct UpdateMemoryArgs {
    section: String,
    content: String,
}

#[derive(Deserialize)]
struct ShiftMaskArgs {
    mask: String,
    #[serde(default)]
    mode: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // `Arc<Store>` per the plan's `Mystagogue::new` signature. `Store` is now
    // `Send + Sync` (its `Connection` sits behind a `ReentrantMutex`), so the
    // dispatcher satisfies the engine seam's `ToolDispatch: Send + Sync`.
    fn store_arc(store: Store) -> Arc<Store> {
        Arc::new(store)
    }

    fn call(name: &str, args: Value) -> AcpToolCall {
        AcpToolCall {
            id: "1".into(),
            name: name.into(),
            args,
        }
    }

    // ---- fix_salt thread resolution (lane 9: model can't see uuids) ----

    #[tokio::test]
    async fn fix_salt_with_a_fumbled_thread_id_falls_back_to_the_focal_thread() {
        let store = Store::open_in_memory("dev").unwrap();
        let focal = store
            .open_thread("why does it collapse?", None, None)
            .unwrap();
        let other = store.open_thread("a different thread", None, None).unwrap();
        let myst = Mystagogue::new(store_arc(store)).with_focal_thread(Some(focal.id.clone()));
        // The model invents a ref it saw in the prompt — not a real uuid.
        let res = myst
            .dispatch(call(
                "fix_salt",
                json!({ "realization": "the collapse is overproofing", "thread_id": "ripe_mercury[0]" }),
            ))
            .await;
        assert!(
            res.value.get("realization_id").is_some(),
            "salt fixed anyway: {:?}",
            res.value
        );
        // It condensed the FOCAL thread, not the other one.
        assert_eq!(
            myst.store().get_thread(&focal.id).unwrap().state,
            ThreadState::Fixed
        );
        assert_eq!(
            myst.store().get_thread(&other.id).unwrap().state,
            ThreadState::Volatile
        );
    }

    #[tokio::test]
    async fn fix_salt_with_no_thread_id_condenses_the_ripest_open_thread() {
        let store = Store::open_in_memory("dev").unwrap();
        let ripe = store.open_thread("the ripe one", None, None).unwrap();
        store
            .set_thread_state(&ripe.id, ThreadState::Condensing)
            .unwrap();
        store
            .open_thread("a younger volatile one", None, None)
            .unwrap();
        // No focal thread, and the model omits thread_id entirely.
        let myst = Mystagogue::new(store_arc(store));
        let res = myst
            .dispatch(call("fix_salt", json!({ "realization": "it clicked" })))
            .await;
        assert!(res.value.get("realization_id").is_some(), "{:?}", res.value);
        assert_eq!(
            myst.store().get_thread(&ripe.id).unwrap().state,
            ThreadState::Fixed,
            "the condensing (ripest) thread was chosen"
        );
    }

    #[tokio::test]
    async fn fix_salt_with_no_open_threads_errors_crisply() {
        let store = Store::open_in_memory("dev").unwrap();
        let myst = Mystagogue::new(store_arc(store));
        let res = myst
            .dispatch(call(
                "fix_salt",
                json!({ "realization": "nowhere to put it" }),
            ))
            .await;
        let err = res.value["error"].as_str().unwrap();
        assert!(err.contains("no open thread"), "crisp error: {err}");
    }

    // ---- the load-bearing spiral test (plan Task 9, Step 1) ----
    #[tokio::test]
    async fn fix_salt_writes_immutable_realization_and_births_child_thread() {
        let store = Store::open_in_memory("dev").unwrap();
        let d = store.upsert_domain("thermodynamics").unwrap();
        let parent = store
            .open_thread("what is entropy?", Some(&d.id), None)
            .unwrap();
        let myst = Mystagogue::new(store_arc(store));
        let res = myst
            .dispatch(call(
                "fix_salt",
                json!({
                    "realization": "entropy is lost ways-to-not-know",
                    "thread_id": parent.id,
                    "domains": ["thermodynamics"]
                }),
            ))
            .await;
        let rid = res.value["realization_id"].as_str().unwrap().to_string();
        // spiral: the realization has a child thread, volatile, back-linked.
        let child = myst.store().realization_child_thread(&rid).unwrap();
        assert_eq!(child.state, ThreadState::Volatile);
        assert_eq!(child.parent_realization_id.as_deref(), Some(rid.as_str()));
        // immutability: no update path exists.
        assert!(myst
            .store()
            .try_mutate_realization(&rid, "tampered")
            .is_err());
        // SALT passage kindled.
        assert!(myst
            .store()
            .kindled()
            .unwrap()
            .contains(&"SALT".to_string()));
    }

    #[tokio::test]
    async fn open_thread_tool_lands_a_volatile_thread() {
        let store = Store::open_in_memory("dev").unwrap();
        let myst = Mystagogue::new(store_arc(store));
        let res = myst
            .dispatch(call(
                "open_thread",
                json!({ "question": "why does iron pull iron?", "domain": "magnetism" }),
            ))
            .await;
        let tid = res.value["thread_id"].as_str().unwrap();
        let thread = myst.store().get_thread(tid).unwrap();
        assert_eq!(thread.state, ThreadState::Volatile);
        assert_eq!(thread.prompt, "why does iron pull iron?");
        // domain name was upserted and linked.
        let domains = myst.store().list_domains().unwrap();
        assert_eq!(thread.domain_id, Some(domains[0].id.clone()));
        assert_eq!(domains[0].name, "magnetism");
    }

    #[tokio::test]
    async fn evaporate_thread_tool_marks_evaporated() {
        let store = Store::open_in_memory("dev").unwrap();
        let thread = store.open_thread("a dead end", None, None).unwrap();
        let myst = Mystagogue::new(store_arc(store));
        let res = myst
            .dispatch(call("evaporate_thread", json!({ "id": thread.id })))
            .await;
        assert_eq!(res.value["state"], "evaporated");
        let reloaded = myst.store().get_thread(&thread.id).unwrap();
        assert_eq!(reloaded.state, ThreadState::Evaporated);
    }

    #[tokio::test]
    async fn kindle_passage_tool_kindles_and_reports_first_wins() {
        let store = Store::open_in_memory("dev").unwrap();
        let myst = Mystagogue::new(store_arc(store));
        let first = myst
            .dispatch(call("kindle_passage", json!({ "term": "NIGREDO" })))
            .await;
        assert_eq!(first.value["kindled"], true);
        let second = myst
            .dispatch(call("kindle_passage", json!({ "term": "NIGREDO" })))
            .await;
        assert_eq!(second.value["kindled"], false, "second kindle is a no-op");
        assert!(myst
            .store()
            .kindled()
            .unwrap()
            .contains(&"NIGREDO".to_string()));
    }

    #[tokio::test]
    async fn weave_domains_tool_lands_correspondence_and_kindles_both() {
        let store = Store::open_in_memory("dev").unwrap();
        let myst = Mystagogue::new(store_arc(store));
        let res = myst
            .dispatch(call(
                "weave_domains",
                json!({ "a": "magnetism", "b": "rhetoric", "note": "both are invisible attraction" }),
            ))
            .await;
        assert!(res.value["correspondence_id"].is_string());
        let kindled = myst.store().kindled().unwrap();
        assert!(kindled.contains(&"CITRINITAS".to_string()));
        assert!(kindled.contains(&"AZOTH".to_string()));
    }

    #[tokio::test]
    async fn update_memory_tool_sets_profile_section() {
        let store = Store::open_in_memory("dev").unwrap();
        let myst = Mystagogue::new(store_arc(store));
        let res = myst
            .dispatch(call(
                "update_memory",
                json!({ "section": "how_i_learn", "content": "dialogue, proof-demanding" }),
            ))
            .await;
        assert_eq!(res.value["ok"], true);
        assert_eq!(
            myst.store().get_profile_section("how_i_learn").unwrap(),
            "dialogue, proof-demanding"
        );
    }

    #[tokio::test]
    async fn unknown_tool_returns_an_error_value_not_a_panic() {
        let store = Store::open_in_memory("dev").unwrap();
        let myst = Mystagogue::new(store_arc(store));
        let res = myst.dispatch(call("nonesuch", json!({}))).await;
        assert!(res.value["error"].is_string());
    }

    #[test]
    fn tool_specs_names_the_seven_tools() {
        let names: Vec<String> = Mystagogue::tool_specs()
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(
            names,
            vec![
                "fix_salt",
                "open_thread",
                "evaporate_thread",
                "kindle_passage",
                "weave_domains",
                "update_memory",
                "shift_mask",
            ]
        );
    }

    // ---- shift_mask (lane 13: fluid, Mystagogue-driven mask shifting) ----

    fn myst_with_mask(store: Store, mask: &str, mode: &str) -> (Mystagogue, SharedMask, String) {
        let store = store_arc(store);
        let session = store.create_session(None, mask, mode).unwrap();
        let state = mask::shared(mask, mode);
        let myst = Mystagogue::new(Arc::clone(&store))
            .with_mask_state(Arc::clone(&state), session.id.clone());
        (myst, state, session.id)
    }

    #[tokio::test]
    async fn shift_mask_moves_the_shared_cell_and_persists_the_row() {
        let (myst, state, sid) = myst_with_mask(
            Store::open_in_memory("dev").unwrap(),
            "philosophus",
            "explain",
        );
        let res = myst
            .dispatch(call(
                "shift_mask",
                json!({ "mask": "adamas", "mode": "challenge" }),
            ))
            .await;
        assert_eq!(res.value["shifted"], true, "{:?}", res.value);
        assert_eq!(mask::current(&state), ("adamas".into(), "challenge".into()));
        // persisted to the row too
        let reloaded = myst.store().get_session(&sid).unwrap();
        assert_eq!(reloaded.mask, "adamas");
        assert_eq!(reloaded.mode, "challenge");
    }

    #[tokio::test]
    async fn shift_mask_without_mode_keeps_the_current_mode() {
        let (myst, state, _) = myst_with_mask(
            Store::open_in_memory("dev").unwrap(),
            "philosophus",
            "explain",
        );
        let res = myst
            .dispatch(call("shift_mask", json!({ "mask": "solve" })))
            .await;
        assert_eq!(res.value["shifted"], true);
        assert_eq!(mask::current(&state), ("solve".into(), "explain".into()));
    }

    #[tokio::test]
    async fn shift_mask_rejects_an_unknown_mask_with_a_crisp_error() {
        let (myst, state, _) = myst_with_mask(
            Store::open_in_memory("dev").unwrap(),
            "philosophus",
            "explain",
        );
        let res = myst
            .dispatch(call("shift_mask", json!({ "mask": "sorcerer" })))
            .await;
        let err = res.value["error"].as_str().unwrap();
        assert!(err.contains("unknown mask 'sorcerer'"), "{err}");
        assert!(err.contains("philosophus") && err.contains("adamas") && err.contains("solve"));
        // unchanged
        assert_eq!(
            mask::current(&state),
            ("philosophus".into(), "explain".into())
        );
    }

    #[tokio::test]
    async fn shift_mask_no_ops_when_pinned_and_tells_the_model() {
        let (myst, state, _) = myst_with_mask(
            Store::open_in_memory("dev").unwrap(),
            "philosophus",
            "explain",
        );
        mask::pin(&state, "philosophus");
        let res = myst
            .dispatch(call("shift_mask", json!({ "mask": "adamas" })))
            .await;
        assert_eq!(res.value["shifted"], false, "{:?}", res.value);
        assert!(res.value["note"].as_str().unwrap().contains("philosophus"));
        // the pin held — no shift to adamas
        assert_eq!(
            mask::current(&state),
            ("philosophus".into(), "explain".into())
        );
    }
}
