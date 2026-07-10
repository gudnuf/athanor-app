# WORKFLOW_NOTES — TestFlight release pipeline (.github/workflows/release.yml)

Companion to `.github/workflows/release.yml`. Records every deviation from the
canonical damsac/sitewalk `release.yml` (builds #24-27, proven) and the factory
doc `~/athanor/forge/factory-apps/release-pipeline.md`, and why each was made.

## Canonical sources read
1. `~/athanor/forge/factory-apps/release-pipeline.md` (factory blueprint, keeper:murmur).
2. `damsac/sitewalk` `.github/workflows/release.yml` (heavily commented, battle-tested).
3. `damsac/sitewalk` `meta/TESTFLIGHT_CI_SETUP.md` (one-time setup doc).

## The shared skeleton (kept verbatim from sitewalk)
- Three lanes: push `main` = internal (upload), tag `v*` = external (upload),
  `workflow_dispatch` with `upload` boolean (default false) = dry-run.
- Two-style signing dance: **archive AUTOMATIC** (`CODE_SIGN_STYLE=Automatic`,
  `-allowProvisioningUpdates`, ASC API key auth) → **export MANUAL**
  (`ExportOptions signingStyle=manual`, explicit `provisioningProfiles` dict,
  cert via `import-codesign-certs@v3`, profile via
  `download-provisioning-profiles@v3`, NO `-allowProvisioningUpdates` on export).
- `CURRENT_PROJECT_VERSION=github.run_number`; `MARKETING_VERSION` from the tag.
- altool upload guarded by the `UPLOAD SUCCEEDED` / failure-marker checks (altool
  exits 0 on validation rejection — factory gotcha a).
- `paths-ignore` docs/markdown on branch pushes; artifact uploaded `if: always()`;
  API keys wiped `if: always()`.
- Secret NAMES: `APPLE_TEAM_ID`, `ASC_API_KEY_ID`, `ASC_API_ISSUER_ID`,
  `ASC_API_KEY_P8`, `APPLE_CERT_P12`, `APPLE_CERT_PASSWORD`.

## Deviations from sitewalk (and why)

### 1. Installs Nix; runs `build-ffi.sh` unmodified (biggest deviation)
Sitewalk's `build-ffi.sh` auto-detects nix and, when absent, builds against
rustup's cargo + the system Xcode toolchain — so its CI needs no nix.
**Athanor's `apps/ios/build-ffi.sh` does NOT have that fallback.** It drives its
own `nix develop -c` blocks per target (the goose+whisper iOS cross-compile
depends on the flake's pinned rust-overlay toolchain with the iOS targets, plus
per-target CC/AR/LINKER overrides pointing at the system Xcode clang). There is
no rustup path in the script.

Options considered:
- (a) Add a rustup fallback to build-ffi.sh — a script rewrite touching the
  delicate SDK/NIX_CFLAGS/xcrun-shadowing logic the header spends 60 lines
  warning about. High risk, out of this lane.
- (b) **Install nix on the runner and run the script as-is.** The nix path is
  the *proven local `nous` path*; reproducing it is the lowest-risk CI. Chosen.

So the workflow adds a `DeterminateSystems/nix-installer-action` step and runs
`bash apps/ios/build-ffi.sh` from a **plain login shell** — NOT wrapped in
`nix develop -c` (the script's guard exits if `IN_NIX_SHELL` is set or
`DEVELOPER_DIR` is a `/nix/store` path, because a nix-preset DEVELOPER_DIR is a
MacOSX-only apple-sdk with no iOS SDKs). The script captures `DEVELOPER_DIR`
from `xcode-select -p` (the runner's pinned Xcode) and passes it into its own
nix blocks.

> **Note to keeper:murmur:** this contradicts the "runner has no nix" framing in
> the lane brief, which assumed sitewalk's fallback. Athanor's script genuinely
> needs nix. If we later prefer a nix-free runner, that's a separate task to add
> a rustup branch to build-ffi.sh and re-verify the whole cross-compile.

### 2. No whisper-model fetch/cache step
Sitewalk bundles the whisper model at build time (`fetch-whisper-model.sh` +
`actions/cache`). Athanor downloads the model at **runtime**
(`Sources/Engine/ModelDownloader.swift`, URLSession) and bundles nothing, so
those two steps have no analogue and are omitted.

### 3. No `rustup target add`
The flake devShell (`flake.nix`) pins rust 1.95.0 with `aarch64-apple-ios`,
`aarch64-apple-ios-sim`, `x86_64-apple-ios` targets. build-ffi.sh uses nix
cargo, never rustup, so sitewalk's "Set up Rust (iOS device target)" step is
removed.

### 4. Generates via `./generate.sh`, not a standalone `project-release.yml`
Athanor has no `project-release.yml`. `generate.sh` is the same script local
dogfood uses: it writes the gitignored `project.local.yml` (the
`ANTHROPIC_API_KEY` build setting) and runs `xcodegen --spec project-real.yml`
(which includes `project.yml` + `project.local.yml` + the AthanorCoreFFI
package). Reusing generate.sh keeps CI and local builds on one path.

### 5. Forces `CODE_SIGNING_ALLOWED=YES` on the archive
`project.yml` sets `CODE_SIGNING_ALLOWED: "NO"` at the target base (dev/sim
convenience). The factory doc's disabled-signing gotcha: an archive with that
still set "succeeds" but silently produces an UNSIGNED app that fails export.
The archive step overrides it to `YES` alongside `CODE_SIGN_STYLE=Automatic`.

### 6. `ANTHROPIC_API_KEY` flows via generate.sh env, not an archive build setting
Sitewalk passes `PPQ_API_KEY` (and `ANTHROPIC_BASE_URL`) as xcodebuild build
settings on the archive, because its project-release.yml doesn't bake them.
Athanor's `generate.sh` already bakes `ANTHROPIC_API_KEY` (read from its env)
into `project.local.yml` → `$(ANTHROPIC_API_KEY)` plist expansion. So the key is
supplied as **env on the generate step** (from the optional `ANTHROPIC_API_KEY`
secret), never printed, never on the xcodebuild line. No `ANTHROPIC_BASE_URL`:
Athanor uses a real Anthropic key against the default host (sitewalk needed PPQ's
custom host).

**Key is OPTIONAL.** Verified in `Sources/Engine/RealEngineLoader.swift`:
`resolve()` guards `guard let key = resolveKey(), !key.isEmpty else { return
DemoEngine() }`, and `resolveKey()` returns nil when neither the Keychain nor the
baked Info.plist value has a key. So with the secret unset, CI builds green and
the shipped app links the real core but runs the DemoEngine.

**External-tester caveat (also in the workflow header):** generate.sh bakes the
key into the built app's Info.plist — fine for internal/single-device dogfood,
but a multi-tester external build would DISTRIBUTE the key. The external lane's
key story (runtime paste-field, or a throwaway/proxied key) must be resolved
before real external distribution. Not built yet.

### 7. Marketing version floor
No `MARKETING_VERSION` is set anywhere in the specs and Athanor has no prior
TestFlight history, so the archive step sets a `DEFAULT_MARKETING_VERSION=0.1.0`
floor for the internal/manual lanes (a `v*` tag overrides). Raise the floor only
above anything ever uploaded (TestFlight silently shadows lower versions).

### 8. `runs-on: macos-15`, Xcode pinned `26.2`
arm64 image; Xcode 26.2 matches the local nous toolchain the pre-signing chain
was rehearsed against (iOS 26.2 SDK), which also fixes the SDK the Rust
cross-build links against.

## Open items / handoff
- **Secrets not yet stamped.** keeper:murmur's `stamp-apple-secrets.sh
  gudnuf/athanor-app` (waiting on dam seeding `~/secrets/apple`). No live run is
  possible until then. `ANTHROPIC_API_KEY` is an additional optional secret.
- **Isaac's ASC clicks:** create the app record (`com.gudnuf.athanor`, team
  `98GXNZ6NKZ`) and create/download an **App Store distribution provisioning
  profile named `Athanor App Store`** (the ExportOptions `provisioningProfiles`
  value MUST match that name). Then enable internal-tester auto-distribution.
- **Icon validation:** verified `AppIcon1024.png` is 1024×1024 with **no alpha
  channel** (`sips -g hasAlpha` = no) — satisfies the hard icon gate.
- **Compliance:** `ITSAppUsesNonExemptEncryption: false` already in project.yml
  Info props — no manual Missing-Compliance stall.
- **Device-only build:** build-ffi.sh has no `--device-only` flag; CI builds
  both sim+device slices (~2× whisper compile). A future script flag would halve
  archive time. Not done here (out of lane — would require editing the script).
- **External key flow:** unbuilt (see deviation #6 caveat).

## Verification performed (this lane, no Apple secrets exist yet)
- `python3 -c 'yaml.safe_load(...)'` on release.yml — parses.
- `actionlint` (via `nix run nixpkgs#actionlint`) — clean.
- Local rehearsal of the non-signing chain on nous (login shell): build-ffi.sh,
  generate.sh, and `xcodebuild archive` with `CODE_SIGNING_ALLOWED=NO` to a
  throwaway path — proving the pre-signing chain is sound. Results recorded in
  the builder's report.
