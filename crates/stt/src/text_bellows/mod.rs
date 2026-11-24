//! Text-driven Bellows: Rust owns endpointing and finalization for a platform
//! recognizer (Apple speech) that hands us cumulative text. Pure and hermetic.

mod committer;
pub(crate) use committer::Committer;

use crate::EndpointConfig;

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
}

pub struct TextBellows {
    #[allow(dead_code)]
    committer: Committer,
}
