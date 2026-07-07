//! Translates parsed academy markdown into store rows through the REAL store
//! APIs — `upsert_domain`, `open_thread`, `fix_salt`, `weave_domains`,
//! `record_tending`, `set_profile_section`. Never raw SQL: every invariant
//! (the spiral, thread-state DAG, immutability, wisdom-by-day) therefore holds
//! on the seeded data exactly as it would in live use.
//!
//! Historic dates come from `SeedClock`: before each dated write the clock is
//! set to that day's epoch, so realization `date`s and thread `born`s land in
//! the past and the Grimoire/Furnace read as a real history.
//!
//! Idempotent by natural keys: existing realization texts, thread prompts,
//! tending days, and correspondence notes are loaded up front and re-seeding
//! skips anything already present — running twice produces no duplicates.

use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use athanor_core::domain::ThreadState;
use athanor_core::store::Clock;
use athanor_core::{CoreError, Store};

use super::parse;

/// A settable clock: the store reads `now()` through it, and the translator
/// winds it back to each entry's date before writing so history lands in the
/// past. Wraps an `AtomicU64` shared with the closure handed to `with_clock`.
pub struct SeedClock {
    inner: Arc<AtomicU64>,
}

impl SeedClock {
    pub fn new(start: u64) -> Self {
        Self {
            inner: Arc::new(AtomicU64::new(start)),
        }
    }

    /// The `Clock` closure to hand to `Store::with_clock`.
    pub fn clock(&self) -> Clock {
        let inner = Arc::clone(&self.inner);
        Arc::new(move || inner.load(Ordering::SeqCst))
    }

    fn set(&self, epoch: u64) {
        self.inner.store(epoch, Ordering::SeqCst);
    }
}

/// Counts of what a seed run produced. Counts only — no content — so it is safe
/// to print or log.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SeedReport {
    pub domains: usize,
    pub realizations: usize,
    pub spiral_children: usize,
    pub open_threads: usize,
    pub condensing_promoted: usize,
    pub correspondences: usize,
    pub tending_days: usize,
    pub profile_sections: usize,
    /// Tabula passage keys kindled to mirror what the seeded actions would have
    /// kindled through the live tools (SALT via fix_salt; CITRINITAS/AZOTH via
    /// weave; FURNACE for a begun practice). Counts only.
    pub kindled_passages: usize,
    pub skipped: usize,
}

/// Errors the seeder can surface — a missing academy dir, a read failure, or a
/// store error bubbled up from a real API call.
#[derive(Debug)]
pub enum SeedError {
    Io(std::io::Error),
    Core(CoreError),
    Missing(String),
}

impl std::fmt::Display for SeedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SeedError::Io(e) => write!(f, "seed io: {e}"),
            SeedError::Core(e) => write!(f, "seed core: {e}"),
            SeedError::Missing(p) => write!(f, "seed: expected path not found: {p}"),
        }
    }
}

impl std::error::Error for SeedError {}

impl From<std::io::Error> for SeedError {
    fn from(e: std::io::Error) -> Self {
        SeedError::Io(e)
    }
}
impl From<CoreError> for SeedError {
    fn from(e: CoreError) -> Self {
        SeedError::Core(e)
    }
}

