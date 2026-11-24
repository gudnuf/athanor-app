//! Text-driven Bellows: Rust owns endpointing and finalization for a platform
//! recognizer (Apple speech) that hands us cumulative text. Pure and hermetic.

mod committer;
pub(crate) use committer::Committer;

use crate::{EndpointConfig, Endpointer};

#[derive(Clone, Debug)]
pub struct TextBellowsConfig {
    pub sample_rate: u32,              // 16_000, same as SttConfig
    pub endpoint: EndpointConfig,      // EndpointConfig { silence_ms: 800, ..Default::default() }
    pub final_timeout_ms: u64,         // 600: max wait for a final after a cycle
    pub rotate_soft_ms: u64, // 40_000: rotate if the request is this old AND the endpointer is Idle
    pub rotate_hard_ms: u64, // 55_000: rotate regardless (hedge against an UNVERIFIED ~60 s cap)
    pub agree_n: usize,      // 2: LocalAgreement-2 over consecutive partials (must be >= 2)
    pub holdback_words: usize, // 1: never commit the newest agreed word from a partial
    pub max_contextual_strings: usize, // 100
}

#[derive(Clone, Debug, PartialEq)]
pub enum TextBellowsEvent {
    Preview(String),   // emitted only when changed; "" clears it
    Finalized(String), // append-only words, space-joined
    UtteranceEnded,
    CycleRequest { request_id: u64 }, // Swift: start a new request, swap it in, then endAudio() this one
    Unavailable, // terminal: recognizer keeps self-closing with no words (R4); Swift -> .permissionDenied
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextBellowsMetrics {
    pub pickup_ms: u64, // wall ms: utterance onset (Idle -> non-Idle) -> first non-empty partial
    pub latch_ms: u64, // audio ms: Endpointer::last_latency_ms at the last latched turn end (0 for send_now)
    pub finalize_ms: u64, // wall ms: turn-ending cycle issued -> final committed (or timeout)
    pub requests: u64, // begin_request count
    pub timeouts: u64, // closes forced by final_timeout_ms
    pub self_closes: u64, // final/error arriving on the ACTIVE request (the recognizer ended it itself)
    pub revisions_dropped: u64, // recognizer revised an already-committed word (committed form kept)
    pub resets: u64,            // hypothesis restarted mid-request (see the committer)
    pub empty_turns: u64, // turns that latched with zero committed words: dropped, no UtteranceEnded
}

enum Status {
    Active,
    Closing { since_ms: u64, ends_turn: bool },
    Closed,
}

struct Request {
    id: u64,
    committer: Committer,
    /// Only `Finalized` and `UtteranceEnded` live here.
    outbox: Vec<TextBellowsEvent>,
    /// Words (alphanumeric content) this request has committed.
    words: usize,
    started_ms: u64,
    status: Status,
}

/// Rust-owned turn machine for a text-producing recognizer.
pub struct TextBellows {
    cfg: TextBellowsConfig,
    endpointer: Endpointer,
    bias_terms: Vec<String>,
    requests: Vec<Request>,
    next_id: u64,
    /// `CycleRequest` / `Unavailable`: drained first.
    control: Vec<TextBellowsEvent>,
    /// `UtteranceEnded` raised when no request exists yet.
    pre_outbox: Vec<TextBellowsEvent>,
    metrics: TextBellowsMetrics,
    last_preview: String,
    onset_ms: Option<u64>,
    empty_self_closes: u32,
    halted: bool,
    /// Words committed by already-closed requests of the turn in progress.
    turn_words: usize,
}

fn words_of(text: &str) -> Vec<String> {
    text.split_whitespace().map(String::from).collect()
}

impl TextBellows {
    pub fn default_config() -> TextBellowsConfig {
        TextBellowsConfig {
            sample_rate: 16_000,
            endpoint: EndpointConfig {
                silence_ms: 800,
                ..Default::default()
            },
            final_timeout_ms: 600,
            rotate_soft_ms: 40_000,
            rotate_hard_ms: 55_000,
            agree_n: 2,
            holdback_words: 1,
            max_contextual_strings: 100,
        }
    }

