//! Reply-register parsing: the stream state machine that turns the model's raw
//! text stream into register-tagged, marker-stripped chunks.
//!
//! The Mystagogue speaks in two registers (identity.md §6): a **quick**
//! conversational voice (the default — questions, reflections, nudges) and a
//! **reading** voice reserved for genuine lessons. The model signals a reading
//! passage with an inline marker convention it is told about in the prompt pack:
//!
//! ```text
//! …a quick question. <!--reading-->A measured lesson, laid down.<!--/reading--> Back to quick.
//! ```
//!
//! [`RegisterParser`] consumes the raw delta stream, **strips those markers
//! entirely**, and emits `(text, Register)` runs so the UI can switch voices.
//! The markers must never reach the caller — not even split across two deltas
//! (`<!--rea` in one chunk, `ding-->` in the next). The parser guarantees this
//! by holding back any trailing text that is still a *prefix* of a marker until
//! enough arrives to decide, flushing the remainder at turn end.

use crate::engine::Register;

/// Opens a reading-voice passage. Text after it is [`Register::Reading`] until a
/// close marker (or end of turn).
pub const READING_OPEN: &str = "<!--reading-->";
/// Closes a reading-voice passage, returning to [`Register::Quick`].
pub const READING_CLOSE: &str = "<!--/reading-->";

/// Streaming parser: strips register markers and tags each emitted run with the
/// register in force. One per turn (state is the current register + a small
/// hold-back buffer for a marker that may be split across deltas).
#[derive(Default)]
pub struct RegisterParser {
    current: Register,
    /// Text withheld because its tail is still a possible partial marker.
    holdback: String,
}

impl RegisterParser {
    /// Feeds one raw delta in, emitting zero or more `(text, register)` runs for
    /// the fully-decided portion. Markers are consumed (never emitted). A tail
    /// that could still become a marker is buffered until the next `push` or a
    /// `flush`.
    pub fn push(&mut self, chunk: &str, emit: &mut impl FnMut(String, Register)) {
        self.holdback.push_str(chunk);
        loop {
            match first_marker(&self.holdback) {
                Some((idx, len, next)) => {
                    if idx > 0 {
                        let before = self.holdback[..idx].to_string();
                        emit(before, self.current);
                    }
                    self.current = next;
                    self.holdback.drain(..idx + len);
                }
                None => {
                    // No complete marker. Hold back only the longest suffix that
                    // is still a prefix of some marker; emit everything before it.
                    let keep = trailing_marker_prefix_len(&self.holdback);
                    let emit_to = self.holdback.len() - keep;
                    if emit_to > 0 {
                        let out = self.holdback[..emit_to].to_string();
                        emit(out, self.current);
                        self.holdback.drain(..emit_to);
                    }
                    break;
                }
            }
        }
    }

    /// Flushes any held-back tail as literal text at turn end. A partial marker
    /// that never completed is emitted verbatim (it was ordinary text, not a
    /// marker after all) rather than silently swallowed. Idempotent: a second
    /// call with an empty buffer is a no-op.
    pub fn flush(&mut self, emit: &mut impl FnMut(String, Register)) {
        if !self.holdback.is_empty() {
            let out = std::mem::take(&mut self.holdback);
            emit(out, self.current);
        }
    }
}

/// Finds the earliest complete marker in `s`. Returns `(byte_index, marker_len,
/// register_after)`. Markers are ASCII, so byte indices are char boundaries.
fn first_marker(s: &str) -> Option<(usize, usize, Register)> {
    let open = s.find(READING_OPEN);
    let close = s.find(READING_CLOSE);
    match (open, close) {
        (None, None) => None,
        (Some(o), None) => Some((o, READING_OPEN.len(), Register::Reading)),
        (None, Some(c)) => Some((c, READING_CLOSE.len(), Register::Quick)),
        // Both present: take whichever comes first. A tie is impossible — the
        // two markers differ at byte 4 ('r' vs '/'), so they can't start at the
        // same index.
        (Some(o), Some(c)) => {
            if o <= c {
                Some((o, READING_OPEN.len(), Register::Reading))
            } else {
                Some((c, READING_CLOSE.len(), Register::Quick))
            }
        }
    }
}