fn read_opt(path: &Path) -> Result<Option<String>, SeedError> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Seeds `store` from an academy directory tree (`domains/*/`, `grimoire/
/// journal.md`, `STATE.md`, `profile/learner.md`). Winds `clock` to historic
/// dates as it goes. Returns per-shape counts.
pub fn seed_from(
    store: &Store,
    clock: &SeedClock,
    academy_dir: &Path,
) -> Result<SeedReport, SeedError> {
    if !academy_dir.is_dir() {
        return Err(SeedError::Missing(academy_dir.display().to_string()));
    }
    let mut report = SeedReport::default();

    // ── existing natural keys (idempotency) ──────────────────────────────
    let existing_realization_texts: HashSet<String> = store
        .list_realizations()?
        .into_iter()
        .map(|e| e.realization.text)
        .collect();
    let mut existing_thread_prompts: HashSet<String> = store
        .open_threads()?
        .into_iter()
        .map(|t| t.prompt)
        .collect();
    let existing_tending_days: HashSet<String> = store.tending_days()?.into_iter().collect();
    let existing_correspondences: HashSet<(String, String)> = store
        .list_correspondences()?
        .into_iter()
        .map(|c| (c.domain_b, c.note))
        .collect();

    // ── domains (sulfur) ─────────────────────────────────────────────────
    let domains_dir = academy_dir.join("domains");
    let mut domain_names: Vec<String> = Vec::new();
    if domains_dir.is_dir() {
        let mut dirs: Vec<String> = std::fs::read_dir(&domains_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().to_str().map(str::to_string))
            .filter(|n| !n.starts_with('.'))
            .collect();
        dirs.sort();
        for name in dirs {
            store.upsert_domain(&name)?;
            report.domains += 1;
            domain_names.push(name);
        }
    }

    // ── journal → realizations with spiral children (salt + mercury) ─────
    if let Some(journal) = read_opt(&academy_dir.join("grimoire").join("journal.md"))? {
        for entry in parse::parse_journal(&journal) {
            // The fixed salt is the entry prose WITHOUT its authored `opens:`
            // spiral question (that becomes the child thread's prompt, not part
            // of the salt text). Bodies with no `opens:` line are unchanged.
            let salt_text = parse::strip_next_question(&entry.body);
            if salt_text.is_empty() {
                continue;
            }
            if existing_realization_texts.contains(&salt_text) {
                report.skipped += 1;
                continue;
            }
            let epoch = match parse::date_to_epoch(&entry.date) {
                Some(e) => e,
                None => {
                    report.skipped += 1;
                    continue;
                }
            };
            clock.set(epoch);

            // Parent thread: the theme being worked. Skip if one with this
            // prompt already stands open (idempotency on the parent side).
            let parent_prompt = if entry.title.is_empty() {
                format!("the work of {}", entry.date)
            } else {
                entry.title.clone()
            };
            let domains = parse::classify_domains(&entry.title, &entry.body, &domain_names);
            let domain_id = domains
                .first()
                .and_then(|n| store.upsert_domain(n).ok())
                .map(|d| d.id);

            let parent = store.open_thread(&parent_prompt, domain_id.as_deref(), None)?;
            existing_thread_prompts.insert(parent_prompt);

            // The spiral child's question: the entry's explicit `opens:` line if
            // authored, else a legacy `open:` bullet, else `fix_salt`'s default.
            let next_q = parse::entry_next_question(&entry.body);
            let legacy_q = parse::entry_open_questions(&entry.body);
            let child_question = next_q
                .as_deref()
                .or_else(|| legacy_q.first().map(String::as_str));

            store.fix_salt(&parent.id, &salt_text, &domains, child_question)?;
            report.realizations += 1;
            report.spiral_children += 1; // fix_salt always births exactly one
        }
    }

    // ── open questions → volatile threads (mercury) ──────────────────────
    let mut open_qs: Vec<String> = Vec::new();
    if let Some(state) = read_opt(&academy_dir.join("STATE.md"))? {
        open_qs.extend(parse::parse_open_questions(&state));
    }
    for q in open_qs {
        if existing_thread_prompts.contains(&q) {
            report.skipped += 1;
            continue;
        }
        clock.set(seed_now());
        store.open_thread(&q, None, None)?;
        existing_thread_prompts.insert(q);
        report.open_threads += 1;
    }

    // Promote the two oldest still-open threads to Condensing so Mercury shows
    // questions at different stages of settling, as it would in live use.
    let ripe = store.ripe_threads(2)?;
    for t in ripe {
        if t.state == ThreadState::Volatile {
            store.set_thread_state(&t.id, ThreadState::Condensing)?;
            report.condensing_promoted += 1;
        }
    }

    // ── correspondences → weave_domains (azoth) ──────────────────────────
    let mut woven: HashSet<(String, String)> = existing_correspondences;
    for name in &domain_names {
        let path = domains_dir.join(name).join("correspondences.md");
        if let Some(md) = read_opt(&path)? {
            let self_domain = store.upsert_domain(name)?;
            for link in parse::parse_correspondences(&md) {
                if link.other.trim().is_empty() {
                    continue;
                }
                let other = store.upsert_domain(&link.other)?;
                let key = (other.id.clone(), link.note.clone());
                if woven.contains(&key) {
                    report.skipped += 1;
                    continue;
                }
                store.weave_domains(&self_domain.id, &other.id, &link.note)?;
                woven.insert(key);
                report.correspondences += 1;
            }
        }
    }

    // Parity kindles for correspondences: the live `weave_domains` TOOL kindles
    // CITRINITAS (the yellowing — domains rhyming) and AZOTH (the Minister who
    // dissolves the boundary) on every weave; the raw store method the seeder
    // calls does not, so the seeder reproduces it here (kindling semantics live
    // at the tool layer, not the store — the seeder's job is to leave the db in
    // the state the live tools would have). First-wins, so once is enough. This
    // lights Tabula III (the Four Gates) and IV (the Ministers).
    if report.correspondences > 0 {
        for key in ["CITRINITAS", "AZOTH"] {
            if store.kindle_passage(key, None)? {
                report.kindled_passages += 1;
            }
        }
    }

    // ── session history → tending days (fire) ────────────────────────────
    if let Some(state) = read_opt(&academy_dir.join("STATE.md"))? {
        let mut seen_days: HashSet<String> = existing_tending_days;
        for s in parse::parse_session_days(&state) {
            if seen_days.contains(&s.day) {
                report.skipped += 1;
                continue;
            }
            let epoch = match parse::date_to_epoch(&s.day) {
                Some(e) => e,
                None => {
                    report.skipped += 1;
                    continue;
                }
            };
            clock.set(epoch);
            store.record_tending(&s.day, s.minutes, &[])?;
            seen_days.insert(s.day);
            report.tending_days += 1;
        }
    }

    // Parity kindle for the Furnace (Tabula I): a lived install with real
    // tending history has, by definition, been begun — the live app kindles
    // FURNACE at initiation close, and you cannot accumulate tended days
    // without having passed through it. Seeding that history therefore implies
    // a lit Furnace. (Only when there IS tending history — a contentless seed
    // leaves it dim.) First-wins.
    if report.tending_days > 0 && store.kindle_passage("FURNACE", None)? {
        report.kindled_passages += 1;
    }
    // SALT is already kindled by `store.fix_salt` during realization seeding
    // (lighting Tabula II + V) — no separate parity kindle needed.

    // ── profile sections ─────────────────────────────────────────────────
    clock.set(seed_now());
    report.profile_sections += seed_profile(store, academy_dir)?;

    Ok(report)
}