    pub fn new(cfg: TextBellowsConfig, bias_terms: &[String]) -> Self {
        assert!(cfg.agree_n >= 2, "agree_n must be >= 2");
        let endpointer = Endpointer::new(cfg.sample_rate, cfg.endpoint.clone());
        Self {
            cfg,
            endpointer,
            bias_terms: bias_terms.to_vec(),
            requests: Vec::new(),
            next_id: 1,
            control: Vec::new(),
            pre_outbox: Vec::new(),
            metrics: TextBellowsMetrics::default(),
            last_preview: String::new(),
            onset_ms: None,
            empty_self_closes: 0,
            halted: false,
            turn_words: 0,
        }
    }

    /// Trimmed, non-empty, de-duplicated (first wins), capped.
    pub fn contextual_strings(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for t in &self.bias_terms {
            let t = t.trim();
            if t.is_empty() || out.iter().any(|o| o == t) {
                continue;
            }
            out.push(t.to_string());
            if out.len() >= self.cfg.max_contextual_strings {
                break;
            }
        }
        out
    }

    /// Start a new Active request. Returns 0 once halted (R4).
    pub fn begin_request(&mut self, now_ms: u64) -> u64 {
        if self.halted {
            return 0;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.requests.push(Request {
            id,
            committer: Committer::new(self.cfg.agree_n, self.cfg.holdback_words),
            outbox: Vec::new(),
            words: 0,
            started_ms: now_ms,
            status: Status::Active,
        });
        self.metrics.requests += 1;
        id
    }

    /// Push a `Finalized` into request `i`'s outbox, counting its real words.
    fn commit(&mut self, i: usize, text: String) {
        let n = text
            .split_whitespace()
            .filter(|w| w.chars().any(char::is_alphanumeric))
            .count();
        if n == 0 {
            return; // punctuation-only / blank text never reaches the transcript
        }
        self.requests[i].words += n;
        self.requests[i]
            .outbox
            .push(TextBellowsEvent::Finalized(text));
    }

    /// A turn boundary where the newest request already closed / none exists:
    /// emit `UtteranceEnded` only if the turn actually had words.
    fn end_turn_without_request(&mut self, into: Option<usize>) {
        let had_words = std::mem::take(&mut self.turn_words) > 0;
        match (had_words, into) {
            (true, Some(i)) => self.requests[i]
                .outbox
                .push(TextBellowsEvent::UtteranceEnded),
            (true, None) => self.pre_outbox.push(TextBellowsEvent::UtteranceEnded),
            (false, _) => self.metrics.empty_turns += 1,
        }
    }

    fn index_of(&self, id: u64) -> Option<usize> {
        self.requests.iter().position(|r| r.id == id)
    }

    pub fn push_pcm(&mut self, pcm: &[f32], now_ms: u64) {
        if self.halted {
            return;
        }
        let was = self.endpointer.in_utterance();
        let fired = self.endpointer.push(pcm);
        if !was && (self.endpointer.in_utterance() || fired) {
            self.onset_ms = Some(now_ms);
        }
        if fired {
            self.metrics.latch_ms = self.endpointer.last_latency_ms();
            self.end_turn(now_ms);
        }
    }

    pub fn send_now(&mut self, now_ms: u64) {
        if self.halted {
            return;
        }
        self.metrics.latch_ms = 0;
        self.endpointer.reset();
        self.end_turn(now_ms);
    }

    /// Mute/bank: clears the endpointer only.
    pub fn reset_endpoint(&mut self) {
        self.endpointer.reset();
    }

    fn end_turn(&mut self, now_ms: u64) {
        if let Some(r) = self
            .requests
            .iter_mut()
            .find(|r| matches!(r.status, Status::Active))
        {
            r.status = Status::Closing {
                since_ms: now_ms,
                ends_turn: true,
            };
            let id = r.id;
            self.control
                .push(TextBellowsEvent::CycleRequest { request_id: id });
            return;
        }
        match self.requests.last_mut() {
            Some(r) => match r.status {
                Status::Closing { .. } => {
                    r.status = Status::Closing {
                        since_ms: now_ms,
                        ends_turn: true,
                    }
                }
                Status::Closed => {
                    let i = self.requests.len() - 1;
                    self.end_turn_without_request(Some(i));
                }
                Status::Active => unreachable!("active handled above"),
            },
            None => self.end_turn_without_request(None),
        }
    }

    pub fn ingest_partial(&mut self, request_id: u64, text: &str, now_ms: u64) {
        if self.halted {
            return;
        }
        let Some(i) = self.index_of(request_id) else {
            return;
        };
        if matches!(self.requests[i].status, Status::Closed) {
            return;
        }
        let words = words_of(text);
        let (fin, _) = self.requests[i].committer.on_partial(&words);
        if let Some(f) = fin {
            self.commit(i, f);
        }
        if !words.is_empty() {
            self.empty_self_closes = 0;
            if let Some(onset) = self.onset_ms.take() {
                self.metrics.pickup_ms = now_ms.saturating_sub(onset);
            }
        }
    }

    pub fn ingest_final(&mut self, request_id: u64, text: &str, now_ms: u64) {
        let words = words_of(text);
        self.close_with(request_id, words, now_ms);
    }

    pub fn ingest_error(&mut self, request_id: u64, now_ms: u64) {
        let Some(i) = self.index_of(request_id) else {
            return;
        };
        let words = self.requests[i].committer.last_hypothesis().to_vec();
        self.close_with(request_id, words, now_ms);
    }

    fn close_with(&mut self, request_id: u64, words: Vec<String>, now_ms: u64) {
        if self.halted {
            return;
        }
        let Some(i) = self.index_of(request_id) else {
            return;
        };
        match self.requests[i].status {
            Status::Closed => {}
            Status::Active => {
                let rem = self.requests[i].committer.on_final(&words);
                if let Some(r) = rem {
                    self.commit(i, r);
                }
                self.requests[i].status = Status::Closed;
                self.turn_words += self.requests[i].words;
                self.metrics.self_closes += 1;
                if self.requests[i].committer.has_seen_words() {
                    self.empty_self_closes = 0;
                } else {
                    self.empty_self_closes += 1;
                }
                if self.empty_self_closes >= 3 {
                    self.control.push(TextBellowsEvent::Unavailable);
                    self.halted = true;
                } else {
                    self.control
                        .push(TextBellowsEvent::CycleRequest { request_id });
                }
            }
            Status::Closing {
                since_ms,
                ends_turn,
            } => {
                if let Some(r) = self.requests[i].committer.on_final(&words) {
                    self.commit(i, r);
                }
                if !words.is_empty() {
                    self.empty_self_closes = 0;
                }
                self.finish_closing(i, since_ms, ends_turn, now_ms);
            }
        }
    }

    fn finish_closing(&mut self, i: usize, since_ms: u64, ends_turn: bool, now_ms: u64) {
        self.requests[i].status = Status::Closed;
        if !ends_turn {
            self.turn_words += self.requests[i].words;
            return;
        }
        let total = std::mem::take(&mut self.turn_words) + self.requests[i].words;
        if total > 0 {
            self.requests[i]
                .outbox
                .push(TextBellowsEvent::UtteranceEnded);
            self.metrics.finalize_ms = now_ms.saturating_sub(since_ms);
        } else {
            self.metrics.empty_turns += 1;
        }
    }

    pub fn tick(&mut self, now_ms: u64) {
        if self.halted {
            return;
        }
        for i in 0..self.requests.len() {
            match self.requests[i].status {
                Status::Closing {
                    since_ms,
                    ends_turn,
                } if now_ms.saturating_sub(since_ms) >= self.cfg.final_timeout_ms => {
                    let hyp = self.requests[i].committer.last_hypothesis().to_vec();
                    if let Some(r) = self.requests[i].committer.on_final(&hyp) {
                        self.commit(i, r);
                    }
                    self.metrics.timeouts += 1;
                    self.finish_closing(i, since_ms, ends_turn, now_ms);
                }
                Status::Active => {
                    let age = now_ms.saturating_sub(self.requests[i].started_ms);
                    let soft = age >= self.cfg.rotate_soft_ms && !self.endpointer.in_utterance();
                    let hard = age >= self.cfg.rotate_hard_ms;
                    if soft || hard {
                        let id = self.requests[i].id;
                        self.requests[i].status = Status::Closing {
                            since_ms: now_ms,
                            ends_turn: false,
                        };
                        self.control
                            .push(TextBellowsEvent::CycleRequest { request_id: id });
                    }
                }
                _ => {}
            }
        }
    }

    pub fn drain(&mut self) -> Vec<TextBellowsEvent> {
        let mut out: Vec<TextBellowsEvent> = std::mem::take(&mut self.control);
        out.append(&mut self.pre_outbox);
        for r in self.requests.iter_mut() {
            out.append(&mut r.outbox);
            if !matches!(r.status, Status::Closed) {
                break;
            }
        }
        let preview = self.compute_preview();
        if preview != self.last_preview {
            self.last_preview = preview.clone();
            out.push(TextBellowsEvent::Preview(preview));
        }
        out
    }

    fn compute_preview(&self) -> String {
        let Some(first) = self
            .requests
            .iter()
            .position(|r| !matches!(r.status, Status::Closed))
        else {
            return String::new();
        };
        let mut parts: Vec<String> = vec![self.requests[first].committer.preview().to_string()];
        for r in &self.requests[first + 1..] {
            for e in &r.outbox {
                if let TextBellowsEvent::Finalized(t) = e {
                    parts.push(t.clone());
                }
            }
            parts.push(r.committer.preview().to_string());
        }
        parts
            .into_iter()
            .filter(|p| !p.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn metrics(&self) -> TextBellowsMetrics {
        let mut m = self.metrics.clone();
        m.revisions_dropped = self
            .requests
            .iter()
            .map(|r| r.committer.revisions_dropped())
            .sum();
        m.resets = self.requests.iter().map(|r| r.committer.resets()).sum();
        m
    }
}

#[cfg(test)]
mod turn_tests {
    use super::*;
    use TextBellowsEvent::*;

    fn sine(n: usize, freq: f32, amp: f32) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * std::f32::consts::PI * freq * i as f32 / 16_000.0).sin())
            .collect()
    }
    fn voiced(b: &mut TextBellows, n: usize, now: &mut u64) {
        for _ in 0..n {
            *now += 30;
            b.push_pcm(&sine(480, 440.0, 0.5), *now);
        }
    }
    fn silent(b: &mut TextBellows, n: usize, now: &mut u64) {
        for _ in 0..n {
            *now += 30;
            b.push_pcm(&[0.0; 480], *now);
        }
    }
    fn drain_no_preview(b: &mut TextBellows) -> Vec<TextBellowsEvent> {
        b.drain()
            .into_iter()
            .filter(|e| !matches!(e, Preview(_)))
            .collect()
    }
    fn fresh() -> TextBellows {
        TextBellows::new(TextBellows::default_config(), &[])
    }

