//! Pure markdown parsers for the lived-in demo seed. NO IO, NO `Store`: every
//! function here takes a `&str` and returns plain data, so the whole layer is
//! hermetically testable on tiny synthetic samples (see the `tests` module).
//! The real academy markdown is read at runtime by `translate.rs`; nothing
//! from it is embedded here.

/// One dated journal entry: `## YYYY-MM-DD — title` followed by its prose body,
/// up to the next level-2 heading. `###` subheadings stay part of the body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalEntry {
    pub date: String,
    pub title: String,
    pub body: String,
}

/// A `## To <Other>` block inside a domain's correspondences.md.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorrespondenceLink {
    pub other: String,
    pub note: String,
}

/// A dated session bullet from STATE's "Recent Sessions", with its minutes
/// (parsed from a `~NNmin` marker, else a nominal default).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionDay {
    pub day: String,
    pub minutes: u32,
}

/// Nominal minutes recorded for a session bullet that carries no `~NNmin`.
pub const DEFAULT_SESSION_MINUTES: u32 = 15;

/// Converts a `YYYY-MM-DD` day to UTC-midnight epoch seconds. Returns `None`
/// on a malformed date. Uses Howard Hinnant's `days_from_civil` so it needs no
/// chrono dependency and round-trips with the core's `today_utc`.
pub fn date_to_epoch(day: &str) -> Option<u64> {
    let (y, m, d) = parse_ymd(day)?;
    let days = days_from_civil(y as i64, m as i64, d as i64);
    if days < 0 {
        return None;
    }
    Some(days as u64 * 86_400)
}

