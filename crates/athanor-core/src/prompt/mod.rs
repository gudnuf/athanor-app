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

use std::collections::HashSet;

use crate::domain::SessionNote;
use crate::store::Store;

/// Default minutes budgeted for a session when the caller doesn't override it
/// (core-identity §5: "assume ~15").
pub const DEFAULT_SESSION_BUDGET_MIN: u32 = 15;

/// How many ripe threads to surface in the profile injection.
const RIPE_LIMIT: usize = 3;

/// How many of the session's focal-thread notes to surface first (resume where
/// the last fire on this thread left off), and how many recent notes overall to
/// consider, before the char budget trims. Notes are the rich continuity that
/// replaces leaning on the one-line trace alone.
const THREAD_NOTES_LIMIT: usize = 3;
const RECENT_NOTES_LIMIT: usize = 6;
/// Total characters of session-note text the injection will spend. Sized to
/// carry a few sessions of real continuity without letting a long run of fires
/// crowd out the rest of the prompt — a hard ceiling, never a per-note cut
/// (notes are already brief by construction, so we drop whole notes, never
/// truncate one mid-sentence — the exact failure the one-line trace showed).
const NOTES_CHAR_BUDGET: usize = 1400;
/// Profile sections beyond name/how_i_learn that the condensation pass may
/// refine and the next session should therefore read back (rendered only when
/// non-empty, so an untouched profile doesn't bloat the prompt).
const EXTRA_PROFILE_SECTIONS: [&str; 3] = ["frictions", "pulls", "working_history"];

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

/// What kind of session is being assembled. `ColdRoom` joins in Release 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SessionKind {
    #[default]
    Ordinary,
    Initiation,
}

/// The cold-room reading rendered into the injection. Release 2 gives it
/// content; until then it is an empty placeholder so the context's shape is
/// settled once, shared by the clock (core ask #1) and the cold room.
#[derive(Debug, Clone, Default)]
pub struct ColdLine;

/// One context for everything `assemble` needs beyond the store. The default
/// reproduces the old `assemble` byte for byte. `now`/`tz_offset_min` carry the
/// clock in (core ask #1): assembly itself never reads a clock, so the same
/// store + ctx always yields the same prompt.
#[derive(Debug, Clone, Default)]
pub struct AssembleCtx<'a> {
    /// `None` → `DEFAULT_SESSION_BUDGET_MIN`.
    pub budget_min: Option<u32>,
    /// Epoch seconds "now"; `None` → `last_session_ago: (not supplied)`.
    pub now: Option<u64>,
    /// The device's UTC offset in minutes; `None` → "(not supplied)".
    pub tz_offset_min: Option<i32>,
    /// Release 2 (cold room); `None` → the line is omitted.
    pub cold: Option<&'a ColdLine>,
    pub kind: SessionKind,
}

impl AssembleCtx<'_> {
    fn budget(&self) -> u32 {
        self.budget_min.unwrap_or(DEFAULT_SESSION_BUDGET_MIN)
    }
}