    #[test]
    fn latch_cycles_request_then_final_ends_utterance() {
        let mut b = fresh();
        let mut now = 0;
        assert_eq!(b.begin_request(now), 1);
        voiced(&mut b, 12, &mut now);
        b.ingest_partial(1, "the salt", now);
        b.ingest_partial(1, "the salt is", now);
        assert_eq!(drain_no_preview(&mut b), vec![Finalized("the".into())]);
        silent(&mut b, 26, &mut now);
        assert_eq!(drain_no_preview(&mut b), vec![]);
        silent(&mut b, 1, &mut now);
        assert_eq!(
            b.drain(),
            vec![CycleRequest { request_id: 1 }],
            "preview unchanged, not re-emitted"
        );
        assert_eq!(b.begin_request(now), 2);
        b.ingest_final(1, "The salt is fixed.", now + 180);
        assert_eq!(
            b.drain(),
            vec![
                Finalized("salt is fixed.".into()),
                UtteranceEnded,
                Preview("".into())
            ]
        );
        let m = b.metrics();
        assert_eq!(
            (m.latch_ms, m.finalize_ms, m.requests, m.pickup_ms),
            (810, 180, 2, 330)
        );
    }

    /// begin request 1, speak, two partials, latch. Returns the latch time.
    fn latched(b: &mut TextBellows, now: &mut u64) -> u64 {
        assert_eq!(b.begin_request(*now), 1);
        voiced(b, 12, now);
        b.ingest_partial(1, "the salt", *now);
        b.ingest_partial(1, "the salt is", *now);
        assert_eq!(drain_no_preview(b), vec![Finalized("the".into())]);
        silent(b, 27, now);
        assert_eq!(drain_no_preview(b), vec![CycleRequest { request_id: 1 }]);
        *now
    }