fn parse_ymd(day: &str) -> Option<(u32, u32, u32)> {
    let b = day.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    if !b
        .iter()
        .enumerate()
        .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
    {
        return None;
    }
    let y: u32 = day[0..4].parse().ok()?;
    let m: u32 = day[5..7].parse().ok()?;
    let d: u32 = day[8..10].parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some((y, m, d))
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// True for a level-2 heading line (`## `), false for `###`+ or non-headings.
fn is_h2(line: &str) -> bool {
    line.starts_with("## ") && !line.starts_with("### ")
}

/// Finds the first `YYYY-MM-DD` substring in `s`.
fn first_date_in(s: &str) -> Option<String> {
    let b = s.as_bytes();
    if b.len() < 10 {
        return None;
    }
    for i in 0..=b.len() - 10 {
        // Only slice at UTF-8 boundaries; a `YYYY-MM-DD` is pure ASCII, so a
        // window straddling a multibyte char (e.g. an em-dash) can't be a date.
        if !s.is_char_boundary(i) || !s.is_char_boundary(i + 10) {
            continue;
        }
        let win = &s[i..i + 10];
        if parse_ymd(win).is_some() {
            return Some(win.to_string());
        }
    }
    None
}

/// Finds a `~NNmin` / `NNmin` minute count in `s`.
fn find_minutes(s: &str) -> Option<u32> {
    let b = s.as_bytes();
    let target = b"min";
    let mut i = 0;
    while i + 3 <= b.len() {
        if &b[i..i + 3] == target {
            // walk back over digits
            let mut j = i;
            while j > 0 && b[j - 1].is_ascii_digit() {
                j -= 1;
            }
            if j < i {
                if let Ok(n) = s[j..i].parse::<u32>() {
                    return Some(n);
                }
            }
        }
        i += 1;
    }
    None
}

/// Splits a journal into dated entries. Heading form: `## YYYY-MM-DD — title`.
/// Entries with the same date are all returned (the journal has several).
pub fn parse_journal(md: &str) -> Vec<JournalEntry> {
    let mut entries = Vec::new();
    let mut current: Option<(String, String, Vec<&str>)> = None;

    for line in md.lines() {
        if is_h2(line) {
            if let Some((date, title, body)) = current.take() {
                push_entry(&mut entries, date, title, body);
            }
            let heading = line[3..].trim();
            if let Some(date) = first_date_in(heading) {
                let title = heading
                    .split('—')
                    .nth(1)
                    .map(str::trim)
                    .unwrap_or("")
                    .to_string();
                current = Some((date, title, Vec::new()));
            } else {
                current = None; // a non-dated H2: skip until the next dated one
            }
        } else if let Some((_, _, body)) = current.as_mut() {
            body.push(line);
        }
    }
    if let Some((date, title, body)) = current.take() {
        push_entry(&mut entries, date, title, body);
    }
    entries
}

fn push_entry(entries: &mut Vec<JournalEntry>, date: String, title: String, body: Vec<&str>) {
    let body = body.join("\n").trim().to_string();
    if body.is_empty() && title.is_empty() {
        return;
    }
    entries.push(JournalEntry { date, title, body });
}

/// The `open:` follow-up questions inside a journal entry body — the bullets
/// listed under a line whose trimmed text is `open:` (case-insensitive), used
/// to seed the spiral child thread's question.
pub fn entry_open_questions(body: &str) -> Vec<String> {
    collect_open_bullets(body)
}

/// An entry's explicit spiral follow-up: a single `opens: <question>` line
/// (case-insensitive). This is the authored next-question for the child thread
/// fix_salt births — distinct from, and preferred over, the older `open:`
/// bullet block. `None` when the entry has no such line (e.g. the operator's
/// real academy journal, whose children then fall back to the default). The
/// line is stripped from the fixed salt by `strip_next_question`.
pub fn entry_next_question(body: &str) -> Option<String> {
    for line in body.lines() {
        let t = line.trim();
        if t.len() >= 6 && t[..6].eq_ignore_ascii_case("opens:") {
            let q = strip_emphasis(t[6..].trim());
            if !q.is_empty() {
                return Some(q);
            }
        }
    }
    None
}

/// The entry body with any `opens:` line removed, so an authored spiral
/// question never also appears inside the fixed salt text. Bodies without an
/// `opens:` line come back unchanged (trimmed).
pub fn strip_next_question(body: &str) -> String {
    body.lines()
        .filter(|l| {
            let t = l.trim();
            !(t.len() >= 6 && t[..6].eq_ignore_ascii_case("opens:"))
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

/// Correspondence links from a domain's correspondences.md. Each `## To <X>`
/// block yields one link; `<X>` is cleaned of a leading "The " and any
/// `(parenthetical)`; the note is the block's first non-empty prose paragraph.
pub fn parse_correspondences(md: &str) -> Vec<CorrespondenceLink> {
    let mut links = Vec::new();
    let mut other: Option<String> = None;
    let mut note: Option<String> = None;

    let flush = |links: &mut Vec<CorrespondenceLink>,
                 other: &mut Option<String>,
                 note: &mut Option<String>| {
        if let Some(o) = other.take() {
            links.push(CorrespondenceLink {
                other: o,
                note: note.take().unwrap_or_default(),
            });
        } else {
            *note = None;
        }
    };

    for line in md.lines() {
        if is_h2(line) {
            flush(&mut links, &mut other, &mut note);
            let title = line[3..].trim();
            if let Some(rest) = title
                .strip_prefix("To ")
                .or_else(|| title.strip_prefix("to "))
            {
                other = Some(clean_domain_name(rest));
            } else {
                other = None;
            }
        } else if other.is_some() && note.is_none() {
            let t = strip_emphasis(line.trim());
            if !t.is_empty() {
                note = Some(t);
            }
        }
    }
    flush(&mut links, &mut other, &mut note);
    links
}

/// Dated session bullets from a STATE "Recent Sessions" list. A bullet is any
/// line that (after trimming) starts with `- ` and contains a `YYYY-MM-DD`.
pub fn parse_session_days(md: &str) -> Vec<SessionDay> {
    let mut out = Vec::new();
    for line in md.lines() {
        let t = line.trim_start();
        if !t.starts_with("- ") {
            continue;
        }
        if let Some(day) = first_date_in(t) {
            let minutes = find_minutes(t).unwrap_or(DEFAULT_SESSION_MINUTES);
            out.push(SessionDay { day, minutes });
        }
    }
    out
}

/// Open-question threads: bullets collected under a label line whose trimmed
/// lowercase text ends with `open:` or `open threads:`, or under an H2 whose
/// title contains "open". Cleaned of emphasis and checkboxes, deduped in order.
pub fn parse_open_questions(md: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut collecting = false;

    for line in md.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_lowercase();

        if is_h2(line) {
            collecting = lower.contains("open");
            continue;
        }
        let is_bullet = trimmed.starts_with("- ") || trimmed.starts_with("* ");
        if is_bullet && collecting {
            if let Some(q) = clean_bullet(trimmed) {
                if !out.contains(&q) {
                    out.push(q);
                }
            }
            continue;
        }
        if !is_bullet {
            // A label line turns collection on; any other prose turns it off.
            if (lower.ends_with("open:") || lower.ends_with("open threads:")) && !trimmed.is_empty()
            {
                collecting = true;
            } else if !trimmed.is_empty() && !lower.contains("open") {
                collecting = false;
            }
        }
    }
    out
}

/// Bullets under an `open:`-style label within a block of text (journal body).
fn collect_open_bullets(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut collecting = false;
    for line in text.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_lowercase();
        let is_bullet = trimmed.starts_with("- ") || trimmed.starts_with("* ");
        if is_bullet && collecting {
            if let Some(q) = clean_bullet(trimmed) {
                out.push(q);
            }
        } else if lower.ends_with("open:") {
            collecting = true;
        } else if !trimmed.is_empty() && !is_bullet {
            collecting = false;
        }
    }
    out
}

fn clean_bullet(line: &str) -> Option<String> {
    let s = line
        .trim_start_matches("- ")
        .trim_start_matches("* ")
        .trim();
    // Drop markdown checkboxes ("[x] ...", "[ ] ...").
    if s.starts_with("[x]") || s.starts_with("[X]") || s.starts_with("[ ]") {
        return None;
    }
    let s = strip_emphasis(s);
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn strip_emphasis(s: &str) -> String {
    s.replace("**", "").replace('*', "").trim().to_string()
}

fn clean_domain_name(raw: &str) -> String {
    let mut s = raw.trim();
    // strip a trailing parenthetical like "(not yet a domain)"
    if let Some(idx) = s.find('(') {
        s = s[..idx].trim();
    }
    let s = s
        .strip_prefix("The ")
        .or_else(|| s.strip_prefix("the "))
        .unwrap_or(s);
    strip_emphasis(s.trim())
}

/// Classifies a journal entry to zero or more of the known domain names,
/// matching purely on stems derived from each domain's own name at runtime —
/// NO hardcoded vocabulary (privacy: nothing from the source material lives in
/// this code). A hyphen/space/underscore-separated name contributes one stem
/// per segment (its first up-to-6 chars, min length 4), so `magnetism` matches
/// "magnet"/"magnetic" and `content-production` matches "content"/"production".
/// Returns matched names in `known`'s order; an entry that matches nothing gets
/// no domain link (deliberately conservative — better unlinked than wrong).
pub fn classify_domains(title: &str, body: &str, known: &[String]) -> Vec<String> {
    let hay = format!("{} {}", title, body).to_lowercase();
    let mut out = Vec::new();
    for name in known {
        if out.contains(name) {
            continue;
        }
        if domain_stems(name).iter().any(|stem| hay.contains(stem)) {
            out.push(name.clone());
        }
    }
    out
}

/// Stems derived from a domain name: one per segment, the first up-to-6 chars,
/// for segments of length >= 4. Empty for very short names (they'd over-match).
fn domain_stems(name: &str) -> Vec<String> {
    name.to_lowercase()
        .split(['-', ' ', '_'])
        .filter(|seg| seg.len() >= 4)
        .map(|seg| seg[..seg.len().min(6)].to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_to_epoch_known_days() {
        assert_eq!(date_to_epoch("1970-01-01"), Some(0));
        assert_eq!(date_to_epoch("1970-01-02"), Some(86_400));
        // 2026-07-06 — cross-check against a fixed known value.
        assert_eq!(date_to_epoch("2000-02-29"), Some(951_782_400));
    }

    #[test]
    fn date_to_epoch_rejects_garbage() {
        assert_eq!(date_to_epoch("not-a-date"), None);
        assert_eq!(date_to_epoch("2026-13-01"), None);
        assert_eq!(date_to_epoch("2026-01-32"), None);
        assert_eq!(date_to_epoch("2026/01/01"), None);
    }

    #[test]
    fn parse_journal_splits_dated_entries_keeping_subheadings() {
        let md = "# Journal\n\nintro\n\n## 2026-01-02 — first light\n\nbody one.\n\n### a subheading\n\nstill body one.\n\n## 2026-01-05 — second\n\nbody two.\n";
        let entries = parse_journal(md);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].date, "2026-01-02");
        assert_eq!(entries[0].title, "first light");
        assert!(entries[0].body.contains("### a subheading"));
        assert!(entries[0].body.contains("still body one."));
        assert_eq!(entries[1].date, "2026-01-05");
        assert_eq!(entries[1].title, "second");
        assert_eq!(entries[1].body, "body two.");
    }

    #[test]
    fn parse_journal_allows_repeated_dates() {
        let md = "## 2026-01-02 — a\n\naa\n\n## 2026-01-02 — b\n\nbb\n";
        let entries = parse_journal(md);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].date, entries[1].date);
        assert_eq!(entries[0].title, "a");
        assert_eq!(entries[1].title, "b");
    }

    #[test]
    fn entry_open_questions_reads_open_label_bullets() {
        let body = "some prose.\n\nopen:\n- is the field one loop or two?\n- which face is north?\n\nnext prose.";
        let qs = entry_open_questions(body);
        assert_eq!(
            qs,
            vec![
                "is the field one loop or two?".to_string(),
                "which face is north?".to_string()
            ]
        );
    }

    #[test]
    fn entry_next_question_reads_and_strips_the_opens_line() {
        let body = "the loaf came out flat. the starter was asleep.\n\nopens: okay but how do I actually tell when it's awake enough?";
        assert_eq!(
            entry_next_question(body).as_deref(),
            Some("okay but how do I actually tell when it's awake enough?")
        );
        let salt = strip_next_question(body);
        assert!(salt.contains("the starter was asleep."));
        assert!(
            !salt.to_lowercase().contains("opens:"),
            "salt is clean: {salt:?}"
        );
        // No opens: line -> body comes back unchanged (trimmed).
        assert_eq!(entry_next_question("just prose."), None);
        assert_eq!(strip_next_question("just prose.\n"), "just prose.");
    }

    #[test]
    fn parse_correspondences_extracts_links_and_notes() {
        let md = "# Correspondences\n\n## To The Widgets\n\nDirect link: widgets rhyme.\n\nmore.\n\n## To Plasma Physics (not yet a domain)\n\n**Bold** claim about plasma.\n";
        let links = parse_correspondences(md);
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].other, "Widgets");
        assert_eq!(links[0].note, "Direct link: widgets rhyme.");
        assert_eq!(links[1].other, "Plasma Physics");
        assert_eq!(links[1].note, "Bold claim about plasma.");
    }

    #[test]
    fn parse_session_days_reads_dates_and_minutes() {
        let md = "## Recent Sessions\n\n- **2026-06-01** — deep session (~75min). did things.\n- **2026-04-09 (evening)** — short one, no minutes.\n- not a bullet 2026-01-01\n";
        let days = parse_session_days(md);
        assert_eq!(days.len(), 3);
        assert_eq!(
            days[0],
            SessionDay {
                day: "2026-06-01".into(),
                minutes: 75
            }
        );
        assert_eq!(
            days[1],
            SessionDay {
                day: "2026-04-09".into(),
                minutes: DEFAULT_SESSION_MINUTES
            }
        );
        assert_eq!(days[2].day, "2026-01-01");
    }

    #[test]
    fn parse_open_questions_from_label_and_heading() {
        let md = "## Next\n\nsome prose here.\n\nOpen threads:\n- what is \"me\"?\n- how does a protector stand down?\n\n## Open Threads\n\n- name the move formally\n- what is \"me\"?\n";
        let qs = parse_open_questions(md);
        // deduped, in first-seen order
        assert_eq!(
            qs,
            vec![
                "what is \"me\"?".to_string(),
                "how does a protector stand down?".to_string(),
                "name the move formally".to_string(),
            ]
        );
    }

    #[test]
    fn parse_open_questions_skips_checkbox_bullets() {
        let md = "Open:\n- [x] already done\n- a real open question\n";
        let qs = parse_open_questions(md);
        assert_eq!(qs, vec!["a real open question".to_string()]);
    }

    #[test]
    fn classify_domains_matches_on_name_stems() {
        // Invented domains + prose; the contract is name-stem matching only.
        let known = vec!["widgetry".to_string(), "gizmo-fabrication".to_string()];
        // "widgetry" -> stem "widget"; prose says "widgets".
        let d = classify_domains("shapes", "the widgets stood up straight", &known);
        assert_eq!(d, vec!["widgetry".to_string()]);
        // hyphenated name: either segment stem hits ("gizmos" -> "gizmo").
        let d = classify_domains("turns", "the gizmos turned twice", &known);
        assert_eq!(d, vec!["gizmo-fabrication".to_string()]);
        // no stem present -> no link (conservative).
        let d = classify_domains("nothing", "unrelated prose here", &known);
        assert!(d.is_empty());
    }

    #[test]
    fn domain_stems_skips_short_segments() {
        assert_eq!(domain_stems("yo"), Vec::<String>::new());
        assert_eq!(domain_stems("widgetry"), vec!["widget".to_string()]);
        assert_eq!(
            domain_stems("gizmo-fabrication"),
            vec!["gizmo".to_string(), "fabric".to_string()]
        );
    }
}
