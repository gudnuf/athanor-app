//! Prompt assembly — compose the session system prompt from the pack assets and
//! what the store knows about the learner.
//!
//! Order (the spec's **profile + ripe mercury + mode + mask**):
//! ```text
//! identity.md
//!   + condensation.md
//!   + {{profile injection}}   # the 7 knows-you placeholders, filled from the store
//!   + modes/<mode>.md
//!   + one mask file
//!   + tool-availability line
//! ```
//!
//! **Determinism is load-bearing.** `assemble` reads only stored state and the
//! compiled assets — never `now()`, never randomness — so the same store state
//! yields a byte-identical prompt. That is what makes the snapshot tests (evals
//! lane) stable: any change to a prompt asset or the layering shows up as a diff.
//!
//! Content quality of the assets is the prompt-smith lane; this module owns the
//! loader, the layering order, and the placeholder-fill.

pub mod assets;

use crate::store::Store;

/// Default minutes budgeted for a session when the caller doesn't override it
/// (core-identity §5: "assume ~15").
pub const DEFAULT_SESSION_BUDGET_MIN: u32 = 15;

/// How many ripe threads to surface in the profile injection.
const RIPE_LIMIT: usize = 3;

const SEP: &str = "\n\n---\n\n";

/// The fully-resolved plan for one session: the chosen voice (mask), kind of
/// work (mode), the focal thread, and the assembled system prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionPlan {
    pub mask: String,
    pub mode: String,
    pub thread_id: Option<String>,
    pub system_prompt: String,
}

