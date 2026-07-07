//! The Mystagogue extension: the six tools the model wields over the `Store`.
//!
//! `Mystagogue` is a `ToolDispatch` impl (the engine seam, Task 8) whose six
//! tools each write to the tria prima store:
//!
//! | tool              | store effect                                            |
//! |-------------------|---------------------------------------------------------|
//! | `fix_salt`        | immutable realization + auto-born child thread; SALT    |
//! | `open_thread`     | a new volatile thread (open question)                   |
//! | `evaporate_thread`| mark a thread evaporated                                 |
//! | `kindle_passage`  | kindle a Tabula passage (first-wins)                    |
//! | `weave_domains`   | a correspondence; kindle CITRINITAS + AZOTH             |
//! | `update_memory`   | set a learner-profile section                           |
//!
//! The spiral invariant lives in `Store::fix_salt` (one transaction: the
//! realization AND its child thread, or neither). The model speaks in domain
//! NAMES; the tools upsert names → ids at the boundary.

mod acp;

pub use acp::{AcpToolCall, AcpToolResult, AcpToolSpec, ToolDispatch};

use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::CoreError;
use crate::store::Store;

pub struct Mystagogue {
    store: Arc<Store>,
}

impl Mystagogue {
    pub fn new(store: Arc<Store>) -> Self {
        Self { store }
    }

    pub fn store(&self) -> &Store {
        &self.store
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
                        "realization": { "type": "string", "description": "the immutable insight, once fixed" },
                        "thread_id": { "type": "string", "description": "the thread this realization closes" },
                        "domains": { "type": "array", "items": { "type": "string" }, "description": "domain names this realization touches" },
                        "child_question": { "type": "string", "description": "the next question this opens (optional; one is synthesized if absent)" }
                    },
                    "required": ["realization", "thread_id"]
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
        ]
    }

    /// Runs a tool by name over the store, returning the JSON value the engine
    /// hands back. Errors surface as values (`dispatch` never fails the turn).
    fn run(&self, name: &str, args: Value) -> Result<Value, CoreError> {
        match name {
            "fix_salt" => {
                let a: FixSaltArgs = serde_json::from_value(args)?;
                let realization = self.store.fix_salt(
                    &a.thread_id,
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
            other => Err(CoreError::BadState(format!("unknown tool: {other}"))),
        }
    }
}

#[async_trait::async_trait(?Send)]
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
    thread_id: String,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ThreadState;

    // `Arc<Store>` is intentional per the plan's `Mystagogue::new` signature;
    // `Store` is `!Sync` (single rusqlite `Connection`) and the dispatcher is
    // single-threaded — see the INTEGRATION NOTE in acp.rs.
    #[allow(clippy::arc_with_non_send_sync)]
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
    fn tool_specs_names_the_six_tools() {
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
            ]
        );
    }
}
