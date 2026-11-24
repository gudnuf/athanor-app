//! The text-Bellows bridge: a thin UniFFI projection of `stt::TextBellows`
//! (Apple speech path). Rust owns endpointing and finalization; Swift only
//! forwards PCM and recognizer text and obeys `CycleRequest`. Hermetic: no feature gate,
//! no model, no network (invariant #3).
//!
//! Zero logic here, only projection plus a
//! monotonic clock so the core's `now_ms` parameters are supplied for Swift.

use std::sync::{Arc, Mutex};
use std::time::Instant;
use stt::{TextBellows, TextBellowsEvent, TextBellowsMetrics};

#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum FfiTextBellowsEvent {
    Preview { text: String },
    Finalized { text: String },
    UtteranceEnded,
    CycleRequest { request_id: u64 },
    Unavailable,
}

impl From<TextBellowsEvent> for FfiTextBellowsEvent {
    fn from(e: TextBellowsEvent) -> Self {
        match e {
            TextBellowsEvent::Preview(text) => Self::Preview { text },
            TextBellowsEvent::Finalized(text) => Self::Finalized { text },
            TextBellowsEvent::UtteranceEnded => Self::UtteranceEnded,
            TextBellowsEvent::CycleRequest { request_id } => Self::CycleRequest { request_id },
            TextBellowsEvent::Unavailable => Self::Unavailable,
        }
    }
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct FfiTextBellowsMetrics {
    pub pickup_ms: u64,
    pub latch_ms: u64,
    pub finalize_ms: u64,
    pub requests: u64,
    pub timeouts: u64,
    pub self_closes: u64,
    pub revisions_dropped: u64,
    pub resets: u64,
    pub empty_turns: u64,
}

impl From<TextBellowsMetrics> for FfiTextBellowsMetrics {
    fn from(m: TextBellowsMetrics) -> Self {
        Self {
            pickup_ms: m.pickup_ms,
            latch_ms: m.latch_ms,
            finalize_ms: m.finalize_ms,
            requests: m.requests,
            timeouts: m.timeouts,
            self_closes: m.self_closes,
            revisions_dropped: m.revisions_dropped,
            resets: m.resets,
            empty_turns: m.empty_turns,
        }
    }
}

#[derive(uniffi::Object)]
pub struct TextBellowsHandle {
    inner: Mutex<TextBellows>,
    clock: Box<dyn Fn() -> u64 + Send + Sync>,
}

impl TextBellowsHandle {
    fn with_clock(
        bias_terms: Vec<String>,
        silence_latch_ms: u64,
        clock: Box<dyn Fn() -> u64 + Send + Sync>,
    ) -> Self {
        let mut cfg = TextBellows::default_config();
        cfg.endpoint.silence_ms = silence_latch_ms;
        Self {
            inner: Mutex::new(TextBellows::new(cfg, &bias_terms)),
            clock,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, TextBellows> {
        // A poisoned lock only means an earlier panic; the state is still usable.
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }
}

#[uniffi::export]
impl TextBellowsHandle {
    #[uniffi::constructor]
    pub fn new(bias_terms: Vec<String>, silence_latch_ms: u64) -> Arc<Self> {
        let start = Instant::now();
        Arc::new(Self::with_clock(
            bias_terms,
            silence_latch_ms,
            Box::new(move || start.elapsed().as_millis() as u64),
        ))
    }

    pub fn contextual_strings(&self) -> Vec<String> {
        self.lock().contextual_strings()
    }

    pub fn begin_request(&self) -> u64 {
        let now = (self.clock)();
        self.lock().begin_request(now)
    }

    pub fn push_pcm(&self, pcm: Vec<f32>) {
        let now = (self.clock)();
        self.lock().push_pcm(&pcm, now);
    }

    pub fn ingest_partial(&self, request_id: u64, text: String) {
        let now = (self.clock)();
        self.lock().ingest_partial(request_id, &text, now);
    }

    pub fn ingest_final(&self, request_id: u64, text: String) {
        let now = (self.clock)();
        self.lock().ingest_final(request_id, &text, now);
    }

    pub fn ingest_error(&self, request_id: u64) {
        let now = (self.clock)();
        self.lock().ingest_error(request_id, now);
    }

    pub fn send_now(&self) {
        let now = (self.clock)();
        self.lock().send_now(now);
    }

    pub fn reset_endpoint(&self) {
        self.lock().reset_endpoint();
    }

    /// `tick(now)` then `drain()`.
    pub fn poll(&self) -> Vec<FfiTextBellowsEvent> {
        let now = (self.clock)();
        let mut b = self.lock();
        b.tick(now);
        b.drain().into_iter().map(Into::into).collect()
    }

    pub fn metrics(&self) -> FfiTextBellowsMetrics {
        self.lock().metrics().into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn sine(n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| 0.5 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 16_000.0).sin())
            .collect()
    }

    fn poll_no_preview(h: &TextBellowsHandle) -> Vec<FfiTextBellowsEvent> {
        h.poll()
            .into_iter()
            .filter(|e| !matches!(e, FfiTextBellowsEvent::Preview { .. }))
            .collect()
    }

    #[test]
    fn handle_round_trips_latch_cycle_final() {
        let clock = Arc::new(AtomicU64::new(0));
        let c = clock.clone();
        let h =
            TextBellowsHandle::with_clock(vec![], 800, Box::new(move || c.load(Ordering::SeqCst)));
        assert_eq!(h.begin_request(), 1);
        for _ in 0..12 {
            clock.fetch_add(30, Ordering::SeqCst);
            h.push_pcm(sine(480));
        }
        h.ingest_partial(1, "the salt".into());
        h.ingest_partial(1, "the salt is".into());
        assert_eq!(
            poll_no_preview(&h),
            vec![FfiTextBellowsEvent::Finalized { text: "the".into() }]
        );
        for _ in 0..27 {
            clock.fetch_add(30, Ordering::SeqCst);
            h.push_pcm(vec![0.0; 480]);
        }
        assert_eq!(
            poll_no_preview(&h),
            vec![FfiTextBellowsEvent::CycleRequest { request_id: 1 }]
        );
        assert_eq!(h.begin_request(), 2);
        clock.fetch_add(180, Ordering::SeqCst);
        h.ingest_final(1, "The salt is fixed.".into());
        assert_eq!(
            poll_no_preview(&h),
            vec![
                FfiTextBellowsEvent::Finalized {
                    text: "salt is fixed.".into()
                },
                FfiTextBellowsEvent::UtteranceEnded
            ]
        );
    }

    #[test]
    fn ffi_event_projects_core_event() {
        use TextBellowsEvent as C;
        let cases = vec![
            (
                C::Preview("a".into()),
                FfiTextBellowsEvent::Preview { text: "a".into() },
            ),
            (
                C::Finalized("b".into()),
                FfiTextBellowsEvent::Finalized { text: "b".into() },
            ),
            (C::UtteranceEnded, FfiTextBellowsEvent::UtteranceEnded),
            (
                C::CycleRequest { request_id: 7 },
                FfiTextBellowsEvent::CycleRequest { request_id: 7 },
            ),
            (C::Unavailable, FfiTextBellowsEvent::Unavailable),
        ];
        for (core, ffi) in cases {
            assert_eq!(FfiTextBellowsEvent::from(core), ffi);
        }
    }

    #[test]
    fn metrics_project() {
        let m = TextBellowsMetrics {
            pickup_ms: 1,
            latch_ms: 2,
            finalize_ms: 3,
            requests: 4,
            timeouts: 5,
            self_closes: 6,
            revisions_dropped: 7,
            resets: 8,
            empty_turns: 9,
        };
        let f = FfiTextBellowsMetrics::from(m);
        assert_eq!(
            (
                f.pickup_ms,
                f.latch_ms,
                f.finalize_ms,
                f.requests,
                f.timeouts,
                f.self_closes,
                f.revisions_dropped,
                f.resets,
                f.empty_turns
            ),
            (1, 2, 3, 4, 5, 6, 7, 8, 9)
        );
    }
}
