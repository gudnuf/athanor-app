# Embedding goose in-process on iOS — field report & feedback

**Project:** Athanor, a voice-first iOS learning app whose AI teacher (the
"Mystagogue") runs on an embedded goose agent inside a Rust core, driven from
SwiftUI over UniFFI. No server, no accounts — the agent loop runs on the phone.

**Date:** 2026-07-07 · **goose tag:** `v1.41.0` · **Status:** working — live
Anthropic turns with in-process tool dispatch, proven on the iOS simulator
end-to-end (real whisper STT → goose agent loop → streamed reply → custom
tools mutating a local SQLite store). First live turn completed in ~2s.

This document is written as feedback for the goose team: what we use, what
worked surprisingly well, where we hit friction, and what would make goose a
first-class embeddable engine.

---

## 1. What we use goose for

- **The full agent loop as a library** (`Agent::reply` stream): system prompt,
  multi-turn conversation, streamed text deltas, tool dispatch, terminal turn.
- **Anthropic provider** from `goose-providers` (`AnthropicProviderBuilder` +
  `ApiClient` with `AuthMethod::ApiKey`), key injected by the caller — never
  read from env inside the library.
- **Frontend tools** (`ExtensionConfig::Frontend`) as the bridge to our six
  domain tools (they mutate an app-owned SQLite store through our own
  `ToolDispatch` trait).
- **Ephemeral sessions**: `SessionType::Hidden`, `max_turns: Some(50)`,
  no retry config. Conversation persistence is ours, not goose's.

What we deliberately do **not** use: goose's own session storage, recipes
(we template prompts ourselves), any external MCP servers (impossible on
iOS), goose's bins/CLI, local inference.

## 2. Build recipe that works (iOS device + simulator)

```toml
goose = { git = "https://github.com/aaif-goose/goose.git", tag = "v1.41.0",
          default-features = false, features = ["rustls-tls"], optional = true }
goose-providers = { git = "…", tag = "v1.41.0", default-features = false, optional = true }
rmcp = "=1.7.0"   # see §4.1 — load-bearing pin
```

- `cargo build --lib` for `aarch64-apple-ios` and `aarch64-apple-ios-sim`:
  **zero errors, no source patches to goose.** 437 dep crates cross-compile,
  including `aws-lc-rs` and bundled `libsqlite3-sys`.
- Toolchain: goose's `rust-toolchain.toml` pins 1.92 — iOS `rust-std` must be
  added **to that pinned toolchain**, not the default one.
- Final link happens in Xcode, which supplies the compiler-rt builtin
  `___chkstk_darwin` (pulled in by `aws-lc-rs`/`blake3`). A raw cargo link of
  a *binary* fails on that symbol; the staticlib-into-Xcode path is fine.

## 3. What worked well (genuinely)

1. **The portable feature set is real.** `default = []` plus `rustls-tls` is
   all it takes; the heavy optionals (local-inference, code-mode/V8, keyring,
   otel…) drop away cleanly, and `keyring` is additionally target-gated so it
   never even resolves for iOS. This felt designed, not accidental.
2. **In-process extensions over duplex pipes.** `tokio::io::duplex` transport
   between the agent and an `rmcp::ServerHandler` in the same process — no
   fork/exec, no sockets. Exactly what a sandboxed mobile app needs, and it
   worked as documented-by-source.
3. **Frontend tools are the right embed seam.** The reply stream yields
   `FrontendToolRequest`, blocks until `agent.handle_tool_result(...)` — we
   service it inline against our own dispatch trait. No globals, no unsafe,
   1:1 mapping onto an app-owned tool layer.
4. **Provider abstraction is embeddable.** We built the Anthropic provider
   with an injected key and custom client config without touching env vars,
   and in tests we swap in a canned `Provider` impl so the whole agent loop
   runs hermetically (no network, no key) in CI.
5. **It's fast enough.** First live turn (system prompt + tools offered +
   streamed reply) completed in ~1.99s on simulator over residential Wi-Fi.

## 4. Friction & limitations found (the actual feedback)

