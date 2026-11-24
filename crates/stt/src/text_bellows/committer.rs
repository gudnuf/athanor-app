//! Append-only LocalAgreement committer over cumulative recognizer text.
//!
//! On-device speech delivers the *cumulative* best transcription per request,
//! and partials may revise earlier words. This turns that into an append-only
//! stream of words. Alignment is positional (accepted limit, plan Risk 6).

use std::collections::VecDeque;

/// Normalise a word for comparison: curly apostrophe -> `'`, lowercase, keep
/// only alphanumerics and `'`.
pub(crate) fn norm(w: &str) -> String {
    w.chars()
        .map(|c| if c == '\u{2019}' { '\'' } else { c })
        .flat_map(|c| c.to_lowercase())
        .filter(|c| c.is_alphanumeric() || *c == '\'')
        .collect()
}

pub(crate) struct Committer {
    agree_n: usize,
    holdback: usize,
    /// Norm'd words of the current lineage already emitted.
    committed: Vec<String>,
    /// Last `agree_n` hypotheses, as surface words.
    history: VecDeque<Vec<String>>,
    /// The last hypothesis (surface words).
    prev: Vec<String>,
    revisions_dropped: u64,
    resets: u64,
    done: bool,
    preview: String,
    seen_words: bool,
}

fn join(words: &[String]) -> String {
    words.join(" ")
}

fn some(s: String) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

impl Committer {
    pub(crate) fn new(agree_n: usize, holdback: usize) -> Self {
        Self {
            agree_n,
            holdback,
            committed: Vec::new(),
            history: VecDeque::new(),
            prev: Vec::new(),
            revisions_dropped: 0,
            resets: 0,
            done: false,
            preview: String::new(),
            seen_words: false,
        }
    }

    pub(crate) fn last_hypothesis(&self) -> &[String] {
        &self.prev
    }
    pub(crate) fn revisions_dropped(&self) -> u64 {
        self.revisions_dropped
    }
    pub(crate) fn resets(&self) -> u64 {
        self.resets
    }
    pub(crate) fn preview(&self) -> &str {
        &self.preview
    }
    /// True once any non-empty hypothesis has been seen (storm backoff, R4).
    pub(crate) fn has_seen_words(&self) -> bool {
        self.seen_words
    }

    /// Reset rule shared by partial and final: the hypothesis shrank and no
    /// committed slot lines up with the new words.
    fn is_reset(&self, new: &[String]) -> bool {
        let lc = self.committed.len();
        if lc == 0 || new.len() >= self.prev.len() {
            return false;
        }
        let n = new.len().min(lc);
        !(0..n).any(|i| norm(&new[i]) == self.committed[i])
    }

    fn has_revision(&self, new: &[String]) -> bool {
        let n = new.len().min(self.committed.len());
        (0..n).any(|i| norm(&new[i]) != self.committed[i])
    }

    /// Largest k such that norm(new[..k]) equals the last k words of the
    /// lineage (committed + held tail).
    fn overlap(lineage: &[String], new: &[String]) -> usize {
        let max = new.len().min(lineage.len());
        for k in (1..=max).rev() {
            let a: Vec<String> = new[..k].iter().map(|w| norm(w)).collect();
            if a[..] == lineage[lineage.len() - k..] {
                return k;
            }
        }
        0
    }

    /// Flush the held-back tail (prev beyond the commit point) and compute the
    /// lineage text used for overlap. Returns (flushed, lineage).
    fn flush_tail(&self) -> (Vec<String>, Vec<String>) {
        let from = self.committed.len().min(self.prev.len());
        let tail: Vec<String> = self.prev[from..].to_vec();
        let mut lineage = self.committed.clone();
        lineage.extend(tail.iter().map(|w| norm(w)));
        (tail, lineage)
    }

    pub(crate) fn on_partial(&mut self, words: &[String]) -> (Option<String>, String) {
        if self.done || words.is_empty() {
            return (None, self.preview.clone());
        }
        self.seen_words = true;
        if self.is_reset(words) {
            let (tail, lineage) = self.flush_tail();
            let k = Self::overlap(&lineage, words);
            self.committed = words[..k].iter().map(|w| norm(w)).collect();
            self.history.clear();
            self.history.push_back(words.to_vec());
            self.prev = words.to_vec();
            self.resets += 1;
            self.preview = join(&words[k..]);
            return (some(join(&tail)), self.preview.clone());
        }
        if self.has_revision(words) {
            self.revisions_dropped += 1;
        }
        self.history.push_back(words.to_vec());
        while self.history.len() > self.agree_n {
            self.history.pop_front();
        }
        let mut finalized = None;
        if self.history.len() == self.agree_n {
            let stable = self.stable_prefix();
            let upto = stable.saturating_sub(self.holdback);
            if upto > self.committed.len() {
                let from = self.committed.len();
                finalized = some(join(&words[from..upto]));
                self.committed
                    .extend(words[from..upto].iter().map(|w| norm(w)));
            }
        }
        self.prev = words.to_vec();
        let from = self.committed.len().min(words.len());
        self.preview = join(&words[from..]);
        (finalized, self.preview.clone())
    }

    fn stable_prefix(&self) -> usize {
        let first = &self.history[0];
        let mut n = first.len();
        for h in self.history.iter().skip(1) {
            let mut i = 0;
            while i < n && i < h.len() && norm(&first[i]) == norm(&h[i]) {
                i += 1;
            }
            n = i;
        }
        n
    }

