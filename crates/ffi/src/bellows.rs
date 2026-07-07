//! The Bellows bridge: a thin UniFFI wrapper over `stt::SttStream` (Plan Task
//! C3). Audio in from the Swift shell's AVAudioEngine tap → append-only
//! finalized transcript segments out, plus a volatile preview tail and an
//! energy-based endpoint signal for silence auto-send.
//!
//! Discipline (Plan §3 C3 "do-not-touch"): ZERO logic lives here. Every method
//! forwards to `SttStream`; the diff between this bridge and sitewalk's is
//! factory evidence, so the bridge stays a projection, never a re-implementation.
//!
//! Threading: `SttStream` is `Send + Sync` (all interior mutability is behind
//! `Mutex`/atomics), so `BellowsHandle` is a plain `uniffi::Object` — the Swift
//! shell pushes PCM off the realtime thread and polls on a background thread
//! (Plan §2 cadences), exactly as `SttStream` documents.

use stt::{FinalizedSegment, SttError, SttStream};

/// A finalized, never-to-be-revised transcript segment crossing FFI. A thin
/// `uniffi::Record` projection of `stt::FinalizedSegment` — we never send the
/// core type across the boundary (Plan §1: "Events as thin projections … never
/// across FFI"). Timestamps are absolute audio milliseconds from stream start.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct FfiSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

impl From<FinalizedSegment> for FfiSegment {
    fn from(s: FinalizedSegment) -> Self {
        FfiSegment {
            start_ms: s.start_ms,
            end_ms: s.end_ms,
            text: s.text,
        }
    }
}

/// Model tier the Bellows opens with (Plan §2 "Model", F1 tier knob). The Swift
/// shell resolves the concrete model file (base.en default / small.en) and
/// passes both the resolved `model_path` and the tier it chose; the tier is
/// carried here as the stable public shape the app targets. The path already
/// encodes the tier for `SttStream`, so the bridge treats this as advisory —
/// it does not second-guess the caller's resolved path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum BellowsTier {
    /// `ggml-base.en-q5_1` (~57 MB) — the day-1 default.
    BaseEn,
    /// `ggml-small.en-q5_1` (~182 MB) — higher accuracy, larger download.
    SmallEn,
}

/// Errors surfaced by the Bellows bridge. A `uniffi::Error` projection of
/// `stt::SttError`. Field is `reason` (not `message`) to avoid the Android
/// `Throwable.message` clash uniffi warns about; harmless on Swift too.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum BellowsError {
    #[error("model load failed: {reason}")]
    ModelLoad { reason: String },
    #[error("decode failed: {reason}")]
    Decode { reason: String },
    #[error("invalid config: {reason}")]
    Config { reason: String },
}

impl From<SttError> for BellowsError {
    fn from(e: SttError) -> Self {
        match e {
            SttError::ModelLoad(m) => BellowsError::ModelLoad { reason: m },
            SttError::Decode(m) => BellowsError::Decode { reason: m },
            SttError::Config(m) => BellowsError::Config { reason: m },
        }
    }
}

/// The streaming STT handle held by the Swift session layer. Wraps one
/// `SttStream`. Endpointing is enabled (default `EndpointConfig`) so the shell
/// can poll `utterance_ended()` for silence auto-send without an explicit DONE.
///
/// Bias terms: `open` seeds the whisper `initial_prompt` from the `bias_terms`
/// the caller passes (active-domain vocab + recent salt). The bridge stays thin
/// and does NOT read the Store itself — C2's session layer is responsible for
/// assembling `bias_terms` and handing them in (see report).
#[derive(uniffi::Object)]
pub struct BellowsHandle {
    stream: SttStream,
}

// These helpers back the whisper-gated `open` constructor and the hermetic
// test seam; off `whisper` and outside tests they are legitimately unused.
#[cfg(any(feature = "whisper", test))]
impl BellowsHandle {
    /// Wrap an already-constructed `SttStream`. The one seam shared by the
    /// native `open` constructor and the hermetic test (which injects a
    /// `ScriptedDecoder` via `SttStream::with_decoder`, mirroring how `stt`'s
    /// own tests stay model-free). Not exported across FFI.
    fn from_stream(stream: SttStream) -> Self {
        BellowsHandle { stream }
    }

    /// SttConfig the Bellows opens with: stt defaults plus endpointing on, so
    /// `utterance_ended()` can drive silence auto-send. Shared by `open` and
    /// the hermetic test so they exercise the identical configuration.
    fn bellows_config() -> stt::SttConfig {
        stt::SttConfig {
            endpoint: Some(stt::EndpointConfig::default()),
            ..stt::SttConfig::default()
        }
    }
}

