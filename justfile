# Athanor — the app. Common commands. Run inside `nix develop` (or with
# cargo/rustup on PATH some other way).

# Hermetic tier: what CI runs on every push. No network, no API keys.
check: fmt-check clippy test

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

clippy:
    cargo clippy --workspace --all-targets -- -D warnings

build:
    cargo build --workspace

test:
    cargo test --workspace

# Real-API / gated tiers land once the evals crate exists (see docs/plans/).

# Regenerate Swift bindings from the ffi crate. Requires the uniffi-bindgen-cli
# feature and a compiled cdylib; wired up once apps/ios/build-ffi.sh exists.
bindgen-swift:
    cargo build -p ffi
    cargo run -p ffi --features uniffi-bindgen-cli --bin uniffi-bindgen -- \
        generate --library target/debug/libffi.dylib --language swift --out-dir apps/ios/Packages/AthanorCoreFFI/Generated