/// A "present" epoch for undated writes (open threads, profile). Uses the
/// real system clock so freshly opened threads sort after historic ones.
fn seed_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Maps learner-profile prose to the store's five profile sections. Idempotent
/// on its own (`set_profile_section` upserts by section).
///
/// The heading strings below ("How You Think", "The Throat", "Active Domains",
/// …) are the academy profile TEMPLATE's structural labels — document
/// *structure*, the same class as the journal's `## date — title` format or the
/// correspondences' `## To X` blocks. They are not personal substance: the
/// private prose lives only under those headings and is written solely to the
/// git-ignored db, never to this code.
fn seed_profile(store: &Store, academy_dir: &Path) -> Result<usize, SeedError> {
    let mut count = 0;
    if let Some(learner) = read_opt(&academy_dir.join("profile").join("learner.md"))? {
        let sections = extract_named_sections(&learner);
        // Each profile key accepts a few candidate headings (first match wins):
        // the academy template's own labels, plus plainer ones so a committed
        // demo persona's learner.md reads naturally (a sourdough baker has no
        // "Throat"). The heading is structure, not substance — only the body
        // prose beneath it is stored.
        let mut set = |key: &str, headers: &[&str]| -> Result<(), SeedError> {
            if let Some(body) = sections
                .iter()
                .find(|(h, _)| headers.iter().any(|w| w == h))
            {
                store.set_profile_section(key, &body.1)?;
                count += 1;
            }
            Ok(())
        };
        set("how_i_learn", &["How You Think", "How I Learn"])?;
        set("frictions", &["The Throat", "Where I Get Stuck"])?;
        set("pulls", &["Drives", "Why I Care"])?;
    }
    if let Some(state) = read_opt(&academy_dir.join("STATE.md"))? {
        let sections = extract_named_sections(&state);
        if let Some((_, body)) = sections.iter().find(|(h, _)| h == "Active Domains") {
            store.set_profile_section("domains", body)?;
            count += 1;
        }
        if let Some((_, body)) = sections.iter().find(|(h, _)| h == "Recent Sessions") {
            // working_history: keep it compact — first few lines only.
            let compact: String = body.lines().take(6).collect::<Vec<_>>().join("\n");
            store.set_profile_section("working_history", &compact)?;
            count += 1;
        }
    }
    Ok(count)
}

