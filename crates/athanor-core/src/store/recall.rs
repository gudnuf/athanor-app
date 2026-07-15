//! `recall`: search past sessions' role-tagged transcripts for what the learner
//! actually said. The whole point of persisting full transcripts (the "note
//! stump" the operator asked for) — "what was I saying about X?" is answerable
//! because the words were kept, not just a one-line trace.
//!
//! v1 is a case-insensitive `LIKE` scan (single-user on-device DB, no FTS
//! needed — see the indices note beside `MIGRATIONS`). Each hit returns a
//! window of surrounding text with the `◇ LEARNER` / `☿ MYSTAGOGUE` markers
//! intact, so the learner's own words stay attributable in the result.

use rusqlite::params;

use crate::domain::RecallHit;
use crate::error::CoreError;

use super::Store;

/// Characters of surrounding context kept on each side of a match — enough to
/// see the exchange around the hit (both roles), bounded so a broad query can't
/// return whole sessions.
const WINDOW_BEFORE: usize = 160;
const WINDOW_AFTER: usize = 340;

impl Store {
    /// Searches closed sessions' transcripts for `query` (case-insensitive),
    /// newest first, returning up to `limit` windowed excerpts. Excludes the
    /// `exclude_session_id` (the current session — already in context) and any
    /// session with an empty transcript (abandoned/never-spoke). Never matches
    /// on itself: an empty/blank query returns nothing.
    pub fn search_transcripts(
        &self,
        query: &str,
        exclude_session_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<RecallHit>, CoreError> {
        let q = query.trim();
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let pattern = format!("%{}%", escape_like(q));
        let exclude = exclude_session_id.unwrap_or("");

        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT s.id, s.created_at, s.mask, s.transcript, t.prompt
             FROM sessions s LEFT JOIN threads t ON t.id = s.thread_id
             WHERE s.state = 'closed'
               AND s.transcript != ''
               AND s.id != ?1
               AND s.transcript LIKE ?2 ESCAPE '\\'
             ORDER BY s.created_at DESC, s.id DESC
             LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![exclude, pattern, limit as i64], |row| {
            let session_id: String = row.get(0)?;
            let created_at: u64 = row.get(1)?;
            let mask: String = row.get(2)?;
            let transcript: String = row.get(3)?;
            let thread_prompt: Option<String> = row.get(4)?;
            Ok((session_id, created_at, mask, transcript, thread_prompt))
        })?;

        let mut hits = Vec::new();
        for row in rows {
            let (session_id, created_at, mask, transcript, thread_prompt) = row?;
            let excerpt = window_around_match(&transcript, q);
            hits.push(RecallHit {
                session_id,
                created_at,
                mask,
                thread_prompt,
                excerpt,
            });
        }
        Ok(hits)
    }
}