    pub(crate) fn on_final(&mut self, words: &[String]) -> Option<String> {
        if self.done {
            return None;
        }
        if !words.is_empty() {
            self.seen_words = true;
        }
        let out = if self.is_reset(words) {
            let (tail, lineage) = self.flush_tail();
            let k = Self::overlap(&lineage, words);
            self.resets += 1;
            let mut parts = tail;
            parts.extend_from_slice(&words[k..]);
            some(join(&parts))
        } else {
            let from = if self.has_revision(words) {
                self.revisions_dropped += 1;
                let k = Self::overlap(&self.committed, words);
                if k > 0 {
                    k
                } else {
                    self.committed.len().min(words.len())
                }
            } else {
                self.committed.len().min(words.len())
            };
            some(join(&words[from..]))
        };
        self.prev = words.to_vec();
        self.preview.clear();
        self.done = true;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }
    fn c21() -> Committer {
        Committer::new(2, 1)
    }

    #[test]
    fn normalize_strips_case_and_punctuation() {
        assert_eq!(norm("Fixed."), "fixed");
        assert_eq!(norm("don't"), "don't");
        assert_eq!(norm("don’t"), "don't");
        assert_eq!(norm("—"), "");
    }
    #[test]
    fn local_agreement_commits_stable_prefix_with_holdback() {
        let mut c = c21();
        assert_eq!(c.on_partial(&w("the")), (None, "the".into()));
        assert_eq!(c.on_partial(&w("the salt")), (None, "the salt".into()));
        assert_eq!(
            c.on_partial(&w("the salt is")),
            (Some("the".into()), "salt is".into())
        );
        assert_eq!(
            c.on_partial(&w("the salt is fixed")),
            (Some("salt".into()), "is fixed".into())
        );
    }
    #[test]
    fn final_commits_only_the_uncommitted_remainder() {
        let mut c = c21();
        for p in ["the", "the salt", "the salt is", "the salt is fixed"] {
            c.on_partial(&w(p));
        }
        assert_eq!(
            c.on_final(&w("The salt is fixed.")),
            Some("is fixed.".into())
        );
        assert_eq!(
            c.on_final(&w("The salt is fixed.")),
            None,
            "done after final"
        );
    }
    #[test]
    fn revised_committed_word_is_never_rewritten() {
        let mut c = c21();
        for p in ["the", "the salt", "the salt is", "the salt is fixed"] {
            c.on_partial(&w(p));
        }
        assert_eq!(
            c.on_partial(&w("a salt is fixed")),
            (None, "is fixed".into()),
            "same length: revision, not reset"
        );
        assert_eq!(c.revisions_dropped(), 1);
        assert_eq!(c.resets(), 0);
    }
    #[test]
    fn reset_flushes_the_held_back_tail() {
        let mut c = c21();
        assert_eq!(
            c.on_partial(&w("we tend the")),
            (None, "we tend the".into())
        );
        assert_eq!(
            c.on_partial(&w("we tend the fire")),
            (Some("we tend".into()), "the fire".into())
        );
        assert_eq!(
            c.on_partial(&w("we tend the fire now")),
            (Some("the".into()), "fire now".into())
        );
        assert_eq!(
            c.on_partial(&w("burns")),
            (Some("fire now".into()), "burns".into()),
            "tail flushed on reset"
        );
        assert_eq!(c.resets(), 1);
        assert_eq!(
            c.on_final(&w("burns brightly")),
            Some("burns brightly".into())
        );
    }
    #[test]
    fn reset_detected_with_one_committed_word() {
        let mut c = c21();
        c.on_partial(&w("the salt"));
        assert_eq!(
            c.on_partial(&w("the salt is")),
            (Some("the".into()), "salt is".into())
        );
        assert_eq!(
            c.on_partial(&w("burns")),
            (Some("salt is".into()), "burns".into())
        );
        assert_eq!((c.resets(), c.revisions_dropped()), (1, 0));
    }
    #[test]
    fn filler_dropping_final_is_not_duplicated() {
        let mut c = c21();
        c.on_partial(&w("um so the"));
        assert_eq!(
            c.on_partial(&w("um so the fire")),
            (Some("um so".into()), "the fire".into())
        );
        assert_eq!(
            c.on_partial(&w("um so the fire")),
            (Some("the".into()), "fire".into())
        );
        // final drops "um": shrank 4 -> 3, no slot matches -> reset; flush "fire"; overlap k = 3 -> nothing new
        assert_eq!(c.on_final(&w("So the fire.")), Some("fire".into()));
    }
    #[test]
    fn filler_dropping_longer_final_keeps_every_word() {
        let mut c = c21();
        c.on_partial(&w("um so the"));
        assert_eq!(
            c.on_partial(&w("um so the")),
            (Some("um so".into()), "the".into())
        );
        // final 4 >= prev 3: no reset; slot 0 so != um: revision -> overlap k = 1 ([so]) -> words[1..]
        assert_eq!(
            c.on_final(&w("So the fire is.")),
            Some("the fire is.".into()),
            "\"the\" is not lost"
        );
        assert_eq!((c.revisions_dropped(), c.resets()), (1, 0));
    }
    #[test]
    fn slot_zero_revision_final_without_overlap_stays_positional() {
        let mut c = c21();
        for p in ["the", "the salt", "the salt is", "the salt is fixed"] {
            c.on_partial(&w(p));
        }
        assert_eq!(
            c.on_final(&w("A salt is fixed.")),
            Some("is fixed.".into()),
            "k = 0 -> positional, no duplicate \"salt\""
        );
    }
    #[test]
    fn empty_and_whitespace_partials_are_ignored() {
        let mut c = c21();
        c.on_partial(&w("the salt"));
        assert_eq!(c.on_partial(&w("")), (None, "the salt".into()));
        assert_eq!(c.on_partial(&w("   ")), (None, "the salt".into()));
    }
}