/// Renders the seven knows-you placeholders as a labeled injection block.
/// Each empty field renders documented not-knowing language, never a fabricated
/// history (core-identity §1: "behave as if you genuinely don't know yet").
fn profile_injection(store: &Store, focal_thread: Option<&str>, ctx: &AssembleCtx) -> String {
    let budget_min = ctx.budget();
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
    // Each line leads with the thread's #handle (the model's only way to name a
    // pre-existing thread) and ends with its domain, or "no domain".
    let domain_names: std::collections::HashMap<String, String> = store
        .list_domains()
        .unwrap_or_default()
        .into_iter()
        .map(|d| (d.id, d.name))
        .collect();
    let ripe_line = |t: &crate::domain::Thread, focal: bool| -> String {
        let domain = t
            .domain_id
            .as_deref()
            .and_then(|id| domain_names.get(id))
            .map(|n| format!("domain: {n}"))
            .unwrap_or_else(|| "no domain".to_string());
        format!(
            "  - {} [{}] {}{} ({domain})",
            crate::ids::handle_of(&t.id),
            t.state.as_str(),
            t.prompt,
            if focal { " (focal)" } else { "" }
        )
    };
    let mut ripe_lines: Vec<String> = Vec::new();
    if let Some(fid) = focal_thread {
        if let Ok(t) = store.get_thread(fid) {
            ripe_lines.push(ripe_line(&t, true));
        }
    }
    for t in &ripe {
        if Some(t.id.as_str()) == focal_thread {
            continue;
        }
        ripe_lines.push(ripe_line(t, false));
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

    // Extra profile sections the condensation pass refines (frictions/pulls/
    // working_history) — rendered only when they carry something, so an
    // untouched profile reads exactly as before.
    for section in EXTRA_PROFILE_SECTIONS {
        let content = store.get_profile_section(section).unwrap_or_default();
        let content = content.trim();
        if !content.is_empty() {
            out.push_str(section);
            out.push_str(": ");
            out.push_str(&content.replace('\n', "; "));
            out.push('\n');
        }
    }

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

    // recent_notes: the rich condensation residue from past sessions — what
    // actually moved, in a few sentences, so "what did I say about X?" is
    // answerable next time (the one-line trace above is now a fallback, not the
    // only memory). Focal thread's own notes first (resume where it left off),
    // then the most recent notes overall, deduped, trimmed to a char budget by
    // dropping whole notes (never truncating one).
    out.push_str("recent_notes:\n");
    let notes = collect_recent_notes(store, focal_thread);
    if notes.is_empty() {
        out.push_str("  (none yet — nothing has condensed)\n");
    } else {
        let mut used = 0usize;
        for n in &notes {
            if used > 0 && used + n.note.len() > NOTES_CHAR_BUDGET {
                break;
            }
            used += n.note.len();
            let day = crate::session::today_utc(n.created_at);
            let one_line = n.note.split_whitespace().collect::<Vec<_>>().join(" ");
            out.push_str(&format!("  - [{day} · {}] {}\n", n.mask, one_line));
        }
    }

    out.push_str(&format!("session_budget_min: {budget_min}\n"));

    out
}

/// Gathers the session notes for `profile_injection`: the focal thread's own
/// most-recent notes first (so a threaded session resumes where the last fire
/// on it left off), then the most recent notes overall, deduped by session so
/// no note appears twice. Deterministic given store state.
fn collect_recent_notes(store: &Store, focal_thread: Option<&str>) -> Vec<SessionNote> {
    let mut notes: Vec<SessionNote> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    if let Some(fid) = focal_thread {
        for n in store
            .thread_session_notes(fid, THREAD_NOTES_LIMIT)
            .unwrap_or_default()
        {
            if seen.insert(n.session_id.clone()) {
                notes.push(n);
            }
        }
    }
    for n in store
        .recent_session_notes(RECENT_NOTES_LIMIT)
        .unwrap_or_default()
    {
        if seen.insert(n.session_id.clone()) {
            notes.push(n);
        }
    }
    notes
}

fn tool_availability_line() -> String {
    format!(
        "Tools available this session: {}.",
        assets::tool_names().join(", ")
    )
}

/// A named section of an assembled prompt, in emission order — the label is
/// a stable structural tag (never the section's prose), so callers (the
/// Task 14 snapshot suite) can redact prose sections by label without
/// string-splitting the joined `system_prompt` on [`SEP`] — which is not
/// safe in general, since asset markdown itself contains `---` dividers
/// that collide with the separator text.
pub const SECTION_IDENTITY: &str = "identity";
pub const SECTION_CONDENSATION: &str = "condensation";
pub const SECTION_PROFILE_INJECTION: &str = "profile_injection";
pub const SECTION_MODE: &str = "mode";
pub const SECTION_MASK: &str = "mask";
pub const SECTION_INITIATION: &str = "initiation";
pub const SECTION_TOOLS: &str = "tools";

/// Builds the labeled sections for a session prompt (`assemble`'s layering),
/// without joining them. The single source of truth both `assemble_with_budget`
/// and the snapshot suite build on.
fn session_sections(
    mask: &str,
    mode: &str,
    thread_id: Option<&str>,
    store: &Store,
    ctx: &AssembleCtx,
) -> Vec<(&'static str, String)> {
    vec![
        (SECTION_IDENTITY, assets::IDENTITY.trim_end().to_string()),
        (
            SECTION_CONDENSATION,
            assets::CONDENSATION.trim_end().to_string(),
        ),
        (
            SECTION_PROFILE_INJECTION,
            profile_injection(store, thread_id, ctx),
        ),
        (
            SECTION_MODE,
            assets::mode_asset(mode)
                .unwrap_or("")
                .trim_end()
                .to_string(),
        ),
        (
            SECTION_MASK,
            assets::mask_asset(mask)
                .unwrap_or("")
                .trim_end()
                .to_string(),
        ),
        (SECTION_TOOLS, tool_availability_line()),
    ]
}

/// Builds the labeled sections for the initiation prompt (`assemble_initiation`'s
/// layering), without joining them.
fn initiation_sections(store: &Store, ctx: &AssembleCtx) -> Vec<(&'static str, String)> {
    vec![
        (SECTION_IDENTITY, assets::IDENTITY.trim_end().to_string()),
        (
            SECTION_CONDENSATION,
            assets::CONDENSATION.trim_end().to_string(),
        ),
        (
            SECTION_PROFILE_INJECTION,
            profile_injection(store, None, ctx),
        ),
        (
            SECTION_INITIATION,
            assets::INITIATION.trim_end().to_string(),
        ),
        (SECTION_TOOLS, tool_availability_line()),
    ]
}

/// Assembles the session system prompt for a `(mask, mode, thread)` against the
/// current store, with the default session budget.
///
/// Deterministic: identical store state + assets ⇒ byte-identical output.
/// Unknown mask/mode ids contribute an empty section rather than panicking, so
/// the function is total (callers select from [`assets::MASK_IDS`] /
/// [`assets::MODE_IDS`]).
pub fn assemble(mask: &str, mode: &str, thread_id: Option<&str>, store: &Store) -> SessionPlan {
    assemble_ctx(mask, mode, thread_id, store, &AssembleCtx::default())
}

/// [`assemble`] with an explicit session budget (minutes).
pub fn assemble_with_budget(
    mask: &str,
    mode: &str,
    thread_id: Option<&str>,
    store: &Store,
    budget_min: u32,
) -> SessionPlan {
    let ctx = AssembleCtx {
        budget_min: Some(budget_min),
        ..AssembleCtx::default()
    };
    assemble_ctx(mask, mode, thread_id, store, &ctx)
}

/// The one assemble entry point: [`assemble`] and [`assemble_with_budget`] are
/// wrappers over it.
pub fn assemble_ctx(
    mask: &str,
    mode: &str,
    thread_id: Option<&str>,
    store: &Store,
    ctx: &AssembleCtx,
) -> SessionPlan {
    let sections = session_sections(mask, mode, thread_id, store, ctx);
    let system_prompt = sections
        .into_iter()
        .map(|(_, body)| body)
        .collect::<Vec<_>>()
        .join(SEP);

    SessionPlan {
        mask: mask.to_string(),
        mode: mode.to_string(),
        thread_id: thread_id.map(str::to_string),
        system_prompt,
    }
}

/// Assembles the first-launch (initiation) prompt: identity + condensation +
/// the (empty) profile injection + initiation script + tools. No mask/mode is
/// selected yet — the cold start is about the learner, not a subject
/// (initiation.md). Runs against an empty profile by design.
pub fn assemble_initiation(store: &Store) -> SessionPlan {
    let ctx = AssembleCtx {
        kind: SessionKind::Initiation,
        ..AssembleCtx::default()
    };
    let sections = initiation_sections(store, &ctx);
    let system_prompt = sections
        .into_iter()
        .map(|(_, body)| body)
        .collect::<Vec<_>>()
        .join(SEP);

    SessionPlan {
        mask: "initiation".to_string(),
        mode: "initiation".to_string(),
        thread_id: None,
        system_prompt,
    }
}

/// Test/eval support (Task 14 snapshot suite): the same layering `assemble`
/// uses, but as labeled sections instead of one joined string — so a
/// consumer can redact prose sections by structural label rather than
/// string-splitting on [`SEP`] (unsafe in general; see [`SECTION_IDENTITY`]
/// et al.). Not `#[cfg(test)]` because the snapshot suite lives in a
/// separate integration-test crate (`tests/prompt_snapshots.rs`), which only
/// sees `pub` items.
pub fn assemble_sections(
    mask: &str,
    mode: &str,
    thread_id: Option<&str>,
    store: &Store,
) -> Vec<(&'static str, String)> {
    session_sections(mask, mode, thread_id, store, &AssembleCtx::default())
}

/// [`assemble_sections`] for the initiation prompt.
pub fn assemble_initiation_sections(store: &Store) -> Vec<(&'static str, String)> {
    initiation_sections(
        store,
        &AssembleCtx {
            kind: SessionKind::Initiation,
            ..AssembleCtx::default()
        },
    )
}

/// Assembles the close-only distillation prompt (`Conductor::condense`):
/// identity (so the voice holds) + the profile injection (so the model refines
/// what's known rather than repeating it) + the condense instructions. Returns
/// just the system prompt — the dialogue itself is fed as the turn history.
pub fn assemble_condensation(store: &Store, thread_id: Option<&str>) -> String {
    [
        assets::IDENTITY.trim_end().to_string(),
        profile_injection(store, thread_id, &AssembleCtx::default()),
        assets::CONDENSE.trim_end().to_string(),
    ]
    .join(SEP)
}

/// The parsed result of a condensation turn: the durable session note plus any
/// `(section, addition)` profile refinements to MERGE (never clobber).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Condensation {
    pub note: Option<String>,
    pub profile_updates: Vec<(String, String)>,
    /// `#handle`s from `WORKED:` lines — threads the session worked. Only
    /// well-formed handles (`#` + six lowercase hex) are kept.
    pub worked: Vec<String>,
    /// `(#handle, name)` from `DOMAIN #handle: name` lines.
    pub domains: Vec<(String, String)>,
}

