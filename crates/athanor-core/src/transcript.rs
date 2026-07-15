//! The durable, role-tagged session transcript: the full corpus of a session's
//! dialogue — BOTH the learner's turns and the Mystagogue's — persisted to the
//! session row so nothing said is lost when the session closes.
//!
//! It is deliberately human-readable (it doubles as the reading view's source
//! and a later-mining corpus): each turn is a block introduced by a glyph
//! marker on its own line, then the turn's text, then a blank line. The same
//! constants format (`Conductor::run_turn_inner` writes) and parse
//! (`Store`/FFI read the transcript back into structured turns) it, so the
//! round-trip is one source of truth.
//!
//! ```text
//! ◇ LEARNER
//! Forgetting costs energy, doesn't it?
//!
//! ☿ MYSTAGOGUE
//! That thread about forgetting — say what just set.
//!
//! ```

/// Who spoke a persisted transcript block. Mirrors `engine::AcpRole` but is a
/// distinct type: `AcpRole` is the live wire shape, this is the durable
/// on-disk/read-projection shape (they can drift independently).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptRole {
    Learner,
    Mystagogue,
}

impl TranscriptRole {
    /// The lower-case tag the FFI projection hands to Swift.
    pub fn as_tag(self) -> &'static str {
        match self {
            TranscriptRole::Learner => "learner",
            TranscriptRole::Mystagogue => "mystagogue",
        }
    }

    /// The full-line marker sentinel that introduces this role's block in the
    /// persisted transcript. A line equal to one of these (after trimming the
    /// trailing newline) starts a new block.
    fn marker(self) -> &'static str {
        match self {
            TranscriptRole::Learner => "◇ LEARNER",
            TranscriptRole::Mystagogue => "☿ MYSTAGOGUE",
        }
    }
}

/// One parsed turn of a persisted transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptEntry {
    pub role: TranscriptRole,
    pub text: String,
}

/// Formats one turn as a block to APPEND to the session's transcript column:
/// the role marker on its own line, the (trimmed) turn text, then a blank line
/// separating it from the next block. Empty text still emits the marker so the
/// corpus records that a turn happened (an empty learner turn is unusual but
/// not a reason to silently drop the block).
pub fn format_block(role: TranscriptRole, text: &str) -> String {
    format!("{}\n{}\n\n", role.marker(), text.trim_end())
}

/// Parses a persisted transcript back into ordered turns. Liberal by design:
/// text before the first marker (legacy transcripts persisted before this
/// scheme were raw Mystagogue text) is returned as one leading `Mystagogue`
/// entry so nothing is dropped; a marker line with no body yields an
/// empty-text entry. Round-trips with [`format_block`].
pub fn parse(transcript: &str) -> Vec<TranscriptEntry> {
    let mut entries: Vec<TranscriptEntry> = Vec::new();
    let mut current: Option<(TranscriptRole, Vec<&str>)> = None;
    // A legacy/pre-scheme prefix (no marker yet) accumulates here and, if any
    // real content lands before the first marker, becomes a Mystagogue entry.
    let mut preamble: Vec<&str> = Vec::new();

    let role_for = |line: &str| -> Option<TranscriptRole> {
        if line == TranscriptRole::Learner.marker() {
            Some(TranscriptRole::Learner)
        } else if line == TranscriptRole::Mystagogue.marker() {
            Some(TranscriptRole::Mystagogue)
        } else {
            None
        }
    };

    let flush = |entries: &mut Vec<TranscriptEntry>, role: TranscriptRole, lines: Vec<&str>| {
        let text = lines.join("\n").trim().to_string();
        entries.push(TranscriptEntry { role, text });
    };

    for line in transcript.split('\n') {
        if let Some(role) = role_for(line) {
            // Close any legacy preamble as a Mystagogue entry before the first
            // real marked block.
            if current.is_none() && preamble.iter().any(|l| !l.trim().is_empty()) {
                flush(
                    &mut entries,
                    TranscriptRole::Mystagogue,
                    std::mem::take(&mut preamble),
                );
            }
            preamble.clear();
            if let Some((prev_role, prev_lines)) = current.take() {
                flush(&mut entries, prev_role, prev_lines);
            }
            current = Some((role, Vec::new()));
        } else if let Some((_, lines)) = current.as_mut() {
            lines.push(line);
        } else {
            preamble.push(line);
        }
    }

    if let Some((role, lines)) = current.take() {
        flush(&mut entries, role, lines);
    } else if preamble.iter().any(|l| !l.trim().is_empty()) {
        flush(&mut entries, TranscriptRole::Mystagogue, preamble);
    }

    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_block_wraps_marker_text_and_blank_line() {
        let block = format_block(TranscriptRole::Learner, "Forgetting costs energy?");
        assert_eq!(block, "◇ LEARNER\nForgetting costs energy?\n\n");
    }

    #[test]
    fn round_trips_a_two_turn_exchange() {
        let mut t = String::new();
        t.push_str(&format_block(
            TranscriptRole::Learner,
            "Forgetting costs energy?",
        ));
        t.push_str(&format_block(
            TranscriptRole::Mystagogue,
            "Say what just set.",
        ));
        let parsed = parse(&t);
        assert_eq!(
            parsed,
            vec![
                TranscriptEntry {
                    role: TranscriptRole::Learner,
                    text: "Forgetting costs energy?".to_string()
                },
                TranscriptEntry {
                    role: TranscriptRole::Mystagogue,
                    text: "Say what just set.".to_string()
                },
            ]
        );
    }

    #[test]
    fn preserves_multiline_turn_text() {
        let block = format_block(TranscriptRole::Mystagogue, "line one\nline two\nline three");
        let parsed = parse(&block);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].text, "line one\nline two\nline three");
    }

    #[test]
    fn legacy_unmarked_transcript_reads_as_one_mystagogue_entry() {
        // Transcripts persisted before the role-tag scheme were raw Mystagogue
        // text with no markers; they must still read back, not vanish.
        let parsed = parse("just raw assistant text, no markers");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].role, TranscriptRole::Mystagogue);
        assert_eq!(parsed[0].text, "just raw assistant text, no markers");
    }

    #[test]
    fn empty_transcript_parses_to_nothing() {
        assert!(parse("").is_empty());
    }

    #[test]
    fn as_tag_is_stable() {
        assert_eq!(TranscriptRole::Learner.as_tag(), "learner");
        assert_eq!(TranscriptRole::Mystagogue.as_tag(), "mystagogue");
    }
}
