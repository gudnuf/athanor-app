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

/// The prompt of a parent thread opened when a salt answers no nameable
/// question. Invariant #7: it names what is missing and invents no question.
pub(crate) const HONEST_GAP_PROMPT: &str = "(the question this answered went unnamed)";

/// How many past-session excerpts `recall` returns at most (bounds prompt cost
/// for a broad query).
const RECALL_MAX_HITS: usize = 5;

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

    /// Records that this session touched `thread_id` in `role`. A no-op outside
    /// a real session (bare tool-dispatch tests have no session id).
    fn link(&self, thread_id: &str, role: &str) -> Result<(), CoreError> {
        if let Some(sid) = self.session_id.as_deref() {
            self.store.link_session_thread(sid, thread_id, role)?;
        }
        Ok(())
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    /// Chooses the thread a `fix_salt` should condense, tolerating the model
    /// fumbling the reference. In order: a real, still-open uuid; a `#handle`
    /// in `thread` or `thread_id`; the session's focal thread; the ONLY thread
    /// this session opened or worked. Otherwise `None` — and the caller opens a
    /// new parent rather than guessing (a salt must never close an unrelated
    /// thread, and must never be dropped).
    fn resolve_salt_thread(&self, a: &FixSaltArgs) -> Option<String> {
        let fixable = |id: &str| -> bool {
            matches!(
                self.store.get_thread(id).map(|t| t.state),
                Ok(ThreadState::Volatile) | Ok(ThreadState::Condensing)
            )
        };
        let given = [a.thread_id.as_deref(), a.thread.as_deref()];
        for g in given
            .iter()
            .flatten()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            if fixable(g) {
                return Some(g.to_string());
            }
        }
        for g in given
            .iter()
            .flatten()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            if let Ok(Some(t)) = self.store.thread_by_handle(g) {
                if fixable(&t.id) {
                    return Some(t.id);
                }
            }
        }
        if let Some(focal) = self.focal_thread.as_deref() {
            if fixable(focal) {
                return Some(focal.to_string());
            }
        }
        if let Some(sid) = self.session_id.as_deref() {
            let mut mine: Vec<String> = Vec::new();
            for (t, role) in self.store.session_links(sid).unwrap_or_default() {
                if (role == "opened" || role == "worked") && fixable(&t) && !mine.contains(&t) {
                    mine.push(t);
                }
            }
            if mine.len() == 1 {
                return mine.pop();
            }
        }
        None
    }

    /// `fix_salt`: bind to the thread it answers, else birth an honest parent.
    fn fix_salt(&self, a: FixSaltArgs) -> Result<Value, CoreError> {
        let (realization, note) = match self.resolve_salt_thread(&a) {
            Some(id) => (
                self.store.fix_salt(
                    &id,
                    &a.realization,
                    &a.domains,
                    a.child_question.as_deref(),
                )?,
                None,
            ),
            None => {
                let answers = a
                    .answers
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty());
                let prompt = answers.unwrap_or(HONEST_GAP_PROMPT);
                let rz = self.store.fix_salt_on_new_parent(
                    prompt,
                    &a.realization,
                    &a.domains,
                    a.child_question.as_deref(),
                )?;
                let note = if answers.is_some() {
                    None
                } else {
                    let open: Vec<String> = self
                        .store
                        .open_threads()?
                        .into_iter()
                        .map(|t| crate::ids::handle_of(&t.id))
                        .collect();
                    Some(format!(
                        "fixed on a new thread; open threads: {}",
                        if open.is_empty() {
                            "(none)".to_string()
                        } else {
                            open.join(" ")
                        }
                    ))
                };
                (rz, note)
            }
        };
        self.link(&realization.thread_id, "fixed")?;
        if let Some(child) = realization.child_thread_id.as_deref() {
            self.link(child, "born")?;
        }
        let mut out = json!({
            "realization_id": realization.id,
            "child_thread_id": realization.child_thread_id,
        });
        if let Some(n) = note {
            out["note"] = json!(n);
        }
        Ok(out)
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

    /// Searches past sessions' transcripts for what the learner said — the
    /// answer to "what was I saying about X?" (the operator's "note stump":
    /// they ramble, and it must be recoverable later, or it's useless). Returns
    /// up to `RECALL_MAX_HITS` windowed excerpts, each attributed with the
    /// session's date, mask, and thread. The CURRENT session is excluded (it's
    /// already in context).
    fn recall(&self, query: &str) -> Result<Value, CoreError> {
        let hits =
            self.store
                .search_transcripts(query, self.session_id.as_deref(), RECALL_MAX_HITS)?;
        let results: Vec<Value> = hits
            .iter()
            .map(|h| {
                json!({
                    "date": crate::session::today_utc(h.created_at),
                    "mask": h.mask,
                    "thread": h.thread_prompt,
                    "excerpt": h.excerpt,
                })
            })
            .collect();
        Ok(json!({
            "query": query,
            "count": results.len(),
            "hits": results,
        }))
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
                        "thread": { "type": "string", "description": "the #handle from ripe_mercury, if this salt answers one of those threads" },
                        "answers": { "type": "string", "description": "if it answers no listed thread: the question it answers, in the learner's words" },
                        "thread_id": { "type": "string", "description": "legacy: a uuid or #handle; prefer `thread`" },
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
                        "domain": { "type": "string", "description": "the learner's domain this belongs to; reuse an active_domains name; else name it in their words" }
                    },
                    "required": ["question"]
                }),
            },
            AcpToolSpec {
                name: "bank_pull".into(),
                json_schema: json!({
                    "type": "object",
                    "description": "Keep what the learner asked you to keep (\"save that\", \"bank it\") as a pull note, in their own words. This stores the sentence; it opens no question.",
                    "properties": {
                        "text": { "type": "string", "description": "the learner's own sentence, verbatim or lightly cleaned for legibility. Never a summary." },
                        "domain": { "type": "string", "description": "the learner's domain it belongs to, if clear; reuse an active_domains name (optional)" }
                    },
                    "required": ["text"]
                }),
            },
            AcpToolSpec {
                name: "evaporate_thread".into(),
                json_schema: json!({
                    "type": "object",
                    "description": "Let a thread go (it is archived, never deleted). Name it by its #handle from ripe_mercury.",
                    "properties": {
                        "thread": { "type": "string", "description": "the #handle from ripe_mercury" },
                        "id": { "type": "string", "description": "legacy: the thread uuid; give `thread` or `id`" }
                    }
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
                    "description": "REPLACES the whole section with `content` — it does not append. Whatever the section held before is gone, so a single new line erases the earlier ones. To add to a section, send its existing lines plus the new one. To keep a sentence the learner asked you to save, use bank_pull instead. At the landing, in their words; never on the strength of silence.",
                    "properties": {
                        "section": { "type": "string" },
                        "content": { "type": "string" }
                    },
                    "required": ["section", "content"]
                }),
            },
            AcpToolSpec {
                name: "recall".into(),
                json_schema: json!({
                    "type": "object",
                    "description": "Search everything the learner has said in PAST sessions for a word or phrase, and get back their own words with when/where they said them. Reach for this the moment the learner asks 'what was I saying about…', 'remind me…', 'didn't I already…', or refers to something earlier you don't have in front of you. Their rambles are kept for exactly this — recover their actual words, then use them.",
                    "properties": {
                        "query": { "type": "string", "description": "the word or short phrase to search for across past transcripts" }
                    },
                    "required": ["query"]
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
            "fix_salt" => self.fix_salt(serde_json::from_value(args)?),
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
                self.link(&thread.id, "opened")?;
                Ok(json!({ "thread_id": thread.id }))
            }
            "evaporate_thread" => {
                let a: EvaporateArgs = serde_json::from_value(args)?;
                let given = [a.id.as_deref(), a.thread.as_deref()];
                let given: Vec<&str> = given
                    .iter()
                    .flatten()
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .collect();
                if given.is_empty() {
                    return Err(CoreError::BadState(
                        "evaporate_thread needs `thread` (a #handle) or `id`".into(),
                    ));
                }
                let mut target: Option<String> = None;
                for g in &given {
                    if self.store.get_thread(g).is_ok() {
                        target = Some((*g).to_string());
                        break;
                    }
                    if let Ok(Some(t)) = self.store.thread_by_handle(g) {
                        target = Some(t.id);
                        break;
                    }
                }
                let id = target
                    .ok_or_else(|| CoreError::NotFound(format!("thread {}", given.join(" / "))))?;
                self.store.evaporate_thread(&id)?;
                self.link(&id, "evaporated")?;
                Ok(json!({ "thread_id": id, "state": "evaporated" }))
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
            "bank_pull" => {
                let a: BankPullArgs = serde_json::from_value(args)?;
                let text = a.text.trim();
                if text.is_empty() {
                    return Err(CoreError::BadState(
                        "bank_pull needs the learner's own sentence in `text`".into(),
                    ));
                }
                let domain_id = match a.domain.as_deref() {
                    Some(name) if !name.trim().is_empty() => {
                        Some(self.store.upsert_domain(name)?.id)
                    }
                    _ => None,
                };
                let note = self.store.add_pull_note(domain_id.as_deref(), text)?;
                Ok(json!({ "id": note.id, "ok": true }))
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
            "recall" => {
                let a: RecallArgs = serde_json::from_value(args)?;
                self.recall(&a.query)
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
    /// A `#handle` from ripe_mercury, if this salt answers one.
    #[serde(default)]
    thread: Option<String>,
    /// Legacy/fumbled reference: a uuid or a `#handle` (`resolve_salt_thread`).
    #[serde(default)]
    thread_id: Option<String>,
    /// If it answers no listed thread: the question it answers, in the
    /// learner's words — a new parent is opened with this prompt.
    #[serde(default)]
    answers: Option<String>,
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
struct BankPullArgs {
    text: String,
    #[serde(default)]
    domain: Option<String>,
}

#[derive(Deserialize)]
struct EvaporateArgs {
    #[serde(default)]
    thread: Option<String>,
    #[serde(default)]
    id: Option<String>,
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

#[derive(Deserialize)]
struct RecallArgs {
    query: String,
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
    async fn fix_salt_unresolved_fixes_on_a_new_honest_gap_parent() {
        let store = Store::open_in_memory("dev").unwrap();
        let one = store.open_thread("the first open one", None, None).unwrap();
        let two = store.open_thread("a second open one", None, None).unwrap();
        // No focal thread, no handle, no `answers`: nothing resolves.
        let myst = Mystagogue::new(store_arc(store));
        let res = myst
            .dispatch(call("fix_salt", json!({ "realization": "it clicked" })))
            .await;
        assert!(res.value.get("realization_id").is_some(), "{:?}", res.value);
        let note = res.value["note"].as_str().expect("a note");
        assert!(note.contains(&crate::ids::handle_of(&one.id)), "{note}");
        assert!(note.contains(&crate::ids::handle_of(&two.id)), "{note}");
        // The realization is unchanged and sits on a NEW parent that names the gap.
        let rid = res.value["realization_id"].as_str().unwrap();
        let rz = myst.store().get_realization(rid).unwrap();
        assert_eq!(rz.text, "it clicked");
        // Invariant #6 (the spiral): the child is a real stored thread, born
        // from THIS realization, and the realization points back at it.
        let child_id = res.value["child_thread_id"]
            .as_str()
            .expect("child id, not null");
        let child = myst.store().get_thread(child_id).unwrap();
        assert_eq!(child.parent_realization_id.as_deref(), Some(rid));
        assert_eq!(rz.child_thread_id.as_deref(), Some(child_id));
        assert_eq!(
            myst.store().realization_child_thread(rid).unwrap().id,
            child_id
        );
        assert_ne!(rz.thread_id, one.id);
        assert_ne!(rz.thread_id, two.id);
        let parent = myst.store().get_thread(&rz.thread_id).unwrap();
        assert_eq!(parent.prompt, HONEST_GAP_PROMPT);
        assert_eq!(parent.state, ThreadState::Fixed);
        // Neither pre-existing thread was touched.
        for t in [&one, &two] {
            assert_eq!(
                myst.store().get_thread(&t.id).unwrap().state,
                ThreadState::Volatile
            );
        }
    }

    #[tokio::test]
    async fn fix_salt_with_no_open_threads_still_fixes_on_a_new_parent() {
        let store = Store::open_in_memory("dev").unwrap();
        let myst = Mystagogue::new(store_arc(store));
        let res = myst
            .dispatch(call(
                "fix_salt",
                json!({ "realization": "nowhere to put it" }),
            ))
            .await;
        assert!(res.value.get("error").is_none(), "{:?}", res.value);
        let rid = res.value["realization_id"].as_str().unwrap();
        let rz = myst.store().get_realization(rid).unwrap();
        assert_eq!(rz.text, "nowhere to put it");
        assert_eq!(
            myst.store().get_thread(&rz.thread_id).unwrap().prompt,
            HONEST_GAP_PROMPT
        );
    }

    #[tokio::test]
    async fn fix_salt_accepts_a_handle_in_thread_or_legacy_thread_id() {
        let store = Store::open_in_memory("dev").unwrap();
        let a = store.open_thread("thread a", None, None).unwrap();
        let b = store.open_thread("thread b", None, None).unwrap();
        let myst = Mystagogue::new(store_arc(store));
        let ha = crate::ids::handle_of(&a.id);
        let hb = crate::ids::handle_of(&b.id);
        let r1 = myst
            .dispatch(call(
                "fix_salt",
                json!({ "realization": "one", "thread": ha }),
            ))
            .await;
        let r2 = myst
            .dispatch(call(
                "fix_salt",
                json!({ "realization": "two", "thread_id": hb }),
            ))
            .await;
        assert!(r1.value.get("note").is_none(), "{:?}", r1.value);
        assert!(r2.value.get("note").is_none(), "{:?}", r2.value);
        for t in [&a, &b] {
            assert_eq!(
                myst.store().get_thread(&t.id).unwrap().state,
                ThreadState::Fixed
            );
        }
    }

    #[tokio::test]
    async fn fix_salt_with_answers_opens_a_parent_in_the_learners_question() {
        let store = Store::open_in_memory("dev").unwrap();
        let bystander = store.open_thread("unrelated", None, None).unwrap();
        let myst = Mystagogue::new(store_arc(store));
        let res = myst
            .dispatch(call(
                "fix_salt",
                json!({ "realization": "the tide waits for the moon",
                        "answers": "why does the tide wait for the moon?" }),
            ))
            .await;
        let rid = res.value["realization_id"].as_str().unwrap();
        let rz = myst.store().get_realization(rid).unwrap();
        let parent = myst.store().get_thread(&rz.thread_id).unwrap();
        assert_eq!(parent.prompt, "why does the tide wait for the moon?");
        assert_eq!(parent.state, ThreadState::Fixed);
        assert_eq!(
            myst.store().get_thread(&bystander.id).unwrap().state,
            ThreadState::Volatile
        );
    }

    #[tokio::test]
    async fn fix_salt_sets_null_parent_domain_from_first_domain_and_child_inherits() {
        let store = Store::open_in_memory("dev").unwrap();
        let parent = store.open_thread("no domain yet", None, None).unwrap();
        let myst = Mystagogue::new(store_arc(store));
        let res = myst
            .dispatch(call(
                "fix_salt",
                json!({ "realization": "r", "thread_id": parent.id,
                        "domains": ["tides", "lunar cycles"] }),
            ))
            .await;
        let rid = res.value["realization_id"].as_str().unwrap();
        let dom = myst.store().upsert_domain("tides").unwrap();
        assert_eq!(
            myst.store().get_thread(&parent.id).unwrap().domain_id,
            Some(dom.id.clone())
        );
        let child = myst.store().realization_child_thread(rid).unwrap();
        assert_eq!(
            child.domain_id,
            Some(dom.id),
            "child inherits the parent's domain"
        );
    }

    #[tokio::test]
    async fn fix_salt_never_replaces_an_existing_parent_domain() {
        let store = Store::open_in_memory("dev").unwrap();
        let d = store.upsert_domain("optics").unwrap();
        let parent = store.open_thread("placed", Some(&d.id), None).unwrap();
        let myst = Mystagogue::new(store_arc(store));
        myst.dispatch(call(
            "fix_salt",
            json!({ "realization": "r", "thread_id": parent.id, "domains": ["acoustics"] }),
        ))
        .await;
        assert_eq!(
            myst.store().get_thread(&parent.id).unwrap().domain_id,
            Some(d.id)
        );
    }

    #[tokio::test]
    async fn evaporate_thread_accepts_handle() {
        let store = Store::open_in_memory("dev").unwrap();
        let thread = store.open_thread("a dead end", None, None).unwrap();
        let myst = Mystagogue::new(store_arc(store));
        let res = myst
            .dispatch(call(
                "evaporate_thread",
                json!({ "thread": crate::ids::handle_of(&thread.id) }),
            ))
            .await;
        assert_eq!(res.value["state"], "evaporated", "{:?}", res.value);
        assert_eq!(
            myst.store().get_thread(&thread.id).unwrap().state,
            ThreadState::Evaporated
        );
        let none = myst.dispatch(call("evaporate_thread", json!({}))).await;
        assert!(none.value.get("error").is_some(), "needs id or thread");
    }

    #[tokio::test]
    async fn open_thread_in_session_then_fix_salt_binds_to_it() {
        // Resolution step 4: the only thread this session opened.
        let store = Store::open_in_memory("dev").unwrap();
        let s = store
            .create_session(None, "philosophus", "explain")
            .unwrap();
        let myst = Mystagogue::new(store_arc(store))
            .with_mask_state(crate::mask::shared("philosophus", "explain"), s.id);
        let opened = myst
            .dispatch(call("open_thread", json!({ "question": "why?" })))
            .await;
        let tid = opened.value["thread_id"].as_str().unwrap().to_string();
        let res = myst
            .dispatch(call("fix_salt", json!({ "realization": "because" })))
            .await;
        let rid = res.value["realization_id"].as_str().unwrap();
        assert_eq!(myst.store().get_realization(rid).unwrap().thread_id, tid);
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
    fn tool_specs_names_the_tools() {
        let names: Vec<String> = Mystagogue::tool_specs()
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(
            names,
            vec![
                "fix_salt",
                "open_thread",
                "bank_pull",
                "evaporate_thread",
                "kindle_passage",
                "weave_domains",
                "update_memory",
                "recall",
                "shift_mask",
            ]
        );
    }

    #[tokio::test]
    async fn recall_tool_returns_past_learner_words_excluding_current_session() {
        use crate::transcript::{format_block, TranscriptRole};
        let store = Store::open_in_memory("dev").unwrap();
        // a past, closed session where the learner used a distinctive word
        let past = store
            .create_session(None, "philosophus", "explain")
            .unwrap();
        let mut t = String::new();
        t.push_str(&format_block(
            TranscriptRole::Learner,
            "The thing I keep circling is grumblewarp — entropy as forgetting.",
        ));
        t.push_str(&format_block(TranscriptRole::Mystagogue, "Say more."));
        store.append_transcript(&past.id, &t).unwrap();
        store.mark_session_closed(&past.id).unwrap();

        // the CURRENT session (also mentions the word) must be excluded
        let current = store.create_session(None, "adamas", "challenge").unwrap();
        store
            .append_transcript(&current.id, "grumblewarp again, but this is now")
            .unwrap();
        let myst = Mystagogue::new(store_arc(store))
            .with_mask_state(mask::shared("adamas", "challenge"), current.id.clone());

        let res = myst
            .dispatch(call("recall", json!({ "query": "grumblewarp" })))
            .await;
        assert_eq!(
            res.value["count"], 1,
            "only the PAST session: {:?}",
            res.value
        );
        let hit = &res.value["hits"][0];
        assert!(hit["excerpt"].as_str().unwrap().contains("grumblewarp"));
        assert!(hit["excerpt"].as_str().unwrap().contains("LEARNER"));
        assert_eq!(hit["mask"], "philosophus");
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

    #[tokio::test]
    async fn bank_pull_stores_the_learners_words_as_a_pull_note() {
        let store = Store::open_in_memory("dev").unwrap();
        let myst = Mystagogue::new(store_arc(store));
        let res = myst
            .dispatch(call(
                "bank_pull",
                json!({ "text": "I keep wanting to know why the tide lags the moon", "domain": "tides" }),
            ))
            .await;
        assert_eq!(res.value["ok"], true, "{:?}", res.value);
        let id = res.value["id"].as_str().unwrap();
        let notes = myst.store().all_pull_notes().unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].id, id);
        assert_eq!(
            notes[0].text,
            "I keep wanting to know why the tide lags the moon"
        );
        let dom = myst.store().upsert_domain("tides").unwrap();
        assert_eq!(notes[0].domain_id, Some(dom.id));
        // No domain is fine; empty text is refused (nothing to keep).
        let bare = myst
            .dispatch(call("bank_pull", json!({ "text": "just this" })))
            .await;
        assert_eq!(bare.value["ok"], true);
        let empty = myst
            .dispatch(call("bank_pull", json!({ "text": "   " })))
            .await;
        assert!(empty.value.get("error").is_some(), "{:?}", empty.value);
        assert_eq!(myst.store().all_pull_notes().unwrap().len(), 2);
    }

    #[test]
    fn update_memory_description_says_it_replaces_the_whole_section() {
        let spec = Mystagogue::tool_specs()
            .into_iter()
            .find(|t| t.name == "update_memory")
            .unwrap();
        let d = spec.json_schema["description"]
            .as_str()
            .expect("has a description");
        assert!(d.contains("REPLACES the whole section"), "{d}");
        assert!(d.contains("bank_pull"), "points keeps at bank_pull: {d}");
    }
}