impl Store {
    /// Recent realizations' text, newest first — fills `{{recent_salt}}`.
    /// A read-only helper (the immutable `realizations` writer, `fix_salt`, is
    /// the Task 9 extension lane; this only reads).
    fn recent_salt_texts(&self, limit: usize) -> Vec<(u64, String)> {
        let conn = self.conn();
        let mut stmt = match conn
            .prepare("SELECT date, text FROM realizations ORDER BY date DESC, id DESC LIMIT ?1")
        {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let rows = stmt.query_map([limit as i64], |r| {
            Ok((r.get::<_, u64>(0)?, r.get::<_, String>(1)?))
        });
        match rows {
            Ok(rows) => rows.filter_map(Result::ok).collect(),
            Err(_) => Vec::new(),
        }
    }
}

/// Renders the seven knows-you placeholders as a labeled injection block.
/// Each empty field renders documented not-knowing language, never a fabricated
/// history (core-identity §1: "behave as if you genuinely don't know yet").
fn profile_injection(store: &Store, focal_thread: Option<&str>, budget_min: u32) -> String {
    let learner_name = store.get_profile_section("name").unwrap_or_default();
    let how_they_learn = store.get_profile_section("how_i_learn").unwrap_or_default();

    let domains: Vec<String> = store
        .list_domains()
        .unwrap_or_default()
        .into_iter()
        .map(|d| d.name)
        .collect();

    let salt = store.recent_salt_texts(3);

    // Ripe mercury: focal thread first (if given and still ripe), then the other
    // ripe threads, deterministically ordered by ripeness (see ripe_threads).
    let ripe = store.ripe_threads(RIPE_LIMIT).unwrap_or_default();
    let mut ripe_lines: Vec<String> = Vec::new();
    if let Some(fid) = focal_thread {
        if let Ok(t) = store.get_thread(fid) {
            ripe_lines.push(format!("  - [{}] {} (focal)", t.state.as_str(), t.prompt));
        }
    }
    for t in &ripe {
        if Some(t.id.as_str()) == focal_thread {
            continue;
        }
        ripe_lines.push(format!("  - [{}] {}", t.state.as_str(), t.prompt));
    }

    let last_trace = store.last_trace().unwrap_or_default();

    let mut out = String::new();
    out.push_str("# What the furnace knows (this session's context)\n\n");
    out.push_str("These are the sections core-identity §1 reads. Empty means genuine\nnot-knowing — do not fabricate a history.\n\n");

    out.push_str("learner_name: ");
    out.push_str(if learner_name.is_empty() {
        "(not yet known — pre-initiation)"
    } else {
        &learner_name
    });
    out.push('\n');

    out.push_str("how_they_learn: ");
    out.push_str(if how_they_learn.is_empty() {
        "(not yet observed)"
    } else {
        &how_they_learn
    });
    out.push('\n');

    out.push_str("active_domains: ");
    if domains.is_empty() {
        out.push_str("(none yet)");
    } else {
        out.push_str(&domains.join(", "));
    }
    out.push('\n');

    out.push_str("recent_salt:\n");
    if salt.is_empty() {
        out.push_str("  (none yet — a young practice)\n");
    } else {
        for (_, text) in &salt {
            out.push_str("  - ");
            out.push_str(text);
            out.push('\n');
        }
    }

    out.push_str("ripe_mercury:\n");
    if ripe_lines.is_empty() {
        out.push_str("  (nothing ripe — open from a fresh vein, or begin anew)\n");
    } else {
        for line in &ripe_lines {
            out.push_str(line);
            out.push('\n');
        }
    }

    out.push_str("last_trace: ");
    match &last_trace {
        Some(t) => out.push_str(t),
        None => out.push_str("(no prior session)"),
    }
    out.push('\n');

    out.push_str(&format!("session_budget_min: {budget_min}\n"));

    out
}

fn tool_availability_line() -> String {
    format!(
        "Tools available this session: {}.",
        assets::tool_names().join(", ")
    )
}

/// Assembles the session system prompt for a `(mask, mode, thread)` against the
/// current store, with the default session budget.
///
/// Deterministic: identical store state + assets ⇒ byte-identical output.
/// Unknown mask/mode ids contribute an empty section rather than panicking, so
/// the function is total (callers select from [`assets::MASK_IDS`] /
/// [`assets::MODE_IDS`]).
pub fn assemble(mask: &str, mode: &str, thread_id: Option<&str>, store: &Store) -> SessionPlan {
    assemble_with_budget(mask, mode, thread_id, store, DEFAULT_SESSION_BUDGET_MIN)
}

/// [`assemble`] with an explicit session budget (minutes).
pub fn assemble_with_budget(
    mask: &str,
    mode: &str,
    thread_id: Option<&str>,
    store: &Store,
    budget_min: u32,
) -> SessionPlan {
    let parts: Vec<String> = vec![
        assets::IDENTITY.trim_end().to_string(),
        assets::CONDENSATION.trim_end().to_string(),
        profile_injection(store, thread_id, budget_min),
        assets::mode_asset(mode)
            .unwrap_or("")
            .trim_end()
            .to_string(),
        assets::mask_asset(mask)
            .unwrap_or("")
            .trim_end()
            .to_string(),
        tool_availability_line(),
    ];

    SessionPlan {
        mask: mask.to_string(),
        mode: mode.to_string(),
        thread_id: thread_id.map(str::to_string),
        system_prompt: parts.join(SEP),
    }
}

/// Assembles the first-launch (initiation) prompt: identity + condensation +
/// the (empty) profile injection + initiation script + tools. No mask/mode is
/// selected yet — the cold start is about the learner, not a subject
/// (initiation.md). Runs against an empty profile by design.
pub fn assemble_initiation(store: &Store) -> SessionPlan {
    let parts: Vec<String> = vec![
        assets::IDENTITY.trim_end().to_string(),
        assets::CONDENSATION.trim_end().to_string(),
        profile_injection(store, None, DEFAULT_SESSION_BUDGET_MIN),
        assets::INITIATION.trim_end().to_string(),
        tool_availability_line(),
    ];

    SessionPlan {
        mask: "initiation".to_string(),
        mode: "initiation".to_string(),
        thread_id: None,
        system_prompt: parts.join(SEP),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Store with a domain, a ripe thread, profile.how_i_learn, and a last trace.
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

    #[test]
    fn assembled_prompt_is_deterministic_and_layers_all_parts() {
        let (store, tid) = seeded_store();
        let plan = assemble("philosophus", "explain", Some(&tid), &store);
        let p = &plan.system_prompt;

        // mask asset present (philosophus signature rule)
        assert!(p.contains("Philosophus emits no declarative that asserts a domain fact"));
        // mode asset present
        assert!(p.contains("explain — make it sayable"));
        // identity + condensation spine present
        assert!(p.contains("The Mystagogue — Core Identity"));
        assert!(p.contains("The Condensation Protocol"));
        // profile injected
        assert!(p.contains("how_they_learn: dialogue-driven; demands proof"));
        assert!(p.contains("learner_name: Damian"));
        assert!(p.contains("active_domains: magnetism"));
        // ripe thread injected (focal)
        assert!(p.contains("ripe_mercury:"));
        assert!(p.contains("why does iron remember? (focal)"));
        // trace injected
        assert!(p.contains("last_trace: Traced hysteresis"));
        // budget + tools
        assert!(p.contains("session_budget_min: 15"));
        assert!(p.contains("Tools available this session: fix_salt, open_thread"));

        // plan metadata
        assert_eq!(plan.mask, "philosophus");
        assert_eq!(plan.mode, "explain");
        assert_eq!(plan.thread_id.as_deref(), Some(tid.as_str()));

        // deterministic: two assemblies of the same state are byte-identical
        let again = assemble("philosophus", "explain", Some(&tid), &store);
        assert_eq!(*p, again.system_prompt);
    }

    #[test]
    fn empty_profile_renders_not_knowing_language_no_fabrication() {
        let store = Store::open_in_memory("dev").unwrap();
        let plan = assemble("adamas", "challenge", None, &store);
        let p = &plan.system_prompt;
        assert!(p.contains("learner_name: (not yet known — pre-initiation)"));
        assert!(p.contains("how_they_learn: (not yet observed)"));
        assert!(p.contains("active_domains: (none yet)"));
        assert!(p.contains("recent_salt:\n  (none yet — a young practice)"));
        assert!(p.contains("ripe_mercury:\n  (nothing ripe"));
        assert!(p.contains("last_trace: (no prior session)"));
        // adamas + challenge assets are present
        assert!(p.contains("Adamas — the Diamond"));
        assert!(p.contains("challenge — defend or revise"));
    }

    #[test]
    fn every_mask_mode_pair_composes_its_two_assets() {
        let (store, tid) = seeded_store();
        for mask in assets::MASK_IDS {
            for mode in assets::MODE_IDS {
                let plan = assemble(mask, mode, Some(&tid), &store);
                let p = &plan.system_prompt;
                // the selected mask asset landed
                let mask_marker = match mask {
                    "philosophus" => "Philosophus — the Midwife",
                    "adamas" => "Adamas — the Diamond",
                    "solve" => "Solve — the Frame-Breaker",
                    _ => unreachable!(),
                };
                assert!(p.contains(mask_marker), "{mask}/{mode} missing mask asset");
                // exactly the selected mode's section landed
                assert!(
                    p.contains(&format!("## {mode} — ")),
                    "{mask}/{mode} missing mode section"
                );
                // no OTHER mode's headline section leaked in
                for other in assets::MODE_IDS {
                    if other != mode {
                        assert!(
                            !p.contains(&format!("## {other} — ")),
                            "{mask}/{mode} leaked mode {other}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn initiation_uses_cold_start_script_and_empty_profile() {
        let store = Store::open_in_memory("dev").unwrap();
        let plan = assemble_initiation(&store);
        let p = &plan.system_prompt;
        assert!(p.contains("Initiation — the First Session"));
        assert!(p.contains("learner_name: (not yet known — pre-initiation)"));
        // no mask/mode overlay in initiation
        assert!(!p.contains("Philosophus — the Midwife"));
        assert!(!p.contains("## trace — "));
        assert_eq!(plan.mask, "initiation");
        assert_eq!(plan.thread_id, None);
        // deterministic
        assert_eq!(*p, assemble_initiation(&store).system_prompt);
    }

    #[test]
    fn recent_salt_absent_when_no_realizations() {
        let (store, _tid) = seeded_store();
        // seeded store has no realizations yet
        let plan = assemble("solve", "design", None, &store);
        assert!(plan
            .system_prompt
            .contains("recent_salt:\n  (none yet — a young practice)"));
    }
}
