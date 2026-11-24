#!/usr/bin/env bash
# Regenerates apps/ios/Packages/AthanorCoreFFI's binary artifacts:
#   - crates/ffi built --release --features goose for
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
# and SDKROOT at the real iphoneos/iphonesimulator SDK via xcrun.
#
# --- Why there is NO `unset NIX_CFLAGS_COMPILE NIX_LDFLAGS …` here ---
# murmur's script unset the NIX_* vars to keep them off the iOS link. We must
# NOT: the per-target CC/CXX/AR/LINKER overrides above already point iOS builds
# at system clang, which ignores NIX_* entirely — so the unsets protect nothing
# on the iOS side. What they DO break is the HOST build-scripts (aws-lc-sys's and
# ring's build.rs, compiled by the nix host toolchain for the build machine):
# those need NIX_CFLAGS_COMPILE/NIX_LDFLAGS to find libiconv, and without them
# the host link fails with `ld: library not found for -liconv` on a cold target
# dir (a warm cache masks it — the earlier build.rs artifacts survive). The D1
# verification build proved the per-target overrides link iOS correctly with the
# NIX_* vars left intact. Do NOT "restore murmur parity" by re-adding the unsets.
#
# --- athanor-specific amendment (meta-verified 2026-07-06) ---
# cc-rs (ring / aws-lc-sys under goose) invokes a
# bare `xcrun`. A nix `xcbuild` xcrun stub on PATH shadows /usr/bin/xcrun and
# kills those builds (exit 255). Fix: prepend /usr/bin to PATH in the cross
# blocks so the real xcrun wins. DEVELOPER_DIR is parameterized off
# `xcode-select -p` (override via env).
#
set -euo pipefail

# --- Run this from a LOGIN shell, NOT inside `nix develop` ---
# This script manages its own devshell (each build_target uses `nix develop -c`).
# If the WHOLE script is wrapped in `nix develop -c bash build-ffi.sh`, the
# devshell presets DEVELOPER_DIR to a nix apple-sdk (`/nix/store/…-apple-sdk-*`,
# MacOSX-only, NO iOS SDKs). The `: "${DEVELOPER_DIR:=…}"` default below then
# keeps that nix value, so `xcrun --sdk iphonesimulator` fails with
# "unable to find sdk: 'iphonesimulator'". Fail loudly instead of building
# against the wrong SDK root.
if [ -n "${IN_NIX_SHELL:-}" ] || case "${DEVELOPER_DIR:-}" in /nix/store/*) true ;; *) false ;; esac; then
  echo "!! Run this from a login shell, NOT inside 'nix develop'." >&2
  echo "!! It manages its own devshell blocks; a nix-preset DEVELOPER_DIR" >&2
  echo "!! (${DEVELOPER_DIR:-<in-nix-shell>}) lacks the iOS SDKs." >&2
  echo "!! Just run:  ./apps/ios/build-ffi.sh" >&2
  exit 1
fi

cd "$(dirname "$0")/../.."   # repo root
FFI_DIR="apps/ios/Packages/AthanorCoreFFI"
BINDINGS_DIR="$(mktemp -d)"
trap 'rm -rf "$BINDINGS_DIR"' EXIT

# Default DEVELOPER_DIR to Xcode when unset (still overridable via env), but
# split the assignment from `export` so `set -e` aborts if xcode-select fails.
# A combined `export X=$(cmd)` always returns 0, swallowing the failure and
# exporting an error string — the exact trap that masked the SDKROOT failure
# below.
if [ -z "${DEVELOPER_DIR:-}" ]; then
  DEVELOPER_DIR="$(xcode-select -p)"
fi
export DEVELOPER_DIR

# Build the STATICLIB slice only (--crate-type staticlib), not the manifest's
# full {cdylib,staticlib,lib} set. The xcframework needs only libffi.a, and a
# staticlib is archived — no final link — so we sidestep the bare-cargo cdylib
# link that fails on device: goose pulls aws-lc-sys, whose bcm.o references the
# compiler-rt builtin `___chkstk_darwin`, which a standalone cargo cdylib link
# doesn't pull in (Xcode's real app-bundle link DOES link clang_rt — that device
# app-link is G2/operator-gated, not this script's bar). This also drops the
# iOS-deployment-target ld warnings from the cdylib link.
build_target() {
  local triple="$1" sdk="$2" envprefix="$3"
  echo "==> building crates/ffi staticlib for $triple (--features goose)"
  nix develop -c bash -c '
    set -euo pipefail
    export PATH="/usr/bin:$PATH"
    export DEVELOPER_DIR="'"$DEVELOPER_DIR"'"
    # Split assignment from export so set -e catches an xcrun failure instead of
    # exporting its error string as SDKROOT (see the header note).
    SDKROOT=$(/usr/bin/xcrun --sdk '"$sdk"' --show-sdk-path)
    export SDKROOT
    export CC_'"$envprefix"'=/usr/bin/clang
    export CXX_'"$envprefix"'=/usr/bin/clang++
    export AR_'"$envprefix"'=/usr/bin/ar
    export CARGO_TARGET_'"$(echo "$envprefix" | tr '[:lower:]' '[:upper:]')"'_LINKER=/usr/bin/clang
    cargo rustc -p ffi --release --features goose --target '"$triple"' --lib --crate-type staticlib
  '
}

build_target aarch64-apple-ios-sim iphonesimulator aarch64_apple_ios_sim
build_target aarch64-apple-ios     iphoneos         aarch64_apple_ios

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
  -library "target/aarch64-apple-ios-sim/release/libffi.a" -headers "$BINDINGS_DIR/headers-sim" \
  -library "target/aarch64-apple-ios/release/libffi.a" -headers "$BINDINGS_DIR/headers-device" \
  -output "$FFI_DIR/Frameworks/ffiFFI.xcframework"

echo "==> done. Run 'cd apps/ios && ./generate.sh' next."
