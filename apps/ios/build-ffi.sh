#!/usr/bin/env bash
# Regenerates apps/ios/Packages/AthanorCoreFFI's binary artifacts:
#   - crates/ffi built --release --features goose,whisper for
#     aarch64-apple-ios-sim AND aarch64-apple-ios
#   - Swift bindings generated via the uniffi-bindgen dev binary
#   - Frameworks/ffiFFI.xcframework (device + sim slices)
#
# GITIGNORED (large binaries): Frameworks/ffiFFI.xcframework.
# COMMITTED: Sources/AthanorCoreFFI/ffi.swift + Package.swift.
# Run this after any crates/ffi surface change, or whenever the xcframework is
# missing. Then run ./generate.sh.
#
# --- Why not `nix develop -c cargo build --target ...` alone ---
# athanor's devShell rust-overlay toolchain is the single source for host builds
# and `cargo test --workspace` (unchanged). But for the iOS cross builds the
# nix-wrapped clang/cc-wrapper hardcodes macOS SDK paths + a `-mmacos-version-min`
# flag that conflicts with `-target arm64-apple-ios...-simulator`, and resolves
# libraries against the MacOSX SDK's .tbd stubs even when --target is iOS —
# failing the final link ("building for iOS Simulator, but linking in .tbd built
# for macOS"). Fix (murmur pattern): keep the nix toolchain for cargo/rustc, but
# point CC/CXX/AR/LINKER at the *system* Xcode toolchain (/usr/bin/clang, ar)
# and SDKROOT at the real iphoneos/iphonesimulator SDK via xcrun, unsetting the
# NIX_*FLAGS.
#
# --- athanor-specific amendment (meta-verified 2026-07-06) ---
# cc-rs (ring / aws-lc-sys under goose, and whisper-rs-sys's cmake) invokes a
# bare `xcrun`. A nix `xcbuild` xcrun stub on PATH shadows /usr/bin/xcrun and
# kills those builds (exit 255). Fix: prepend /usr/bin to PATH in the cross
# blocks so the real xcrun wins. DEVELOPER_DIR is parameterized off
# `xcode-select -p` (override via env).
#
# --- whisper native link stack ---
# `--features whisper` pulls whisper-rs-sys, whose cmake build emits static libs
# (libwhisper.a, libggml*.a) into target/<t>/release/build/whisper-rs-sys-*/out/lib
# and asks cargo to link the Accelerate/Foundation/Metal/MetalKit frameworks +
# libc++. `cargo build` records those directives in libffi's metadata but does
# NOT fold the ggml/whisper archives into libffi.a. So we merge libffi.a with all
# the ggml/whisper archives per-slice via `libtool -static`, and the framework/
# libc++ link flags are declared in Package.swift's AthanorCoreFFI linkerSettings.
set -euo pipefail

cd "$(dirname "$0")/../.."   # repo root
FFI_DIR="apps/ios/Packages/AthanorCoreFFI"
BINDINGS_DIR="$(mktemp -d)"
MERGE_DIR="$(mktemp -d)"
trap 'rm -rf "$BINDINGS_DIR" "$MERGE_DIR"' EXIT

: "${DEVELOPER_DIR:=$(xcode-select -p)}"
export DEVELOPER_DIR

# Merge libffi.a + the whisper/ggml static archives for one target into a single
# fat static lib. Args: <target-triple> <sdk> <out.a>
merge_slice() {
  local triple="$1" out="$3"
  local base="target/$triple/release"
  local ffi_a="$base/libffi.a"
  [ -f "$ffi_a" ] || { echo "!! missing $ffi_a" >&2; exit 1; }
  # Locate the whisper-rs-sys out/lib holding the ggml/whisper archives for this
  # target. There may be several build-dir hashes; pick the one that actually has
  # libwhisper.a (newest wins).
  local libdir
  libdir="$(find "$base/build" -type f -path '*whisper-rs-sys-*/out/lib/libwhisper.a' \
              -exec dirname {} \; 2>/dev/null | xargs -I{} stat -f '%m %N' {} 2>/dev/null \
              | sort -rn | head -1 | cut -d' ' -f2-)"
  [ -n "$libdir" ] && [ -d "$libdir" ] || { echo "!! no whisper-rs-sys out/lib under $base/build" >&2; exit 1; }
  echo "    whisper libs: $libdir"
  local archives=("$ffi_a")
  local a
  for a in whisper ggml ggml-base ggml-blas ggml-cpu ggml-metal; do
    [ -f "$libdir/lib$a.a" ] && archives+=("$libdir/lib$a.a")
  done
  echo "    merging ${#archives[@]} archives -> $out"
  /usr/bin/libtool -static -o "$out" "${archives[@]}"
}