### 4.1 No stable embed API — internal-type coupling + the rmcp pin
The biggest one. There is no `goose-embed`/SDK crate for this use case
(`goose-sdk-types` on crates.io is a 0.1.0-alpha and not what the engine
needs), so we depend on **internal** types across `goose`, `goose-providers`,
and `rmcp`. Concretely: goose v1.41.0 calls `.cloned()` on
`Peer::peer_info()`, whose return type changed (`Option<&InitializeResult>` →
`Option<Arc<InitializeResult>>`) in rmcp 1.8.0. goose's own `Cargo.lock` pins
1.7.0, but an external embedder re-resolves the graph fresh and greedily picks
1.8.0 → **goose fails to compile from a clean checkout of an embedding
project**. We carry `rmcp = "=1.7.0"` as a load-bearing pin.
**Ask:** either a semver-honest `rmcp` range in goose's manifests, or a small
blessed embed surface (agent loop + provider trait + frontend-tool types)
whose compatibility is tested in goose's CI against a fresh resolve.

### 4.2 Builtin-extension registration is a bare `fn` pointer
`register_builtin_extension(name, fn(DuplexStream, DuplexStream))` takes a
plain fn pointer — no captures — so an extension can't close over app state
(our store handle). We routed around it with frontend tools (§3.3), which are
better for us anyway, but if builtin registration took `Arc<dyn Fn…>` the
duplex path would be usable for stateful embedders too.

### 4.3 Tool namespacing is silent
Extension tools surface to the provider as `{extension}__{tool}`. Calling the
bare name doesn't error loudly — it produces an error tool-response that the
model then sees. Fine once you know; cost us a debugging loop in the spike.
A `tool not found: did you mean mystagogue__fix_salt?` in the tracing output
would have saved it.

### 4.4 Bins don't (and shouldn't) link for iOS — but `-p goose` tries
`cargo build -p goose --target aarch64-apple-ios` fails in the *linker* on
goose's own `[[bin]]` targets. `--lib` is the fix and is fine, but it reads
as a goose bug until you inspect the failure. `required-features` on the bins
(or a note in the manifest) would make the library-only path self-evident.

### 4.5 Inert-but-compiled subsystems on iOS
`arboard` (clipboard), `tokio` `process`/`signal`, rmcp's
`transport-child-process`, `webbrowser`, `tree-sitter` grammars, `image` —
all unconditional deps that compile for iOS but can never be used there.
They cost compile time and binary size (debug rlib ~473MB; release app still
carries meaningful dead weight). **Ask:** feature-gate the desktop-only
conveniences so a minimal embedded build exists (`features = ["agent-core"]`
sort of shape).

### 4.6 Session manager assumes it owns persistence
We wanted conversation state fully app-owned (our SQLite schema). goose's
session machinery leans toward its own storage; `SessionType::Hidden` +
carrying our own history into each `reply` works, but it took source reading
to find the combination, and rebuilding `Vec<Message>` from our history each
turn is the kind of thing an embed API would make explicit.

### 4.7 Version drift risk is structural
Everything above compounds: tag-pinned git dep + internal types + ecosystem
crates moving (rmcp) means each goose upgrade is a deliberate migration task
for us, not a bump. That's acceptable for now, but it's the main thing
keeping this integration "expert-only."

## 5. Readiness verdict

**For our purpose — an embedded, in-process, tools-enabled agent loop on
iOS — goose v1.41.0 is a green light and is running in our app today.** The
architecture held from spike to shipped MVP with zero patches to goose
itself, which is the strongest endorsement we can give.

What separates "possible for a motivated team" from "supported use case" is
almost entirely §4.1 + §4.5: a blessed embed surface with a fresh-resolve CI
gate, and a lean feature profile for hosts that can't spawn processes or
touch a clipboard. With those, goose-on-mobile would be a genuinely unique
capability — we found no other agent framework that survives
`aarch64-apple-ios` this cleanly.

---

*Deeper artifacts (in this repo / project workspace): the Gate-1 spike report
with the full failure table, the engine adapter
(`crates/athanor-core/src/engine/goose.rs` — the one file that names goose
types), and the hermetic eval harness that drives the agent loop with a
canned provider in CI.*
