//! Vault export (v1): the whole store as one deterministic markdown document.
//! Read-only. Every record is `### heading` + a `---` frontmatter block in a
//! fixed key order + an optional verbatim body. Strings are JSON-encoded
//! (valid YAML double-quoted scalars); timestamps are ISO-8601 UTC.

use std::collections::HashMap;
use std::fmt::Write;

use crate::error::CoreError;
use crate::session::civil_from_days;
use crate::transcript::{self, TranscriptRole};

use super::Store;

/// `YYYY-MM-DDTHH:MM:SSZ` for epoch seconds (UTC).
pub(crate) fn iso_utc(epoch_secs: u64) -> String {
    let (y, m, d) = civil_from_days((epoch_secs / 86_400) as i64);
    let rem = epoch_secs % 86_400;
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// A string as a JSON (hence YAML double-quoted) scalar.
fn q(s: &str) -> String {
    serde_json::to_string(s).expect("string serializes")
}

fn q_opt(s: Option<&str>) -> String {
    s.map(q).unwrap_or_else(|| "null".to_string())
}

fn q_list(v: &[String]) -> String {
    serde_json::to_string(v).expect("list serializes")
}

fn ts(t: u64) -> String {
    q(&iso_utc(t))
}

fn ts_opt(t: Option<u64>) -> String {
    t.map(ts).unwrap_or_else(|| "null".to_string())
}

/// One record: heading, frontmatter in the given order, then the body (omitted
/// when empty after trimming) followed by a blank line.
fn record(out: &mut String, heading: &str, fm: &[(&str, String)], body: Option<&str>) {
    let _ = writeln!(out, "### {heading}\n\n---");
    for (k, v) in fm {
        let _ = writeln!(out, "{k}: {v}");
    }
    out.push_str("---\n\n");
    if let Some(b) = body {
        if !b.trim().is_empty() {
            let _ = write!(out, "{}\n\n", b.trim_end());
        }
    }
}

fn section<T>(out: &mut String, title: &str, items: &[T], mut each: impl FnMut(&mut String, &T)) {
    let _ = write!(out, "## {title}\n\n");
    if items.is_empty() {
        out.push_str("_none_\n\n");
    }
    for item in items {
        each(out, item);
    }
}

impl Store {
    /// Writes a consistent copy of the whole database to `dest` (must not exist)
    /// via `VACUUM INTO`: a transactional snapshot, no WAL/sidecar handling.
    /// Works on a read-only connection.
    pub fn export_database_copy(&self, dest: impl AsRef<std::path::Path>) -> Result<(), CoreError> {
        let dest = dest.as_ref();
        if dest.exists() {
            return Err(CoreError::BadState(format!(
                "export destination {} already exists",
                dest.display()
            )));
        }
        self.conn()
            .execute("VACUUM INTO ?1", rusqlite::params![dest.to_string_lossy()])?;
        Ok(())
    }

    /// The whole store as one deterministic markdown document (vault format v1).
    /// Read-only. Stable ordering; free text verbatim. Uses `self.now()` for
    /// `exported_at` (so an injected clock makes it byte-for-byte reproducible).
    pub fn export_vault(&self) -> Result<String, CoreError> {
        let schema_version: i64 = self
            .conn()
            .pragma_query_value(None, "user_version", |r| r.get(0))?;
        let profile = self.all_profile_sections()?;
        let domains = self.all_domains()?;
        let pulls = self.all_pull_notes()?;
        let threads = self.all_threads()?;
        let realizations = self.list_realizations()?;
        let correspondences = self.list_correspondences()?;
        let sessions = self.all_sessions()?;
        let tending = self.all_tending()?;
        let kindling = self.all_kindling()?;

        let domain_name: HashMap<&str, &str> = domains
            .iter()
            .map(|d| (d.id.as_str(), d.name.as_str()))
            .collect();
        let name_of =
            |id: Option<&str>| -> Option<&str> { id.and_then(|i| domain_name.get(i).copied()) };
        let thread_prompt: HashMap<&str, &str> = threads
            .iter()
            .map(|t| (t.id.as_str(), t.prompt.as_str()))
            .collect();
        // realization id -> the thread it fixed (the parent of its child thread)
        let rz_thread: HashMap<&str, &str> = realizations
            .iter()
            .map(|e| (e.realization.id.as_str(), e.realization.thread_id.as_str()))
            .collect();

        let mut out = String::new();
        let _ = writeln!(
            out,
            "---\nathanor_vault: 1\nexported_at: {}\nschema_version: {}\ndevice_id: {}\ncounts: {{profile_sections: {}, domains: {}, pull_notes: {}, threads: {}, realizations: {}, correspondences: {}, sessions: {}, tending_days: {}, kindled: {}}}\n---\n",
            ts(self.now()),
            schema_version,
            q(&self.device_id),
            profile.len(),
            domains.len(),
            pulls.len(),
            threads.len(),
            realizations.len(),
            correspondences.len(),
            sessions.len(),
            tending.len(),
            kindling.len(),
        );
        out.push_str(
            "# Athanor vault\n\nEverything this device's store holds, in stable order. Free text is verbatim;\nthe raw SQLite copy shared alongside it is the lossless form.\n\n",
        );

        section(&mut out, "Profile", &profile, |o, (name, content, at)| {
            record(
                o,
                name,
                &[
                    ("kind", "profile".to_string()),
                    ("section", q(name)),
                    ("updated_at", ts(*at)),
                ],
                Some(content),
            );
        });

        section(&mut out, "Domains", &domains, |o, d| {
            record(
                o,
                &d.name,
                &[
                    ("kind", "domain".to_string()),
                    ("id", q(&d.id)),
                    ("name", q(&d.name)),
                    ("created_at", ts(d.created_at)),
                    ("updated_at", ts(d.updated_at)),
                    ("deleted_at", ts_opt(d.deleted_at)),
                ],
                None,
            );
        });

        section(&mut out, "Pull notes", &pulls, |o, n| {
            record(
                o,
                &format!("pull · {}", n.id),
                &[
                    ("kind", "pull_note".to_string()),
                    ("id", q(&n.id)),
                    ("domain_id", q_opt(n.domain_id.as_deref())),
                    ("domain", q_opt(name_of(n.domain_id.as_deref()))),
                    ("created_at", ts(n.created_at)),
                ],
                Some(&n.text),
            );
        });

        section(&mut out, "Threads", &threads, |o, t| {
            let parent = t
                .parent_realization_id
                .as_deref()
                .and_then(|r| rz_thread.get(r).copied());
            let mut kids: Vec<_> = realizations
                .iter()
                .filter(|e| e.realization.thread_id == t.id)
                .filter_map(|e| {
                    e.realization
                        .child_thread_id
                        .as_deref()
                        .map(|c| (e.realization.created_at, e.realization.id.as_str(), c))
                })
                .collect();
            kids.sort();
            let kids: Vec<String> = kids.into_iter().map(|(_, _, c)| c.to_string()).collect();
            let heading = t.prompt.lines().next().unwrap_or("").trim();
            record(
                o,
                heading,
                &[
                    ("kind", "thread".to_string()),
                    ("id", q(&t.id)),
                    ("prompt", q(&t.prompt)),
                    ("state", q(t.state.as_str())),
                    ("domain_id", q_opt(t.domain_id.as_deref())),
                    ("domain", q_opt(name_of(t.domain_id.as_deref()))),
                    ("born", ts(t.born)),
                    ("last_worked", ts_opt(t.last_worked)),
                    (
                        "parent_realization_id",
                        q_opt(t.parent_realization_id.as_deref()),
                    ),
                    ("parent_thread_id", q_opt(parent)),
                    ("child_thread_ids", q_list(&kids)),
                    ("created_at", ts(t.created_at)),
                    ("updated_at", ts(t.updated_at)),
                    ("deleted_at", ts_opt(t.deleted_at)),
                ],
                None,
            );
        });

        section(&mut out, "Realizations", &realizations, |o, e| {
            let r = &e.realization;
            record(
                o,
                &format!("{} · {}", &iso_utc(r.date)[..10], r.id),
                &[
                    ("kind", "realization".to_string()),
                    ("id", q(&r.id)),
                    ("date", ts(r.date)),
                    ("thread_id", q(&r.thread_id)),
                    ("child_thread_id", q_opt(r.child_thread_id.as_deref())),
                    ("domains", q_list(&e.domains)),
                    ("created_at", ts(r.created_at)),
                ],
                Some(&r.text),
            );
        });

        section(&mut out, "Correspondences", &correspondences, |o, c| {
            record(
                o,
                &format!("{} ⟷ {}", c.domain_a, c.domain_b),
                &[
                    ("kind", "correspondence".to_string()),
                    ("id", q(&c.id)),
                    ("domain_a", q(&c.domain_a)),
                    ("domain_b", q(&c.domain_b)),
                    ("created_at", ts(c.created_at)),
                ],
                Some(&c.note),
            );
        });

        // Sessions need fallible reads, so collect their rendering first.
        let mut session_err: Option<CoreError> = None;
        section(&mut out, "Sessions", &sessions, |o, s| {
            let turns = transcript::parse(&s.transcript);
            let mut body = String::new();
            let notes = self.session_notes_for(&s.id);
            let traces = self.traces_for(&s.id);
            match (notes, traces) {
                (Ok(notes), Ok(traces)) => {
                    for (at, text) in notes {
                        let _ = write!(body, "#### Note · {}\n\n{}\n\n", iso_utc(at), text.trim());
                    }
                    for (at, text) in traces {
                        let _ = write!(body, "#### Trace · {}\n\n{}\n\n", iso_utc(at), text.trim());
                    }
                }
                (Err(e), _) | (_, Err(e)) => {
                    session_err.get_or_insert(e);
                }
            }
            body.push_str("#### Transcript\n\n");
            for t in &turns {
                let who = match t.role {
                    TranscriptRole::Learner => "◇ learner",
                    TranscriptRole::Mystagogue => "☿ mystagogue",
                };
                let _ = write!(body, "##### {who}\n\n{}\n\n", t.text.trim());
            }
            let tp = s
                .thread_id
                .as_deref()
                .and_then(|id| thread_prompt.get(id).copied());
            record(
                o,
                &format!(
                    "{} · {}/{} · {}",
                    iso_utc(s.started_at),
                    s.mask,
                    s.mode,
                    s.id
                ),
                &[
                    ("kind", "session".to_string()),
                    ("id", q(&s.id)),
                    ("state", q(&s.state)),
                    ("mask", q(&s.mask)),
                    ("mode", q(&s.mode)),
                    ("thread_id", q_opt(s.thread_id.as_deref())),
                    ("thread_prompt", q_opt(tp)),
                    ("started_at", ts(s.started_at)),
                    ("ended_at", ts_opt(s.ended_at)),
                    ("created_at", ts(s.created_at)),
                    ("updated_at", ts(s.updated_at)),
                    ("turns", turns.len().to_string()),
                ],
                Some(&body),
            );
        });
        if let Some(e) = session_err {
            return Err(e);
        }

        section(&mut out, "Tending", &tending, |o, t| {
            record(
                o,
                &t.day,
                &[
                    ("kind", "tending".to_string()),
                    ("day", q(&t.day)),
                    ("minutes", t.minutes.to_string()),
                    ("thread_ids", q_list(&t.thread_ids)),
                    ("created_at", ts(t.created_at)),
                ],
                None,
            );
        });

        section(&mut out, "Kindling", &kindling, |o, (key, at, src)| {
            record(
                o,
                key,
                &[
                    ("kind", "kindling".to_string()),
                    ("passage_key", q(key)),
                    ("first_kindled_at", ts(*at)),
                    ("source_id", q_opt(src.as_deref())),
                ],
                None,
            );
        });

        let mut out = out.trim_end().to_string();
        out.push('\n');
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use rusqlite::params;

    use super::*;
    use crate::transcript::format_block;

    const CLOCK: u64 = 1_795_381_200; // 2026-11-22T21:00:00Z

    fn store() -> Store {
        Store::open_in_memory("device")
            .unwrap()
            .with_clock(Arc::new(|| CLOCK))
    }

    /// The snapshot fixture (docs/plans/2026-11-22-relight-v1.0.7.md, Task 4),
    /// seeded with fixed ids by direct SQL in FK-safe order.
    fn seed_fixture(store: &Store) {
        let c = store.conn();
        c.execute(
            "INSERT INTO domains (id, name, created_at, updated_at, device_id, deleted_at)
             VALUES ('dom-thermo', 'Thermodynamics', 1783843200, 1783843200, 'device', NULL)",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO threads (id, prompt, domain_id, state, born, last_worked, parent_realization_id, created_at, updated_at, device_id, deleted_at)
             VALUES ('thr-entropy', 'what is entropy?', 'dom-thermo', 'fixed', 1783843200, 1784020800, NULL, 1783843200, 1783843200, 'device', NULL)",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO realizations (id, text, date, thread_id, child_thread_id, created_at, device_id)
             VALUES ('rz-1', 'Forgetting a bit always costs heat — erasure is never free.', 1784020320, 'thr-entropy', NULL, 1784020320, 'device')",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO threads (id, prompt, domain_id, state, born, last_worked, parent_realization_id, created_at, updated_at, device_id, deleted_at)
             VALUES ('thr-child', 'what does this open?', 'dom-thermo', 'volatile', 1784020320, NULL, 'rz-1', 1784020320, 1784020320, 'device', NULL)",
            [],
        )
        .unwrap();
        c.execute("UPDATE realizations SET child_thread_id = 'thr-child'", [])
            .unwrap();
        c.execute(
            "INSERT INTO realization_domains (realization_id, domain_id) VALUES ('rz-1', 'dom-thermo')",
            [],
        )
        .unwrap();
        let transcript = format!(
            "{}{}",
            format_block(TranscriptRole::Learner, "Forgetting costs energy, doesn't it?"),
            format_block(
                TranscriptRole::Mystagogue,
                "It does — erasing a bit dissipates heat. Say it in your own words and we'll fix it."
            )
        );
        c.execute(
            "INSERT INTO sessions (id, thread_id, mask, mode, state, transcript, started_at, ended_at, created_at, updated_at, device_id)
             VALUES ('ses-1', 'thr-entropy', 'philosophus', 'explain', 'closed', ?1, 1784019600, 1784020800, 1784019600, 1784020800, 'device')",
            params![transcript],
        )
        .unwrap();
        c.execute(
            "INSERT INTO session_notes (id, session_id, thread_id, note, created_at, device_id)
             VALUES ('n-1', 'ses-1', 'thr-entropy', 'Learner tied Landauer''s principle to forgetting; salt fixed.', 1784020800, 'device')",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO traces (id, session_id, text, created_at, device_id)
             VALUES ('tr-1', 'ses-1', 'landauer: forgetting costs heat', 1784020800, 'device')",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO tending (day, minutes, thread_ids, created_at, device_id)
             VALUES ('2026-07-14', 20, '[\"thr-entropy\"]', 1784020800, 'device')",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO kindling (passage_key, first_kindled_at, source_id) VALUES ('SALT', 1784020320, 'rz-1')",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO profile (section, content, updated_at) VALUES ('how_i_learn', 'dialogue, proof-demanding', 1783843200)",
            [],
        )
        .unwrap();
    }

    #[test]
    fn iso_utc_formats_epoch_seconds() {
        assert_eq!(iso_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_utc(1_784_019_630), "2026-07-14T09:00:30Z");
        assert_eq!(iso_utc(1_795_381_200), "2026-11-22T21:00:00Z");
    }

    #[test]
    fn export_vault_matches_snapshot() {
        let s = store();
        seed_fixture(&s);
        let got = s.export_vault().unwrap();
        let want = include_str!("testdata/vault_export_fixture.md");
        if got != want {
            println!("--- GOT ---\n{got}\n--- WANT ---\n{want}");
        }
        assert_eq!(got, want);
    }

    #[test]
    fn export_vault_is_deterministic() {
        let s = store();
        seed_fixture(&s);
        assert_eq!(s.export_vault().unwrap(), s.export_vault().unwrap());
    }

    #[test]
    fn export_vault_empty_store_has_every_section() {
        let doc = store().export_vault().unwrap();
        let headings = [
            "## Profile",
            "## Domains",
            "## Pull notes",
            "## Threads",
            "## Realizations",
            "## Correspondences",
            "## Sessions",
            "## Tending",
            "## Kindling",
        ];
        let mut last = 0;
        for h in headings {
            let at = doc
                .find(&format!("\n{h}\n"))
                .unwrap_or_else(|| panic!("missing {h}"));
            assert!(at >= last, "{h} out of order");
            last = at;
        }
        assert_eq!(doc.matches("_none_").count(), 9);
        assert!(doc.contains("counts: {profile_sections: 0,"));
    }

    #[test]
    fn export_vault_escapes_frontmatter_strings() {
        let s = store();
        let t = s.open_thread("say \"hi\"\nthen go", None, None).unwrap();
        s.create_session(Some(&t.id), "philosophus", "explain")
            .unwrap();
        let doc = s.export_vault().unwrap();
        assert!(doc.contains("### say \"hi\"\n"));
        assert!(doc.contains("prompt: \"say \\\"hi\\\"\\nthen go\"\n"));
        assert!(doc.contains("thread_prompt: \"say \\\"hi\\\"\\nthen go\"\n"));
    }

    #[test]
    fn export_vault_via_public_api_carries_everything() {
        let s = store();
        let d = s.upsert_domain("Thermodynamics").unwrap();
        let pull = s.add_pull_note(Some(&d.id), "why does heat flow?").unwrap();
        let t = s
            .open_thread("what is entropy?", Some(&d.id), None)
            .unwrap();
        let ses = s
            .create_session(Some(&t.id), "philosophus", "explain")
            .unwrap();
        s.append_transcript(
            &ses.id,
            &format_block(TranscriptRole::Learner, "forgetting costs heat"),
        )
        .unwrap();
        s.append_transcript(
            &ses.id,
            &format_block(TranscriptRole::Mystagogue, "say it in your words"),
        )
        .unwrap();
        let rz = s
            .fix_salt(
                &t.id,
                "Erasure is never free.",
                &["Thermodynamics".to_string()],
                Some("what does erasure open?"),
            )
            .unwrap();
        let co = s
            .weave_domains("Thermodynamics", "Memory", "Erasure is forgetting.")
            .unwrap();
        s.add_session_note(&ses.id, Some(&t.id), "a note on the session")
            .unwrap();
        s.add_trace(&ses.id, "a trace line").unwrap();
        s.record_tending("2026-07-14", 12, std::slice::from_ref(&t.id))
            .unwrap();
        s.set_profile_section("frictions", "gets lost in notation")
            .unwrap();
        let gone = s.create_session(None, "philosophus", "explain").unwrap();
        s.mark_session_abandoned(&gone.id).unwrap();
        let evap = s.open_thread("a passing fancy?", None, None).unwrap();
        s.evaporate_thread(&evap.id).unwrap();

        let doc = s.export_vault().unwrap();
        for id in [
            &d.id, &pull.id, &t.id, &ses.id, &rz.id, &co.id, &gone.id, &evap.id,
        ] {
            assert!(doc.contains(id.as_str()), "missing id {id}");
        }
        assert!(rz.child_thread_id.is_some());
        assert!(doc.contains("state: \"evaporated\""));
        assert!(doc.contains("state: \"abandoned\""));
        assert!(doc.contains("why does heat flow?"));
        assert!(doc.contains("Erasure is forgetting."));
        assert!(doc.contains("turns: 2"));
        assert!(doc.contains("a note on the session"));
        assert!(doc.contains("a trace line"));
        assert!(doc.contains("gets lost in notation"));
    }

    #[test]
    fn export_vault_pull_note_and_correspondence_shapes() {
        let s = store();
        {
            let c = s.conn();
            c.execute(
                "INSERT INTO pull_notes (id, domain_id, text, created_at, device_id)
                 VALUES ('pn-1', NULL, 'I keep circling Maxwell''s demon.', 1784019600, 'device')",
                [],
            )
            .unwrap();
            c.execute(
                "INSERT INTO correspondences (id, domain_a, domain_b, note, created_at, device_id)
                 VALUES ('co-1', 'Thermodynamics', 'Memory', 'Erasure is forgetting.', 1784020320, 'device')",
                [],
            )
            .unwrap();
        }
        let doc = s.export_vault().unwrap();
        let pull = "### pull · pn-1\n\n---\nkind: pull_note\nid: \"pn-1\"\ndomain_id: null\ndomain: null\ncreated_at: \"2026-07-14T09:00:00Z\"\n---\n\nI keep circling Maxwell's demon.\n\n";
        let corr = "### Thermodynamics ⟷ Memory\n\n---\nkind: correspondence\nid: \"co-1\"\ndomain_a: \"Thermodynamics\"\ndomain_b: \"Memory\"\ncreated_at: \"2026-07-14T09:12:00Z\"\n---\n\nErasure is forgetting.\n\n";
        assert!(doc.contains(pull), "pull note block:\n{doc}");
        assert!(doc.contains(corr), "correspondence block:\n{doc}");
    }

    /// A fresh temp dir; the caller removes it.
    fn tmpdir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("athanor-export-{}", crate::ids::new_id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn open_read_only_refuses_missing_file() {
        let dir = tmpdir();
        let p = dir.join("nope.sqlite");
        assert!(Store::open_read_only(&p, "d").is_err());
        assert!(!p.exists());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn open_read_only_refuses_schema_mismatch() {
        let dir = tmpdir();
        let p = dir.join("old.sqlite");
        {
            let c = rusqlite::Connection::open(&p).unwrap();
            c.pragma_update(None, "user_version", 1).unwrap();
        }
        assert!(Store::open_read_only(&p, "d").is_err());
        let c = rusqlite::Connection::open(&p).unwrap();
        let v: i64 = c
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(v, 1, "no migration ran");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn open_read_only_cannot_write() {
        let dir = tmpdir();
        let p = dir.join("live.sqlite");
        drop(Store::open(&p, "d").unwrap());
        let ro = Store::open_read_only(&p, "d").unwrap();
        assert!(ro.open_thread("x", None, None).is_err());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn database_copy_from_read_only_store_matches() {
        let dir = tmpdir();
        let src = dir.join("src.sqlite");
        {
            let s = Store::open(&src, "device").unwrap();
            seed_fixture(&s);
        }
        let clock: crate::store::Clock = Arc::new(|| CLOCK);
        let ro = Store::open_read_only(&src, "device")
            .unwrap()
            .with_clock(clock.clone());
        let dest = dir.join("copy.sqlite");
        ro.export_database_copy(&dest).unwrap();
        let copy = Store::open_read_only(&dest, "device")
            .unwrap()
            .with_clock(clock);
        assert_eq!(copy.export_vault().unwrap(), ro.export_vault().unwrap());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn database_copy_refuses_existing_dest() {
        let dir = tmpdir();
        let dest = dir.join("exists.sqlite");
        std::fs::write(&dest, b"keep me").unwrap();
        let s = store();
        assert!(s.export_database_copy(&dest).is_err());
        assert_eq!(std::fs::metadata(&dest).unwrap().len(), 7);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn database_copy_from_in_memory_store() {
        let dir = tmpdir();
        let s = Store::open_in_memory("d").unwrap();
        let t = s.open_thread("q?", None, None).unwrap();
        let dest = dir.join("mem.sqlite");
        s.export_database_copy(&dest).unwrap();
        assert!(Store::open_read_only(&dest, "d")
            .unwrap()
            .get_thread(&t.id)
            .is_ok());
        std::fs::remove_dir_all(dir).ok();
    }
}