    #[test]
    fn final_timeout_commits_last_partial_and_ignores_late_final() {
        let mut b = fresh();
        let mut now = 0;
        let since = latched(&mut b, &mut now);
        b.begin_request(since);
        b.tick(since + 599);
        assert_eq!(drain_no_preview(&mut b), vec![]);
        b.tick(since + 600);
        assert_eq!(
            drain_no_preview(&mut b),
            vec![Finalized("salt is".into()), UtteranceEnded]
        );
        assert_eq!(b.metrics().timeouts, 1);
        b.ingest_final(1, "The salt is fixed.", since + 700);
        assert_eq!(drain_no_preview(&mut b), vec![]);
    }

    #[test]
    fn send_now_cycles_without_silence_and_zeroes_latch() {
        let mut b = fresh();
        let mut now = 0;
        latched(&mut b, &mut now);
        assert_eq!(b.begin_request(now), 2);
        b.ingest_final(1, "The salt is fixed.", now + 100);
        assert_eq!(
            drain_no_preview(&mut b),
            vec![Finalized("salt is fixed.".into()), UtteranceEnded]
        );
        assert_eq!(b.metrics().latch_ms, 810);
        voiced(&mut b, 12, &mut now);
        b.ingest_partial(2, "next one", now);
        b.send_now(now);
        assert_eq!(
            drain_no_preview(&mut b),
            vec![CycleRequest { request_id: 2 }]
        );
        b.ingest_final(2, "next one now", now + 50);
        assert_eq!(
            drain_no_preview(&mut b),
            vec![Finalized("next one now".into()), UtteranceEnded]
        );
        assert_eq!(b.metrics().latch_ms, 0);
    }

