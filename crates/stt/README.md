# stt

Pure, hermetic speech-to-text building blocks (no native backend, no model):

- `Endpointer` — energy-based silence latch (turn end).
- `TextBellows` — the Rust-owned turn machine behind the Apple-speech Bellows:
  takes 16 kHz PCM plus recognizer text events, decides the append-only commit,
  request cycling, the turn end, and never emits an empty turn.
- `SttStream` + `Decoder` — the older chunk/finalize pipeline over a scripted
  decoder seam (kept as tested library code; no decoder backend ships).

The recognizer itself is Apple's on-device `SFSpeechRecognizer`, driven from
`apps/ios` (`AppleBellowsController`). The whisper backend was removed on
2026-11-23. See `src/lib.rs` for module docs.