/// Length (in bytes) of the longest suffix of `s` that is a proper prefix of
/// either marker — the tail we must hold back in case the rest of the marker is
/// still coming. Returns 0 when no suffix could begin a marker.
fn trailing_marker_prefix_len(s: &str) -> usize {
    let bytes = s.as_bytes();
    let max = READING_CLOSE.len().max(READING_OPEN.len()).min(bytes.len());
    // Longest first: the largest partial match is the one to preserve.
    for k in (1..=max).rev() {
        let suffix = &bytes[bytes.len() - k..];
        if READING_OPEN.as_bytes().starts_with(suffix)
            || READING_CLOSE.as_bytes().starts_with(suffix)
        {
            return k;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drives a whole sequence of deltas through the parser and returns the
    /// emitted `(text, register)` runs, flushing at the end.
    fn run(chunks: &[&str]) -> Vec<(String, Register)> {
        let mut parser = RegisterParser::default();
        let mut out = Vec::new();
        for c in chunks {
            parser.push(c, &mut |t, r| out.push((t, r)));
        }
        parser.flush(&mut |t, r| out.push((t, r)));
        out
    }

    /// The concatenation of everything emitted — used to assert no marker ever
    /// leaks, regardless of how runs were split.
    fn joined(runs: &[(String, Register)]) -> String {
        runs.iter().map(|(t, _)| t.as_str()).collect()
    }

    #[test]
    fn plain_text_is_all_quick_and_unchanged() {
        let runs = run(&["Say more about that."]);
        assert_eq!(
            runs,
            vec![("Say more about that.".to_string(), Register::Quick)]
        );
    }

    #[test]
    fn a_reading_passage_is_split_and_markers_stripped() {
        let runs =
            run(&["A quick nudge. <!--reading-->A measured lesson.<!--/reading--> Back to quick."]);
        assert_eq!(
            runs,
            vec![
                ("A quick nudge. ".to_string(), Register::Quick),
                ("A measured lesson.".to_string(), Register::Reading),
                (" Back to quick.".to_string(), Register::Quick),
            ]
        );
        assert!(!joined(&runs).contains("<!--"), "no marker fragment leaks");
    }

    #[test]
    fn open_marker_at_the_very_start() {
        let runs = run(&["<!--reading-->Lesson only.<!--/reading-->"]);
        assert_eq!(runs, vec![("Lesson only.".to_string(), Register::Reading)]);
    }

    #[test]
    fn an_unclosed_reading_passage_stays_reading_through_flush() {
        let runs = run(&["Quick. <!--reading-->Lesson with no close"]);
        assert_eq!(
            runs,
            vec![
                ("Quick. ".to_string(), Register::Quick),
                ("Lesson with no close".to_string(), Register::Reading),
            ]
        );
    }

    #[test]
    fn a_marker_split_across_two_deltas_never_leaks() {
        // The open marker arrives byte-by-byte across many deltas.
        let chunks = [
            "before ", "<!--rea", "ding-->", "lesson", "<!--/rea", "ding-->", " after",
        ];
        let runs = run(&chunks);
        assert_eq!(
            runs,
            vec![
                ("before ".to_string(), Register::Quick),
                ("lesson".to_string(), Register::Reading),
                (" after".to_string(), Register::Quick),
            ]
        );
        assert!(
            !joined(&runs).contains('<'),
            "no marker byte leaks mid-stream"
        );
    }

    #[test]
    fn marker_split_one_byte_at_a_time_never_leaks() {
        let marker: Vec<String> = READING_OPEN.chars().map(|c| c.to_string()).collect();
        let mut chunks: Vec<&str> = vec!["x"];
        for m in &marker {
            chunks.push(m);
        }
        chunks.push("y");
        let runs = run(&chunks);
        assert_eq!(joined(&runs), "xy");
        // 'x' before the marker is quick; 'y' after is reading.
        assert_eq!(runs.last().unwrap().1, Register::Reading);
        assert!(!joined(&runs).contains("<!"));
    }

    #[test]
    fn a_false_marker_prefix_that_never_completes_is_emitted_verbatim() {
        // "<!--" appears but is never a real marker — it must survive as text,
        // not be swallowed.
        let runs = run(&["a <!-- just a comment b"]);
        assert_eq!(joined(&runs), "a <!-- just a comment b");
        assert!(runs.iter().all(|(_, r)| *r == Register::Quick));
    }

    #[test]
    fn back_to_back_passages() {
        let runs = run(&["<!--reading-->one<!--/reading--><!--reading-->two<!--/reading-->"]);
        assert_eq!(
            runs,
            vec![
                ("one".to_string(), Register::Reading),
                ("two".to_string(), Register::Reading),
            ]
        );
    }
}