    #[test]
    fn next_request_words_wait_behind_closing_request() {
        let mut b = fresh();
        let mut now = 0;
        latched(&mut b, &mut now);
        assert_eq!(b.begin_request(now), 2);
        b.ingest_partial(2, "next words now", now);
        b.ingest_partial(2, "next words now", now);
        assert_eq!(drain_no_preview(&mut b), vec![]);
        b.ingest_final(1, "The salt is fixed.", now + 100);
        assert_eq!(
            drain_no_preview(&mut b),
            vec![
                Finalized("salt is fixed.".into()),
                UtteranceEnded,
                Finalized("next words".into())
            ]
        );
    }

    #[test]
    fn soft_rotation_only_when_idle_and_does_not_end_utterance() {
        let mut b = fresh();
        let mut now = 0;
        b.begin_request(0);
        voiced(&mut b, 3, &mut now);
        silent(&mut b, 27, &mut now);
        b.ingest_partial(1, "hm", now);
        b.tick(39_999);
        assert_eq!(drain_no_preview(&mut b), vec![]);
        b.tick(40_000);
        assert_eq!(
            drain_no_preview(&mut b),
            vec![CycleRequest { request_id: 1 }]
        );
        b.ingest_final(1, "hm", 40_100);
        assert_eq!(drain_no_preview(&mut b), vec![Finalized("hm".into())]);
    }

    #[test]
    fn hard_rotation_while_in_utterance() {
        let mut b = fresh();
        let mut now = 0;
        b.begin_request(0);
        voiced(&mut b, 12, &mut now);
        silent(&mut b, 3, &mut now);
        b.tick(40_000);
        assert_eq!(drain_no_preview(&mut b), vec![]);
        b.tick(55_000);
        assert_eq!(
            drain_no_preview(&mut b),
            vec![CycleRequest { request_id: 1 }]
        );
    }

    #[test]
    fn stale_request_events_are_ignored() {
        let mut b = fresh();
        let mut now = 0;
        let since = latched(&mut b, &mut now);
        b.ingest_partial(99, "ghost words", since);
        b.ingest_final(99, "ghost words", since);
        assert_eq!(drain_no_preview(&mut b), vec![]);
        b.begin_request(since);
        b.ingest_final(1, "The salt is fixed.", since + 10);
        drain_no_preview(&mut b);
        b.ingest_partial(1, "late partial words", since + 20);
        b.ingest_final(1, "late final", since + 30);
        assert_eq!(drain_no_preview(&mut b), vec![]);
    }

    #[test]
    fn reset_endpoint_clears_pending_latch() {
        let mut b = fresh();
        let mut now = 0;
        b.begin_request(0);
        voiced(&mut b, 12, &mut now);
        silent(&mut b, 20, &mut now);
        b.reset_endpoint();
        silent(&mut b, 10, &mut now);
        assert_eq!(drain_no_preview(&mut b), vec![]);
    }

    #[test]
    fn pickup_latency_measures_onset_to_first_partial() {
        let mut b = fresh();
        let mut now = 0;
        b.begin_request(0);
        voiced(&mut b, 1, &mut now);
        assert_eq!(now, 30);
        b.ingest_partial(1, "the", 250);
        assert_eq!(b.metrics().pickup_ms, 220);
    }