/// Escapes `%`, `_`, and `\` so a query is matched literally by `LIKE ... ESCAPE '\'`.
fn escape_like(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// A char-safe window of `transcript` around the first case-insensitive match
/// of `needle`, with `…` where it was clipped. Falls back to the head of the
/// transcript if the needle isn't found (shouldn't happen — the SQL already
/// matched — but keeps this total).
fn window_around_match(transcript: &str, needle: &str) -> String {
    let lower = transcript.to_lowercase();
    let needle_lower = needle.to_lowercase();
    let char_start = match lower.find(&needle_lower) {
        Some(byte_pos) => lower[..byte_pos].chars().count(),
        None => 0,
    };
    let chars: Vec<char> = transcript.chars().collect();
    let start = char_start.saturating_sub(WINDOW_BEFORE);
    let end = (char_start + needle_lower.chars().count() + WINDOW_AFTER).min(chars.len());
    let core: String = chars[start..end].iter().collect();
    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    out.push_str(core.trim());
    if end < chars.len() {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::{format_block, TranscriptRole};

    /// Persists a closed session with a role-tagged transcript, returns its id.
    fn closed_session_with(
        store: &Store,
        thread_id: Option<&str>,
        learner: &str,
        mystagogue: &str,
    ) -> String {
        let s = store
            .create_session(thread_id, "philosophus", "explain")
            .unwrap();
        let mut t = String::new();
        t.push_str(&format_block(TranscriptRole::Learner, learner));
        t.push_str(&format_block(TranscriptRole::Mystagogue, mystagogue));
        store.append_transcript(&s.id, &t).unwrap();
        store.mark_session_closed(&s.id).unwrap();
        s.id
    }

    #[test]
    fn recall_finds_learner_words_with_attribution() {
        let store = Store::open_in_memory("d")
            .unwrap()
            .with_clock(std::sync::Arc::new(|| 100));
        let thread = store.open_thread("what is beingness?", None, None).unwrap();
        closed_session_with(
            &store,
            Some(&thread.id),
            "I think beingness is a verb — the grumblewarp of the self, always happening.",
            "Say what you mean by grumblewarp.",
        );
        // a second, unrelated session that must NOT match
        let store = store.with_clock(std::sync::Arc::new(|| 200));
        closed_session_with(
            &store,
            None,
            "Let's talk about magnetism instead.",
            "Go on.",
        );

        let hits = store.search_transcripts("grumblewarp", None, 5).unwrap();
        assert_eq!(hits.len(), 1, "only the session with the word matches");
        assert!(
            hits[0].excerpt.contains("grumblewarp"),
            "the excerpt carries the learner's word: {:?}",
            hits[0].excerpt
        );
        assert!(
            hits[0].excerpt.contains("LEARNER"),
            "the learner's role is attributable in the excerpt"
        );
        assert_eq!(hits[0].thread_prompt.as_deref(), Some("what is beingness?"));
        assert_eq!(hits[0].mask, "philosophus");
    }

    #[test]
    fn recall_excludes_the_current_session_and_empty_ones() {
        let store = Store::open_in_memory("d").unwrap();
        let current = closed_session_with(&store, None, "I said the magic word alpenglow.", "Mm.");
        // an OPEN session with the term (never closed) must not surface
        let open = store.create_session(None, "solve", "design").unwrap();
        store
            .append_transcript(&open.id, "alpenglow again but still open")
            .unwrap();

        // excluding the current session hides its own hit…
        let hits = store
            .search_transcripts("alpenglow", Some(&current), 5)
            .unwrap();
        assert!(
            hits.is_empty(),
            "current session + open session both excluded"
        );
        // …and including it surfaces exactly the one closed hit.
        let hits = store.search_transcripts("alpenglow", None, 5).unwrap();
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn recall_is_case_insensitive_and_bounded_by_limit() {
        let store = Store::open_in_memory("d").unwrap();
        for i in 0..8 {
            closed_session_with(
                &store,
                None,
                &format!("session {i}: the term ENTROPY recurs here"),
                "noted.",
            );
        }
        let hits = store.search_transcripts("entropy", None, 3).unwrap();
        assert_eq!(hits.len(), 3, "capped at the limit even with many matches");
        assert!(hits[0].excerpt.to_lowercase().contains("entropy"));
    }

    #[test]
    fn recall_blank_query_returns_nothing() {
        let store = Store::open_in_memory("d").unwrap();
        closed_session_with(&store, None, "anything at all", "ok");
        assert!(store.search_transcripts("   ", None, 5).unwrap().is_empty());
    }

    #[test]
    fn recall_treats_like_wildcards_literally() {
        let store = Store::open_in_memory("d").unwrap();
        closed_session_with(&store, None, "we discussed 100% commitment", "good");
        closed_session_with(&store, None, "no percentage here", "ok");
        // '%' must be matched literally, not as a wildcard.
        let hits = store.search_transcripts("100%", None, 5).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].excerpt.contains("100%"));
    }
}