# Build the STATICLIB slice only (--crate-type staticlib), not the manifest's
# full {cdylib,staticlib,lib} set. The xcframework needs only libffi.a, and a
# staticlib is archived — no final link — so we sidestep the bare-cargo cdylib
# link that fails on device: goose pulls aws-lc-sys, whose bcm.o references the
# compiler-rt builtin `___chkstk_darwin`, which a standalone cargo cdylib link
# doesn't pull in (Xcode's real app-bundle link DOES link clang_rt — that device
# app-link is G2/operator-gated, not this script's bar). This also drops the
# iOS-deployment-target ld warnings from the cdylib link (whisper.cpp cmake
# targets the SDK's 26.2 while cargo's cdylib link defaulted to 10.0).
build_target() {
  local triple="$1" sdk="$2" envprefix="$3"
  echo "==> building crates/ffi staticlib for $triple (--features goose,whisper)"
  nix develop -c bash -c '
    set -euo pipefail
    export PATH="/usr/bin:$PATH"
    export DEVELOPER_DIR="'"$DEVELOPER_DIR"'"
    export SDKROOT=$(/usr/bin/xcrun --sdk '"$sdk"' --show-sdk-path)
    export CC_'"$envprefix"'=/usr/bin/clang
    export CXX_'"$envprefix"'=/usr/bin/clang++
    export AR_'"$envprefix"'=/usr/bin/ar
    export CARGO_TARGET_'"$(echo "$envprefix" | tr '[:lower:]' '[:upper:]')"'_LINKER=/usr/bin/clang
    unset NIX_CFLAGS_COMPILE NIX_LDFLAGS NIX_CFLAGS_COMPILE_FOR_BUILD NIX_LDFLAGS_FOR_BUILD
    cargo rustc -p ffi --release --features goose,whisper --target '"$triple"' --lib --crate-type staticlib
  '
}

build_target aarch64-apple-ios-sim iphonesimulator aarch64_apple_ios_sim
build_target aarch64-apple-ios     iphoneos         aarch64_apple_ios

echo "==> merging whisper/ggml archives into per-slice static libs"
merge_slice aarch64-apple-ios-sim iphonesimulator "$MERGE_DIR/libathanorffi-sim.a"
merge_slice aarch64-apple-ios     iphoneos         "$MERGE_DIR/libathanorffi-device.a"

echo "==> generating Swift bindings (uniffi-bindgen, host build)"
nix develop -c cargo run -p ffi --features uniffi-bindgen-cli --bin uniffi-bindgen -- \
  generate --library target/aarch64-apple-ios-sim/release/libffi.a \
  --language swift --out-dir "$BINDINGS_DIR"

mkdir -p "$FFI_DIR/Sources/AthanorCoreFFI"
cp "$BINDINGS_DIR/ffi.swift" "$FFI_DIR/Sources/AthanorCoreFFI/ffi.swift"

echo "==> assembling ffiFFI.xcframework"
rm -rf "$FFI_DIR/Frameworks/ffiFFI.xcframework"
mkdir -p "$FFI_DIR/Frameworks"
for slice in sim device; do
  hdir="$BINDINGS_DIR/headers-$slice"
  mkdir -p "$hdir"
  cp "$BINDINGS_DIR/ffiFFI.h" "$hdir/"
  cp "$BINDINGS_DIR/ffiFFI.modulemap" "$hdir/module.modulemap"
done

/usr/bin/xcodebuild -create-xcframework \
  -library "$MERGE_DIR/libathanorffi-sim.a"    -headers "$BINDINGS_DIR/headers-sim" \
  -library "$MERGE_DIR/libathanorffi-device.a" -headers "$BINDINGS_DIR/headers-device" \
  -output "$FFI_DIR/Frameworks/ffiFFI.xcframework"

echo "==> done. Run 'cd apps/ios && ./generate.sh' next."