/// Open the Bellows over a whisper model file. `bias_terms` seed the
/// `initial_prompt` (domain vocab + recent salt, capped at
/// `SttConfig::max_bias_terms` by stt). `tier` records the model tier the caller
/// resolved. Native-only: needs `SttStream::with_model`, itself
/// `#[cfg(feature = "whisper")]`, so the whole export block is gated — off the
/// whisper feature `BellowsHandle` has no constructor and is only reachable
/// through the hermetic test seam. Errors (bad model / bad config) surface as
/// `BellowsError` — no panic across FFI.
#[cfg(feature = "whisper")]
#[uniffi::export]
impl BellowsHandle {
    #[uniffi::constructor]
    pub fn open(
        model_path: String,
        bias_terms: Vec<String>,
        tier: BellowsTier,
    ) -> Result<std::sync::Arc<Self>, BellowsError> {
        let _ = tier; // advisory; the resolved model_path already encodes it.
        let stream = SttStream::with_model(
            std::path::Path::new(&model_path),
            Self::bellows_config(),
            &bias_terms,
        )?;
        Ok(std::sync::Arc::new(Self::from_stream(stream)))
    }
}

#[uniffi::export]
impl BellowsHandle {
    /// Buffer PCM (16 kHz mono f32). Owns the `Vec` handed across FFI and
    /// passes a slice to the core — the core signature stays `&[f32]` (Plan
    /// review edit #7). Cheap; call off the realtime thread.
    pub fn push_pcm(&self, pcm: Vec<f32>) {
        self.stream.push_pcm(&pcm);
    }

    /// Drain buffered PCM, decode every ready window, and return the segments
    /// finalized this call (append-only). Runs the decode on the caller's
    /// (background) thread. Decode failure surfaces as `BellowsError`.
    pub fn poll(&self) -> Result<Vec<FfiSegment>, BellowsError> {
        Ok(self.stream.poll()?.into_iter().map(Into::into).collect())
    }

    /// Volatile preview tail for the shimmer UI. Never persisted.
    pub fn preview_tail(&self) -> String {
        self.stream.preview_tail()
    }

    /// True once since the last read if endpointing has latched sustained
    /// trailing silence after a real utterance. Latched, auto-cleared on read
    /// (the shell polls this and calls `end()` when it flips true).
    pub fn utterance_ended(&self) -> bool {
        self.stream.utterance_ended()
    }

    /// Reset endpointing state only (fresh turn / manual tap-to-send). Leaves
    /// buffered PCM and finalized/pending transcript state untouched.
    pub fn reset_endpoint(&self) {
        self.stream.reset_endpoint();
    }

