#!/usr/bin/env bash
# Regenerate the AppIcon asset from the SVG source. Requirements enforced for
# App Store validation: 1024x1024, PNG24, NO alpha channel.
set -euo pipefail
cd "$(dirname "$0")"
OUT=../../apps/ios/Sources/Assets.xcassets/AppIcon.appiconset
nix shell nixpkgs#librsvg nixpkgs#imagemagick -c bash -c "
  rsvg-convert -w 1024 -h 1024 athanor-icon.svg -o /tmp/athanor-icon-raw.png &&
  magick /tmp/athanor-icon-raw.png -background '#0c0906' -alpha remove -alpha off PNG24:$OUT/AppIcon1024.png
"
echo 'regenerated: verify no alpha:' && sips -g hasAlpha "$OUT/AppIcon1024.png"