/// `#` + exactly six lowercase hex digits, else `None`.
fn well_formed_handle(tok: &str) -> Option<String> {
    let h = tok.strip_prefix('#')?;
    (h.len() == 6
        && h.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
    .then(|| tok.to_string())
}

/// The profile sections a condensation is allowed to refine (the ones the
/// prompt names). An unrecognized `PROFILE <x>:` block is ignored rather than
/// inventing a junk section.
const CONDENSE_PROFILE_SECTIONS: [&str; 6] = [
    "how_i_learn",
    "frictions",
    "domains",
    "pulls",
    "working_history",
    "name",
];

/// Parses a condensation turn's plain-text output (see `prompts/condense.md`).
/// Liberal by design: recognizes `NOTE:` and `PROFILE <section>:` labels
/// (case-insensitive), lets a block run across continuation lines, and — if the
/// model emitted no `NOTE:` label at all but did say something — falls back to
/// treating the whole reply as the note. A `PROFILE` block whose value is empty
/// or is just the instructional parenthetical (`(only if …)`) is dropped.
pub fn parse_condensation(text: &str) -> Condensation {
    #[derive(Clone)]
    enum Target {
        None,
        Note,
        Profile(String),
    }

    let mut note_lines: Vec<String> = Vec::new();
    let mut profiles: Vec<(String, Vec<String>)> = Vec::new();
    let mut target = Target::None;
    let mut saw_note_label = false;

    let strip_label = |line: &str, label: &str| -> Option<String> {
        let trimmed = line.trim_start();
        let lower = trimmed.to_ascii_lowercase();
        if lower.starts_with(label) {
            Some(trimmed[label.len()..].trim_start().to_string())
        } else {
            None
        }
    };

    let mut worked: Vec<String> = Vec::new();
    let mut domains: Vec<(String, String)> = Vec::new();

    for line in text.lines() {
        // `WORKED: #handle[, #handle…]` and `DOMAIN #handle: name` end whatever
        // block was open (they are labels, not note continuation).
        if let Some(rest) = strip_label(line, "worked:") {
            target = Target::None;
            for tok in rest.split(|c: char| c.is_whitespace() || c == ',') {
                if let Some(h) = well_formed_handle(tok) {
                    if !worked.contains(&h) {
                        worked.push(h);
                    }
                }
            }
            continue;
        }
        if let Some(rest) = strip_label(line, "domain ") {
            if let Some((h, name)) = rest.split_once(':') {
                target = Target::None;
                if let Some(h) = well_formed_handle(h.trim()) {
                    let name = name.trim();
                    if !name.is_empty() {
                        domains.push((h, name.to_string()));
                    }
                }
                continue;
            }
        }
        if let Some(rest) = strip_label(line, "note:") {
            saw_note_label = true;
            target = Target::Note;
            if !rest.trim().is_empty() {
                note_lines.push(rest);
            }
            continue;
        }
        // `PROFILE <section>: value`
        let trimmed = line.trim_start();
        if trimmed.to_ascii_lowercase().starts_with("profile ") {
            if let Some(colon) = trimmed.find(':') {
                let section = trimmed[7..colon].trim().to_ascii_lowercase();
                let value = trimmed[colon + 1..].trim().to_string();
                if CONDENSE_PROFILE_SECTIONS.contains(&section.as_str()) {
                    profiles.push((
                        section,
                        if value.is_empty() {
                            vec![]
                        } else {
                            vec![value]
                        },
                    ));
                    target = Target::Profile(profiles.last().unwrap().0.clone());
                } else {
                    target = Target::None;
                }
                continue;
            }
        }
        // A continuation line for whatever block we're in.
        match &target {
            Target::Note => note_lines.push(line.to_string()),
            Target::Profile(section) => {
                if let Some((_, lines)) = profiles.iter_mut().rev().find(|(s, _)| s == section) {
                    lines.push(line.to_string());
                }
            }
            Target::None => {}
        }
    }

    let join_clean = |lines: &[String]| -> String { lines.join("\n").trim().to_string() };

    // A profile value that's empty or an instructional parenthetical is dropped.
    let is_placeholder = |v: &str| v.is_empty() || v.starts_with('(');

    let profile_updates: Vec<(String, String)> = profiles
        .iter()
        .filter_map(|(section, lines)| {
            let value = join_clean(lines);
            if is_placeholder(&value) {
                None
            } else {
                Some((section.clone(), value))
            }
        })
        .collect();

    let note = if saw_note_label {
        let n = join_clean(&note_lines);
        if n.is_empty() {
            None
        } else {
            Some(n)
        }
    } else {
        // No NOTE: label — liberal fallback: the whole reply IS the note, unless
        // it was only profile lines / empty.
        let whole = text.trim();
        if whole.is_empty() || !profile_updates.is_empty() {
            None
        } else {
            Some(whole.to_string())
        }
    };

    Condensation {
        note,
        profile_updates,
        worked,
        domains,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assemble_ctx_default_is_byte_identical_to_assemble() {
        let store = Store::open_in_memory("d").unwrap();
        store
            .open_thread("why does ice float?", None, None)
            .unwrap();
        let a = assemble("philosophus", "explain", None, &store);
        let b = assemble_ctx(
            "philosophus",
            "explain",
            None,
            &store,
            &AssembleCtx::default(),
        );
        assert_eq!(a.system_prompt, b.system_prompt);
        let c = assemble_with_budget("adamas", "trace", None, &store, 30);
        let d = assemble_ctx(
            "adamas",
            "trace",
            None,
            &store,
            &AssembleCtx {
                budget_min: Some(30),
                ..AssembleCtx::default()
            },
        );
        assert_eq!(c.system_prompt, d.system_prompt);
    }

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
        assert!(p.contains("why does iron remember? (focal) (domain: magnetism)"));
        assert!(p.contains(&format!("- {} [volatile]", crate::ids::handle_of(&tid))));
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
    fn parse_condensation_extracts_note_and_warranted_profiles() {
        let out = "NOTE: The learner circled whether forgetting costs energy, then \
                   named erasure as dissipation in their own words.\n\
                   PROFILE how_i_learn: reaches conviction by restating in their own terms\n\
                   PROFILE frictions: (only if a real recurring friction surfaced)";
        let c = parse_condensation(out);
        assert!(c.note.unwrap().contains("erasure as dissipation"));
        // the placeholder-parenthetical frictions line is dropped; only the real
        // how_i_learn refinement survives.
        assert_eq!(
            c.profile_updates,
            vec![(
                "how_i_learn".to_string(),
                "reaches conviction by restating in their own terms".to_string()
            )]
        );
    }

    #[test]
    fn parse_condensation_multiline_note() {
        let out = "NOTE: first sentence.\nsecond sentence that wrapped.\n";
        let c = parse_condensation(out);
        assert_eq!(
            c.note.unwrap(),
            "first sentence.\nsecond sentence that wrapped."
        );
    }

    #[test]
    fn parse_condensation_falls_back_to_whole_text_without_labels() {
        let c = parse_condensation("just a plain distillation with no labels at all");
        assert_eq!(
            c.note.unwrap(),
            "just a plain distillation with no labels at all"
        );
        assert!(c.profile_updates.is_empty());
    }

    #[test]
    fn parse_condensation_empty_yields_nothing() {
        let c = parse_condensation("   \n  ");
        assert_eq!(c, Condensation::default());
    }

    #[test]
    fn parse_condensation_ignores_unknown_profile_section() {
        let out = "NOTE: something moved.\nPROFILE mood: cranky";
        let c = parse_condensation(out);
        assert_eq!(c.note.unwrap(), "something moved.");
        assert!(c.profile_updates.is_empty(), "unknown section dropped");
    }

    #[test]
    fn assembled_prompt_reads_back_recent_session_notes() {
        // The item-7 fix: a condensed note from a PAST session must appear in a
        // NEW session's assembled prompt so "what did I say about X?" is
        // answerable (continuity, not just the one-line trace).
        let store = Store::open_in_memory("dev").unwrap();
        let thread = store
            .open_thread("does forgetting cost energy?", None, None)
            .unwrap();
        let past = store
            .create_session(Some(&thread.id), "philosophus", "explain")
            .unwrap();
        store
            .add_session_note(
                &past.id,
                Some(&thread.id),
                "The learner named erasure as dissipation in their own words.",
            )
            .unwrap();

        let p = assemble("adamas", "challenge", Some(&thread.id), &store).system_prompt;
        assert!(p.contains("recent_notes:"), "notes section present");
        assert!(
            p.contains("erasure as dissipation"),
            "the past note's text is read back into the new prompt:\n{p}"
        );
    }

    #[test]
    fn assembled_prompt_reads_back_condense_refined_profile_sections() {
        let store = Store::open_in_memory("dev").unwrap();
        store
            .set_profile_section("frictions", "avoids committing to a claim until cornered")
            .unwrap();
        let p = assemble("adamas", "challenge", None, &store).system_prompt;
        assert!(
            p.contains("frictions: avoids committing to a claim until cornered"),
            "a condense-refined profile section is read back:\n{p}"
        );
    }

    #[test]
    fn empty_store_still_renders_notes_not_knowing_language() {
        let store = Store::open_in_memory("dev").unwrap();
        let p = assemble("solve", "design", None, &store).system_prompt;
        assert!(p.contains("recent_notes:\n  (none yet — nothing has condensed)"));
        // an untouched profile does NOT render the extra sections
        assert!(!p.contains("frictions:"));
        assert!(!p.contains("pulls:"));
    }

    #[test]
    fn assemble_condensation_layers_identity_profile_and_instructions() {
        let (store, tid) = seeded_store();
        let sys = assemble_condensation(&store, Some(&tid));
        assert!(sys.contains("The Mystagogue — Core Identity"));
        assert!(sys.contains("Closing the session — distill what remains"));
        assert!(sys.contains("how_they_learn: dialogue-driven; demands proof"));
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

    #[test]
    fn ripe_mercury_lines_carry_handle_and_domain() {
        let store = Store::open_in_memory("d").unwrap();
        let d = store.upsert_domain("optics").unwrap();
        let a = store
            .open_thread("why is the sky dark at night?", Some(&d.id), None)
            .unwrap();
        let b = store
            .open_thread("an unplaced thought?", None, None)
            .unwrap();
        let p = assemble("philosophus", "explain", None, &store).system_prompt;
        let ha = crate::ids::handle_of(&a.id);
        let hb = crate::ids::handle_of(&b.id);
        assert!(
            p.contains(&format!(
                "  - {ha} [volatile] why is the sky dark at night? (domain: optics)"
            )),
            "{p}"
        );
        assert!(
            p.contains(&format!(
                "  - {hb} [volatile] an unplaced thought? (no domain)"
            )),
            "{p}"
        );
    }

    #[test]
    fn parse_condensation_reads_worked_and_domain_lines() {
        let c = parse_condensation(
            "NOTE: Circled the grain question.\nstill the note\nWORKED: #a3f9c2\nWORKED: #0b1c2d, #ffffff\nDOMAIN #a3f9c2: woodworking\nPROFILE frictions: loses the thread when rushed",
        );
        assert_eq!(
            c.note.as_deref(),
            Some("Circled the grain question.\nstill the note")
        );
        assert_eq!(c.worked, vec!["#a3f9c2", "#0b1c2d", "#ffffff"]);
        assert_eq!(
            c.domains,
            vec![("#a3f9c2".to_string(), "woodworking".to_string())]
        );
        assert_eq!(c.profile_updates.len(), 1);
    }

    #[test]
    fn parse_condensation_ignores_malformed_handles() {
        let c = parse_condensation(
            "NOTE: n\nWORKED: #abc\nWORKED: a3f9c2\nWORKED: #A3F9C2\nWORKED: #a3f9c2zz\nWORKED: #g3f9c2\nDOMAIN #xyz: nope\nDOMAIN #a3f9c2:\nDOMAIN a3f9c2: bare",
        );
        assert!(c.worked.is_empty(), "{:?}", c.worked);
        assert!(c.domains.is_empty(), "{:?}", c.domains);
        assert_eq!(c.note.as_deref(), Some("n"));
    }
}
