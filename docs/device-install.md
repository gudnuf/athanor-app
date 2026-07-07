# Installing Athanor on a real iPhone

The staged state (as of `bde7960`): device xcframework built, signing
identity `Apple Development: damiangoodenough1@gmail.com (J4R462XD94)`
present on the Mac, iPhone 14 ("Damian's iPhone") paired but offline.
Once the phone is reachable, the whole install is a few commands.

## 0. Make the phone reachable

Plug the iPhone into the Mac with a cable (most reliable), unlock it,
and tap **Trust** if prompted. Verify:

```sh
xcrun devicectl list devices   # State must NOT be "unavailable"
```

## 1. Fresh FFI build (only if main moved since the last build)

From a **login shell** — never inside `nix develop`:

```sh
bash ~/athanor-app/apps/ios/build-ffi.sh
```

## 2. Generate the real project

```sh
cd ~/athanor-app/apps/ios && ./generate.sh
```

This picks the real project when the xcframework and `project.local.yml`
are present (the local file carries the API key build setting — it is
gitignored; don't cat it into a terminal you're screensharing).

## 3. Build + sign for the device

```sh
cd ~/athanor-app/apps/ios
xcodebuild -project Athanor.xcodeproj -scheme Athanor \
  -destination 'platform=iOS,name=Damian’s iPhone' \
  -allowProvisioningUpdates \
  DEVELOPMENT_TEAM=J4R462XD94 \
  CODE_SIGN_STYLE=Automatic \
  CODE_SIGNING_ALLOWED=YES \
  build
```

`-allowProvisioningUpdates` lets xcodebuild mint the provisioning
profile headlessly with the existing certificate.

## 4. Install and launch

```sh
APP=$(find ~/Library/Developer/Xcode/DerivedData -path '*Debug-iphoneos/Athanor.app' -newer /tmp -print -quit 2>/dev/null || \
      find ~/Library/Developer/Xcode/DerivedData -path '*Debug-iphoneos/Athanor.app' -print -quit)
xcrun devicectl device install app --device 'Damians-iPhone.coredevice.local' "$APP"
xcrun devicectl device process launch --device 'Damians-iPhone.coredevice.local' com.gudnuf.athanor
```

## 5. First-run notes on device

- **Trust the developer profile** the first time: on the phone,
  Settings → General → VPN & Device Management → trust the
  Apple Development certificate, then launch again.
- **No seed on device**: the lived-in seed fixture is a local dev-only
  file; a device install starts cold with initiation. That's the
  intended first-device-run experience (and gate G4: first live turn).
- **Whisper model**: the app downloads the model tier on first session;
  give it a minute on Wi-Fi. RTF check runbook:
  `~/athanor/forge/athanor-app/spike-whisper-report.md` (gate G3).
- Free-tier provisioning profiles expire after 7 days; re-run steps
  3–4 to refresh.