    #[test]
    fn contextual_strings_cap_dedupe_trim() {
        let mut terms: Vec<String> = vec!["  ".into(), " salt ".into()];
        terms.extend((0..146).map(|i| format!("term{i}")));
        terms.extend(["salt".into(), "term3".into()]);
        assert_eq!(terms.len(), 150);
        let b = TextBellows::new(TextBellows::default_config(), &terms);
        let cs = b.contextual_strings();
        assert_eq!(cs.len(), 100);
        assert_eq!(cs[0], "salt");
        assert_eq!(cs[1], "term0");
        assert_eq!(cs[99], "term98");
        assert_eq!(cs.iter().filter(|s| *s == "salt").count(), 1);
        assert!(cs.iter().all(|s| !s.is_empty() && s.trim() == s));
    }

    #[test]
    fn preview_concatenates_closing_tail_held_words_and_active() {
        let mut b = fresh();
        let mut now = 0;
        b.begin_request(0);
        voiced(&mut b, 12, &mut now);
        b.ingest_partial(1, "the salt is", now);
        b.ingest_partial(1, "the salt is fixed", now);
        silent(&mut b, 27, &mut now);
        drain_no_preview(&mut b);
        b.begin_request(now);
        b.ingest_partial(2, "next words", now);
        b.ingest_partial(2, "next words", now);
        let ev = b.drain();
        let last_preview = ev.iter().rev().find_map(|e| match e {
            Preview(s) => Some(s.clone()),
            _ => None,
        });
        assert_eq!(last_preview, Some("is fixed next words".to_string()));
    }

    #[test]
    fn active_request_final_cycles_without_ending_turn() {
        let mut b = fresh();
        let mut now = 0;
        b.begin_request(0);
        voiced(&mut b, 12, &mut now);
        b.ingest_partial(1, "the salt", now);
        b.ingest_partial(1, "the salt is", now);
        assert_eq!(drain_no_preview(&mut b), vec![Finalized("the".into())]);
        b.ingest_final(1, "the salt is", now);
        assert_eq!(
            drain_no_preview(&mut b),
            vec![CycleRequest { request_id: 1 }, Finalized("salt is".into())]
        );
        assert_eq!(b.metrics().self_closes, 1);
        assert_eq!(b.begin_request(now), 2);
        silent(&mut b, 27, &mut now);
        assert_eq!(
            drain_no_preview(&mut b),
            vec![CycleRequest { request_id: 2 }]
        );
        b.ingest_final(2, "more", now + 10);
        assert_eq!(
            drain_no_preview(&mut b),
            vec![Finalized("more".into()), UtteranceEnded]
        );
    }

    #[test]
    fn active_request_error_cycles_to_fresh_request() {
        let mut b = fresh();
        let mut now = 0;
        b.begin_request(0);
        voiced(&mut b, 12, &mut now);
        b.ingest_partial(1, "we tend the fire", now);
        b.ingest_partial(1, "we tend the fire", now);
        assert_eq!(
            drain_no_preview(&mut b),
            vec![Finalized("we tend the".into())]
        );
        b.ingest_error(1, now);
        assert_eq!(
            drain_no_preview(&mut b),
            vec![CycleRequest { request_id: 1 }, Finalized("fire".into())]
        );
        assert_eq!(b.begin_request(now), 2);
        b.ingest_partial(2, "and then", now);
        b.ingest_partial(2, "and then some", now);
        assert_eq!(drain_no_preview(&mut b), vec![Finalized("and".into())]);
    }

    #[test]
    fn latch_with_no_active_request_marks_newest_closing() {
        let mut b = fresh();
        let mut now = 40_000;
        b.begin_request(0);
        b.ingest_partial(1, "hm", now);
        b.tick(now);
        assert_eq!(
            drain_no_preview(&mut b),
            vec![CycleRequest { request_id: 1 }]
        );
        voiced(&mut b, 12, &mut now);
        silent(&mut b, 27, &mut now);
        assert_eq!(
            drain_no_preview(&mut b),
            vec![],
            "no active request to cycle"
        );
        b.ingest_final(1, "hm", now + 100);
        assert_eq!(
            drain_no_preview(&mut b),
            vec![Finalized("hm".into()), UtteranceEnded]
        );
    }