/// Splits markdown into (heading-title, body) pairs on `## ` headings.
fn extract_named_sections(md: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut current: Option<(String, Vec<&str>)> = None;
    for line in md.lines() {
        if line.starts_with("## ") && !line.starts_with("### ") {
            if let Some((h, body)) = current.take() {
                out.push((h, body.join("\n").trim().to_string()));
            }
            current = Some((line[3..].trim().to_string(), Vec::new()));
        } else if let Some((_, body)) = current.as_mut() {
            body.push(line);
        }
    }
    if let Some((h, body)) = current.take() {
        out.push((h, body.join("\n").trim().to_string()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn seeded_store() -> (Store, SeedClock) {
        let clock = SeedClock::new(seed_now());
        let store = Store::open_in_memory("seed-test")
            .unwrap()
            .with_clock(clock.clock());
        (store, clock)
    }

    /// A tiny synthetic academy tree — invented content, nothing personal.
    fn synthetic_academy(root: &Path) {
        fs::create_dir_all(root.join("domains/widgets")).unwrap();
        fs::create_dir_all(root.join("domains/gizmos")).unwrap();
        fs::create_dir_all(root.join("grimoire")).unwrap();
        fs::create_dir_all(root.join("profile")).unwrap();

        fs::write(
            root.join("grimoire/journal.md"),
            "# Journal\n\n## 2026-01-02 — widget shapes\n\nthe widget stood up straight.\n\nopen:\n- does the widget bend?\n\n## 2026-01-05 — gizmo turns\n\nthe gizmo turned twice.\n",
        )
        .unwrap();

        fs::write(
            root.join("domains/widgets/correspondences.md"),
            "# Correspondences\n\n## To Gizmos\n\nwidgets and gizmos rhyme.\n\n## To Sprockets (not yet a domain)\n\nsprockets are downstream.\n",
        )
        .unwrap();

        fs::write(
            root.join("STATE.md"),
            "# State\n\n## Recent Sessions\n\n- **2026-01-02** — widget session (~40min).\n- **2026-01-05** — gizmo session.\n\n## Open Threads\n\n- what turns the gizmo?\n- does the widget bend?\n\n## Active Domains\n\n- widgets — the shape\n- gizmos — the turn\n",
        )
        .unwrap();

        fs::write(
            root.join("profile/learner.md"),
            "# Learner\n\n## How You Think\n\nby feel then sharpen.\n\n## The Throat\n\nthe bottleneck.\n\n## Drives\n\ntouch reality.\n",
        )
        .unwrap();
    }

    fn tmp_root() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("athanor-seed-test-{}", athanor_core::ids::new_id()))
    }

    #[test]
    fn seed_produces_lived_in_state() {
        let root = tmp_root();
        synthetic_academy(&root);
        let (store, clock) = seeded_store();

        let report = seed_from(&store, &clock, &root).unwrap();

        // two domain dirs
        assert_eq!(report.domains, 2);
        // two journal entries -> two realizations, each with its spiral child
        assert_eq!(report.realizations, 2);
        assert_eq!(report.spiral_children, 2);
        // grimoire reads chronologically with real historic dates
        let grim = store.list_realizations().unwrap();
        assert_eq!(grim.len(), 2);
        assert_eq!(
            grim[0].realization.date,
            parse::date_to_epoch("2026-01-02").unwrap()
        );
        assert!(
            grim[0].realization.child_thread_id.is_some(),
            "spiral link present"
        );
        // wisdom counts the distinct session days
        assert_eq!(store.wisdom_days().unwrap(), 2);
        assert_eq!(report.tending_days, 2);
        // correspondences woven
        assert_eq!(report.correspondences, 2);
        // profile written
        assert!(report.profile_sections >= 4);
        assert_eq!(
            store.get_profile_section("how_i_learn").unwrap(),
            "by feel then sharpen."
        );

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn seeding_kindles_the_passages_its_actions_imply() {
        let root = tmp_root();
        synthetic_academy(&root);
        let (store, clock) = seeded_store();
        let report = seed_from(&store, &clock, &root).unwrap();

        // The fixture has realizations (→ SALT via fix_salt), correspondences
        // (→ CITRINITAS + AZOTH parity kindle), and tending days (→ FURNACE).
        assert!(report.realizations > 0 && report.correspondences > 0 && report.tending_days > 0);

        let by_key = |k: &str| {
            store
                .tabula()
                .unwrap()
                .into_iter()
                .find(|p| p.key == k)
                .unwrap()
        };
        // Lit by the seeded practice — exactly what the live tools would light.
        assert!(
            by_key("FURNACE").kindled,
            "tending history begins the Work (I)"
        );
        assert!(by_key("PRINCIPLES").kindled, "first salt fixed (II)");
        assert!(
            by_key("GATES").kindled,
            "a correspondence is the yellowing (III)"
        );
        assert!(
            by_key("MINISTERS").kindled,
            "Azoth dissolved a boundary (IV)"
        );
        assert!(by_key("GRIMOIRE").kindled, "the salt shelf filled (V)");
        // Still the Mystagogue's to light — no honest seed event kindles these.
        assert!(
            !by_key("SOURCES").kindled,
            "VI stays dim until a source is cited"
        );
        assert!(
            !by_key("WORLD").kindled,
            "VII stays dim until something is made"
        );

        // Re-seeding never re-fires kindling (first-wins).
        let second = seed_from(&store, &clock, &root).unwrap();
        assert_eq!(
            second.kindled_passages, 0,
            "kindling is first-wins on re-seed"
        );

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn normy_persona_seeds_a_full_lived_in_state_through_the_same_path() {
        let tmp = std::env::temp_dir().join(format!("normy-seed-test-{}", std::process::id()));
        crate::seed::profiles::NORMY.materialize(&tmp).unwrap();
        let (store, clock) = seeded_store();
        let report = seed_from(&store, &clock, &tmp).unwrap();

        // A real three-week practice: domains, salt, threads, tending, weaves.
        assert!(
            report.domains >= 4,
            "starter/proofing/scoring/crumb: {report:?}"
        );
        assert!(report.realizations >= 6, "the journal's bakes became salt");
        assert!(report.open_threads >= 5, "Sam's plain-language questions");
        assert!(report.tending_days >= 10, "~three weeks of sessions");
        assert!(report.correspondences >= 1, "cross-domain links");

        // No wall of default child-questions: every spiral child carries an
        // authored `opens:` question, and the salt never contains the line.
        for t in store.open_threads().unwrap() {
            assert_ne!(
                t.prompt, "what does this open?",
                "authored spiral questions"
            );
        }
        for g in store.list_realizations().unwrap() {
            assert!(
                !g.realization.text.to_lowercase().contains("opens:"),
                "the opens: line is stripped from the fixed salt"
            );
        }

        // The SAME parity kindles as the lived seed light Tabula I–V.
        let by_key = |k: &str| {
            store
                .tabula()
                .unwrap()
                .into_iter()
                .find(|p| p.key == k)
                .unwrap()
        };
        for key in ["FURNACE", "PRINCIPLES", "GATES", "MINISTERS", "GRIMOIRE"] {
            assert!(
                by_key(key).kindled,
                "{key} should be lit for a lived install"
            );
        }

        // Profile read from the plainer headings (How I Learn / Where I Get Stuck).
        assert!(!store.get_profile_section("how_i_learn").unwrap().is_empty());
        assert!(!store.get_profile_section("frictions").unwrap().is_empty());

        // THE DEMO'S PREMISE: the learner's own material is entirely plain —
        // no alchemical vocabulary anywhere in what Sam wrote. (The Mystagogue's
        // voice is untouched and lives in the prompt pack, not here.)
        let mut learner = String::new();
        for g in store.list_realizations().unwrap() {
            learner.push_str(&g.realization.text);
            learner.push(' ');
        }
        for t in store.open_threads().unwrap() {
            learner.push_str(&t.prompt);
            learner.push(' ');
        }
        learner.push_str(&store.get_profile_section("how_i_learn").unwrap());
        learner.push_str(&store.get_profile_section("frictions").unwrap());
        learner.push_str(&store.get_profile_section("pulls").unwrap());
        let lower = learner.to_lowercase();
        for tell in [
            "nigredo",
            "albedo",
            "citrinitas",
            "rubedo",
            "azoth",
            "mystagogue",
            "adamas",
            "alchem",
        ] {
            assert!(
                !lower.contains(tell),
                "learner input must stay plain — found alchemical tell '{tell}'"
            );
        }

        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn spiral_children_carry_the_open_question() {
        let root = tmp_root();
        synthetic_academy(&root);
        let (store, clock) = seeded_store();
        seed_from(&store, &clock, &root).unwrap();

        // The first entry named an explicit open question -> it becomes the
        // child thread's prompt (fix_salt's spiral).
        let grim = store.list_realizations().unwrap();
        let child = store
            .realization_child_thread(&grim[0].realization.id)
            .unwrap();
        assert_eq!(child.prompt, "does the widget bend?");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn re_seeding_is_idempotent() {
        let root = tmp_root();
        synthetic_academy(&root);
        let (store, clock) = seeded_store();

        let first = seed_from(&store, &clock, &root).unwrap();
        let realizations_after_first = store.list_realizations().unwrap().len();
        let wisdom_after_first = store.wisdom_days().unwrap();

        let second = seed_from(&store, &clock, &root).unwrap();

        // Nothing new landed the second time.
        assert_eq!(
            store.list_realizations().unwrap().len(),
            realizations_after_first
        );
        assert_eq!(store.wisdom_days().unwrap(), wisdom_after_first);
        assert_eq!(second.realizations, 0, "no new realizations on re-seed");
        assert_eq!(second.tending_days, 0, "no new tending days on re-seed");
        assert_eq!(
            second.correspondences, 0,
            "no new correspondences on re-seed"
        );
        assert!(
            second.skipped >= first.realizations,
            "re-seed skips prior work"
        );

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn open_threads_land_volatile_with_some_condensing() {
        let root = tmp_root();
        synthetic_academy(&root);
        let (store, clock) = seeded_store();
        seed_from(&store, &clock, &root).unwrap();

        let open = store.open_threads().unwrap();
        // spiral children + parsed open questions are all open (non-fixed)
        assert!(!open.is_empty());
        let condensing = open
            .iter()
            .filter(|t| t.state == ThreadState::Condensing)
            .count();
        assert!(
            condensing >= 1,
            "at least one thread condensing for variety"
        );

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn missing_academy_dir_errors() {
        let (store, clock) = seeded_store();
        let err = seed_from(&store, &clock, Path::new("/no/such/academy")).unwrap_err();
        assert!(matches!(err, SeedError::Missing(_)));
    }
}
