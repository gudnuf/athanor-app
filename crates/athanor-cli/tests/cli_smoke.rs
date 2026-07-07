//! Hermetic smoke test: drive a full scripted session (assemble → run turn →
//! dispatch the Mystagogue's tools → land the session) with no model and no
//! network, and assert the store state it should have produced.
//!
//! Runs in `cargo test` (no `goose` feature, no `ANTHROPIC_API_KEY`).

use std::sync::Arc;

use athanor_cli::{run_scripted_session, script::parse_script};
use athanor_core::Store;

/// Build a `Store` behind an `Arc` (Store is `Send + Sync` — its Connection
/// sits behind a reentrant mutex).
fn store_arc(store: Store) -> Arc<Store> {
    Arc::new(store)
}

#[tokio::test]
async fn scripted_session_fixes_salt_and_births_child_thread() {
    let store = store_arc(Store::open_in_memory("dev").unwrap());

    // Seed the mercury the session will condense: one domain, one ripe thread.
    let domain = store.upsert_domain("thermodynamics").unwrap();
    let parent = store
        .open_thread("does forgetting cost energy?", Some(&domain.id), None)
        .unwrap();

    // Script: the Mystagogue reflects, the learner condenses, the engine calls
    // fix_salt on the ripe thread, then the turn lands.
    let script = parse_script(&format!(
        r#"[
            {{ "text": "That thread about forgetting — say what just set." }},
            {{ "tool": "fix_salt", "id": "1", "args": {{
                "realization": "forgetting costs energy — erasure is dissipation",
                "thread_id": "{parent_id}",
                "domains": ["thermodynamics"]
            }} }},
            {{ "complete": true }}
        ]"#,
        parent_id = parent.id
    ))
    .unwrap();

    let mut out: Vec<u8> = Vec::new();
    let outcome = run_scripted_session(
        Arc::clone(&store),
        "philosophus",
        "explain",
        Some(&parent.id),
        script,
        &mut out,
    )
    .await
    .unwrap();

    // The turn landed and the engine invoked exactly fix_salt.
    assert!(outcome.landed, "TurnComplete should land the session");
    assert_eq!(outcome.tools_called, vec!["fix_salt".to_string()]);
    // Assistant text streamed through to the writer.
    assert!(String::from_utf8(out).unwrap().contains("forgetting"));

    // A landed session tends today → one wisdom day.
    assert_eq!(store.wisdom_days().unwrap(), 1);

    // The spiral: exactly one ripe thread now carries a parent realization (the
    // child born of the fixed salt); the original parent is Fixed, out of the
    // ripe pool.
    let ripe = store.ripe_threads(16).unwrap();
    let children: Vec<_> = ripe
        .iter()
        .filter(|t| t.parent_realization_id.is_some())
        .collect();
    assert_eq!(children.len(), 1, "one spiral child thread exists");

    // That child back-links a real, immutable realization.
    let rid = children[0].parent_realization_id.clone().unwrap();
    let realization = store.get_realization(&rid).unwrap();
    assert!(realization.text.contains("forgetting costs energy"));
    assert!(store.try_mutate_realization(&rid, "tampered").is_err());

    // The original parent thread left the ripe pool (it's Fixed).
    assert!(
        !ripe.iter().any(|t| t.id == parent.id),
        "fixed parent is no longer ripe"
    );
}