    #[test]
    fn pickup_not_rearmed_by_interword_dip() {
        let mut b = fresh();
        let mut now = 0;
        b.begin_request(0);
        voiced(&mut b, 1, &mut now);
        b.ingest_partial(1, "the", 250);
        assert_eq!(b.metrics().pickup_ms, 220);
        now = 250;
        silent(&mut b, 3, &mut now);
        voiced(&mut b, 1, &mut now);
        b.ingest_partial(1, "the salt", now + 100);
        assert_eq!(b.metrics().pickup_ms, 220);
    }

    #[test]
    fn self_close_storm_halts_to_unavailable() {
        let mut b = fresh();
        assert_eq!(b.begin_request(0), 1);
        b.ingest_error(1, 0);
        assert_eq!(
            drain_no_preview(&mut b),
            vec![CycleRequest { request_id: 1 }]
        );
        assert_eq!(b.begin_request(0), 2);
        b.ingest_error(2, 0);
        assert_eq!(
            drain_no_preview(&mut b),
            vec![CycleRequest { request_id: 2 }]
        );
        assert_eq!(b.begin_request(0), 3);
        b.ingest_error(3, 0);
        assert_eq!(drain_no_preview(&mut b), vec![Unavailable]);
        assert_eq!(b.metrics().self_closes, 3);
        assert_eq!(b.begin_request(0), 0);
        b.ingest_partial(3, "late", 0);
        assert_eq!(drain_no_preview(&mut b), vec![]);

        // streak resets when a request heard words
        let mut b = fresh();
        b.begin_request(0);
        b.ingest_error(1, 0);
        b.begin_request(0);
        b.ingest_error(2, 0);
        drain_no_preview(&mut b);
        assert_eq!(b.begin_request(0), 3);
        b.ingest_partial(3, "hm", 0);
        b.ingest_error(3, 0);
        assert_eq!(
            drain_no_preview(&mut b),
            vec![CycleRequest { request_id: 3 }, Finalized("hm".into())]
        );
    }

    #[test]
    fn silence_only_turn_is_dropped_and_counted() {
        let mut b = fresh();
        let mut now = 0;
        b.begin_request(0);
        voiced(&mut b, 12, &mut now); // energy, but the recognizer hears no words
        silent(&mut b, 27, &mut now);
        assert_eq!(
            drain_no_preview(&mut b),
            vec![CycleRequest { request_id: 1 }]
        );
        b.begin_request(now);
        b.ingest_final(1, "  ", now + 40);
        assert_eq!(drain_no_preview(&mut b), vec![], "no UtteranceEnded");
        assert_eq!(b.metrics().empty_turns, 1);
        // the machine still works: a real turn on request 2 ends normally
        voiced(&mut b, 12, &mut now);
        b.ingest_partial(2, "hello there", now);
        b.ingest_partial(2, "hello there now", now);
        silent(&mut b, 27, &mut now);
        b.begin_request(now);
        b.ingest_final(2, "Hello there now.", now + 50);
        let ev = drain_no_preview(&mut b);
        assert!(ev.contains(&UtteranceEnded), "{ev:?}");
        assert_eq!(b.metrics().empty_turns, 1);
    }

    #[test]
    fn punctuation_only_and_timed_out_empty_turns_are_dropped() {
        let mut b = fresh();
        let mut now = 0;
        b.begin_request(0);
        voiced(&mut b, 12, &mut now);
        b.ingest_partial(1, "—", now);
        silent(&mut b, 27, &mut now);
        drain_no_preview(&mut b);
        b.begin_request(now);
        b.tick(now + 600); // no final ever arrives
        assert_eq!(drain_no_preview(&mut b), vec![]);
        let m = b.metrics();
        assert_eq!((m.empty_turns, m.timeouts), (1, 1));
    }

    #[test]
    fn words_before_a_rotation_still_count_for_the_turn() {
        let mut b = fresh();
        let mut now = 0;
        b.begin_request(0);
        b.ingest_partial(1, "we tend", now);
        b.ingest_partial(1, "we tend the", now);
        b.tick(40_000); // soft rotation closes request 1 (no turn end)
        drain_no_preview(&mut b);
        b.begin_request(40_000);
        b.ingest_final(1, "we tend the", 40_050);
        now = 40_050;
        voiced(&mut b, 12, &mut now);
        silent(&mut b, 27, &mut now);
        b.ingest_final(2, "", now + 10);
        let ev = drain_no_preview(&mut b);
        assert!(ev.contains(&UtteranceEnded), "{ev:?}");
        assert_eq!(b.metrics().empty_turns, 0);
    }
}
