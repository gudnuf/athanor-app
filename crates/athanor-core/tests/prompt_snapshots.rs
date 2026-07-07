//! Assembled-prompt snapshot tests (Task 14).
//!
//! **What these lock:** the assembly *structure* — section order, the
//! separators between sections, and the two placeholder-fill sites (the
//! profile injection and the tool-availability line). **What they do NOT
//! lock:** prose. Every section that comes straight out of a `prompts/*.md`
//! asset (identity, condensation, a mode file, a mask file, the initiation
//! script) is redacted to an opaque, content-independent marker before it's
//! written to the snapshot file. That's deliberate: prompt-smith iterates
//! wording in those `.md` files continuously, and none of that should ever
//! touch these 16 checked-in files. Only a change to `assemble`'s layering
//! (Task 10, `crates/athanor-core/src/prompt/mod.rs`) — a reordered section,
//! a missing one, a changed separator, or a changed placeholder-fill format
//! — shows up as a diff here.
//!
//! Matrix: 3 masks (philosophus, adamas, solve) × 5 modes (trace, explain,
//! predict, challenge, design) = 15 session snapshots, sharing one seeded
//! store fixture (one domain, one ripe/focal thread, a fixed profile, one
//! prior trace) so every placeholder-fill field has real content to render.
//! Plus 1 initiation snapshot (cold start, empty store, no mask/mode yet).
//! 15 + 1 = 16.
//!
//! Regenerate after an intentional `assemble` structure change:
//! `UPDATE_SNAPSHOTS=1 nix develop -c cargo test -p athanor-core --test prompt_snapshots`
//! then eyeball the diffs in `tests/snapshots/` before committing, then rerun
//! without the env var to confirm green.

use athanor_core::prompt::assets::{MASK_IDS, MODE_IDS};
use athanor_core::prompt::{
    assemble_initiation_sections, assemble_sections, SECTION_INITIATION, SECTION_MASK,
    SECTION_MODE, SECTION_PROFILE_INJECTION, SECTION_TOOLS,
};
use athanor_core::store::Store;

/// One seeded store shared by all 15 mask×mode snapshots: one domain, one
/// ripe thread (used as the focal thread), a fixed profile, and a prior
/// trace — every placeholder-fill field (name, how-they-learn,
/// active_domains, recent_salt is intentionally still empty, ripe_mercury,
/// last_trace, session_budget_min) has something real to render except
/// recent_salt, which stays on its documented not-knowing text (no
/// `add_realization` call in this fixture — matches Task 10's own fixture).
fn seeded_store() -> (Store, String) {
    let store = Store::open_in_memory("dev").unwrap();
    let d = store.upsert_domain("magnetism").unwrap();
    store.set_profile_section("name", "Damian").unwrap();
    store
        .set_profile_section("how_i_learn", "dialogue-driven; demands proof")
        .unwrap();
    let thread = store
        .open_thread("why does iron remember?", Some(&d.id), None)
        .unwrap();
    let session = store
        .create_session(None, "philosophus", "explain")
        .unwrap();
    store
        .add_trace(
            &session.id,
            "Traced hysteresis; opened whether memory is energetic.",
        )
        .unwrap();
    (store, thread.id)
}

/// Redacts a prose asset's body to an opaque, content-independent marker —
/// the label carries the *structural* fact (which asset occupies this slot),
/// never the asset's wording.
fn redact(label: &str) -> String {
    format!("<<{label}: opaque prose asset, not snapshotted>>")
}

/// Rebuilds the structural view of a labeled section list (from
/// [`assemble_sections`] / [`assemble_initiation_sections`] — the same
/// layering `assemble`/`assemble_initiation` produce, kept as labeled parts
/// rather than one joined string precisely so this redaction step never has
/// to string-split on the separator, which is unsafe in general: asset
/// markdown itself contains `---` dividers that collide with it).
///
/// Every prose-asset section (identity, condensation, mode, mask,
/// initiation) is redacted; the two placeholder-fill sections (profile
/// injection, tool-availability line) are kept literal — that's the
/// surface these snapshots exist to lock.
fn structural_view(
    mask: &str,
    mode: &str,
    thread_id: Option<&str>,
    sections: &[(&'static str, String)],
) -> String {
    let mut out = String::new();
    out.push_str(&format!("mask: {mask}\n"));
    out.push_str(&format!("mode: {mode}\n"));
    out.push_str(&format!(
        "thread_id: {}\n",
        if thread_id.is_some() {
            "<present, redacted (uuid v7 is non-deterministic)>"
        } else {
            "none"
        }
    ));

    for (i, (label, body)) in sections.iter().enumerate() {
        let (header, rendered): (String, String) = match *label {
            SECTION_PROFILE_INJECTION | SECTION_TOOLS => (
                format!("{}: {label} (placeholder-fill, literal)", i + 1),
                body.clone(),
            ),
            SECTION_MODE => (
                format!("{}: {label} asset \"{mode}\" (opaque)", i + 1),
                redact(&format!("MODE {mode}")),
            ),
            SECTION_MASK => (
                format!("{}: {label} asset \"{mask}\" (opaque)", i + 1),
                redact(&format!("MASK {mask}")),
            ),
            SECTION_INITIATION => (
                format!("{}: initiation script (opaque)", i + 1),
                redact("INITIATION"),
            ),
            other => (
                format!("{}: {other} (opaque)", i + 1),
                redact(&other.to_uppercase()),
            ),
        };
        out.push_str("\n=== section ");
        out.push_str(&header);
        out.push_str(" ===\n");
        out.push_str(&rendered);
        out.push('\n');
    }
    out
}

/// Compares `actual` against the checked-in snapshot `tests/snapshots/<name>.txt`.
/// With `UPDATE_SNAPSHOTS=1` set, writes/overwrites the file instead of
/// asserting — used once to seed the 16 files, then reviewed by eye per the
/// plan's Step 3 before committing.
fn check_snapshot(name: &str, actual: &str) {
    let path = format!("{}/tests/snapshots/{name}.txt", env!("CARGO_MANIFEST_DIR"));
    if std::env::var("UPDATE_SNAPSHOTS").is_ok() {
        std::fs::write(&path, actual).expect("write snapshot");
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!("missing snapshot {path} — run with UPDATE_SNAPSHOTS=1 to create it")
    });
    assert_eq!(
        expected, actual,
        "snapshot mismatch for {name} — if this is an intentional assembly change, \
         rerun with UPDATE_SNAPSHOTS=1, eyeball the diff, then commit"
    );
}

#[test]
fn assembled_prompt_structure_matches_snapshots_for_every_mask_mode_pair() {
    let (store, tid) = seeded_store();
    for mask in MASK_IDS {
        for mode in MODE_IDS {
            let sections = assemble_sections(mask, mode, Some(&tid), &store);
            let structural = structural_view(mask, mode, Some(&tid), &sections);
            check_snapshot(&format!("{mask}__{mode}"), &structural);
        }
    }
}

#[test]
fn initiation_prompt_structure_matches_snapshot() {
    let store = Store::open_in_memory("dev").unwrap();
    let sections = assemble_initiation_sections(&store);
    let structural = structural_view("initiation", "initiation", None, &sections);
    check_snapshot("initiation", &structural);
}

/// Sanity check on the matrix itself, independent of the snapshot files:
/// 3 masks x 5 modes + 1 initiation = 16 (Task 14's Step 1 count, and the
/// review's confirmation of it).
#[test]
fn matrix_is_sixteen() {
    assert_eq!(MASK_IDS.len() * MODE_IDS.len() + 1, 16);
}