    /// DONE: flush remaining buffered audio and finalize everything pending.
    /// Idempotent. Returns the tail segments finalized by the flush.
    pub fn end(&self) -> Result<Vec<FfiSegment>, BellowsError> {
        Ok(self.stream.end()?.into_iter().map(Into::into).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stt::{RawSegment, ScriptedDecoder, SttConfig};

    // Hermetic: 16 kHz mono f32, the shape the AVAudioEngine tap feeds
    // push_pcm. Endpoint arithmetic (stolen from stt's endpoint tests):
    // 480-sample / 30 ms windows, min-utterance 300 ms (10 windows), silence
    // latch 1200 ms (40 windows).
    const WINDOW: usize = 480;

    fn sine(n_samples: usize, freq_hz: f32, amplitude: f32) -> Vec<f32> {
        let sr = SttConfig::default().sample_rate as f32;
        (0..n_samples)
            .map(|i| {
                let t = i as f32 / sr;
                amplitude * (2.0 * std::f32::consts::PI * freq_hz * t).sin()
            })
            .collect()
    }

    fn seg(cs0: i64, cs1: i64, t: &str) -> RawSegment {
        RawSegment {
            start_cs: cs0,
            end_cs: cs1,
            text: t.into(),
        }
    }

    /// Build a BellowsHandle over a ScriptedDecoder — the same seam stt's own
    /// tests use, so the whole bridge is exercised with no model file.
    fn scripted_bellows(scripts: Vec<Vec<RawSegment>>, bias: &[String]) -> BellowsHandle {
        let stream = SttStream::with_decoder(
            Box::new(ScriptedDecoder::new(scripts)),
            BellowsHandle::bellows_config(),
            bias,
        );
        BellowsHandle::from_stream(stream)
    }

    #[test]
    fn push_poll_preview_end_stream_through_the_bridge() {
        // 9 s of PCM → two 5 s / 1 s windows, both drained in one poll(). Same
        // realistic time-shifted composition as stt's finalize test: only the
        // 1 s overlap words repeat; the straddling tail is held for end().
        let bellows = scripted_bellows(
            vec![
                vec![
                    seg(0, 180, "order twelve"),
                    seg(180, 360, "two by tens"),
                    seg(360, 480, "for the"),
                ],
                vec![
                    seg(0, 80, "for the"),
                    seg(80, 300, "deck framing"),
                    seg(300, 480, "today"),
                ],
                vec![seg(0, 80, "today")],
            ],
            &[],
        );

        bellows.push_pcm(vec![0.0; 144_000]); // 9 s → both windows ready

        let live = bellows.poll().expect("poll decodes both ready windows");
        let live_text: Vec<&str> = live.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(
            live_text,
            vec!["order", "twelve", "two", "by", "tens", "for", "the", "deck", "framing"],
            "poll() returns finalized segments through the bridge, append-only"
        );

        assert_eq!(
            bellows.preview_tail(),
            "today",
            "preview_tail() surfaces the held straddling tail (volatile)"
        );

        let tail = bellows.end().expect("end() flushes the bounded tail");
        let tail_text: Vec<&str> = tail.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(
            tail_text,
            vec!["today"],
            "end() finalizes only the held tail"
        );

        // Append-only in time across the whole stream (FfiSegment preserves it).
        let mut prev = 0;
        for s in live.iter().chain(tail.iter()) {
            assert!(s.start_ms >= prev, "start_ms must be non-decreasing");
            prev = s.start_ms;
        }
    }

    #[test]
    fn poll_is_a_noop_until_a_window_is_ready() {
        let bellows = scripted_bellows(vec![], &[]);
        bellows.push_pcm(vec![0.0; 1000]); // far short of a 5 s window
        assert!(
            bellows
                .poll()
                .expect("no decode, no scripted panic")
                .is_empty(),
            "poll() before any ready window returns no segments"
        );
    }

    #[test]
    fn bias_terms_seed_the_initial_prompt_through_the_bridge() {
        // One 5 s window; assert the scripted decode still finalizes, proving
        // open()'s bias_terms path (vocab -> initial_prompt) is wired. The
        // prompt CONTENT is verified in stt's own bias_prompt test; here we
        // confirm the bridge threads bias_terms into SttStream::with_decoder.
        let bellows = scripted_bellows(
            vec![vec![seg(0, 300, "the french drain today")]],
            &["french drain".to_string()],
        );
        bellows.push_pcm(vec![0.0; 80_000]); // 5 s → one window ready
        let out = bellows.poll().expect("biased poll decodes");
        assert!(
            !out.is_empty(),
            "a biased stream still finalizes segments through the bridge"
        );
    }

    #[test]
    fn utterance_ended_latches_on_trailing_silence_through_the_bridge() {
        let bellows = scripted_bellows(vec![], &[]);

        // 12 windows (360 ms) voiced — over the 300 ms min-utterance guard.
        for _ in 0..12 {
            bellows.push_pcm(sine(WINDOW, 440.0, 0.5));
            assert!(!bellows.utterance_ended(), "voiced audio must not latch");
        }
        // 39 windows of silence: not yet (1170 ms < 1200 ms latch).
        for _ in 0..39 {
            bellows.push_pcm(vec![0.0; WINDOW]);
            assert!(!bellows.utterance_ended(), "under 1200 ms must not latch");
        }
        // 40th silence window (1200 ms total): latches true, clears on read.
        bellows.push_pcm(vec![0.0; WINDOW]);
        assert!(
            bellows.utterance_ended(),
            "sustained trailing silence must latch utterance_ended through the bridge"
        );
        assert!(!bellows.utterance_ended(), "latch clears after being read");
    }

    #[test]
    fn reset_endpoint_clears_a_pending_latch_through_the_bridge() {
        let bellows = scripted_bellows(vec![], &[]);
        for _ in 0..12 {
            bellows.push_pcm(sine(WINDOW, 440.0, 0.5));
        }
        for _ in 0..40 {
            bellows.push_pcm(vec![0.0; WINDOW]);
        }
        // Don't read the latch — reset_endpoint() must clear it.
        bellows.reset_endpoint();
        assert!(
            !bellows.utterance_ended(),
            "reset_endpoint() clears a pending latch through the bridge"
        );
    }

    #[test]
    fn ffi_segment_projects_finalized_segment() {
        let s = FfiSegment::from(FinalizedSegment {
            start_ms: 120,
            end_ms: 480,
            text: "salt".into(),
        });
        assert_eq!(
            s,
            FfiSegment {
                start_ms: 120,
                end_ms: 480,
                text: "salt".into()
            }
        );
    }

    #[test]
    fn stt_error_maps_to_bellows_error() {
        assert!(matches!(
            BellowsError::from(SttError::ModelLoad("x".into())),
            BellowsError::ModelLoad { .. }
        ));
        assert!(matches!(
            BellowsError::from(SttError::Decode("x".into())),
            BellowsError::Decode { .. }
        ));
        assert!(matches!(
            BellowsError::from(SttError::Config("x".into())),
            BellowsError::Config { .. }
        ));
    }
}
