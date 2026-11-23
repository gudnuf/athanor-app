# The gesture vocabulary

Every hidden door in the app, in one place (invariant #8, docs/invariants.md:
hidden depth — capability never crowds the surface). Any lane that adds a
gesture, long-press, tap-count, or context-conditional chip MUST record it
here in the same commit. This is the owner's spellbook: hidden from the
surface, never from him.

| Where | Gesture | What it opens |
|-------|---------|---------------|
| Session header — mask name | tap | Mask picker (escape hatch: pin a mask for the session) |
| Session header — mask name | triple-tap | STT dev overlay (on-device `metal`/`rtf`/latency numbers) |
| Bellows bed | tap | Bank the utterance (send what was heard) |
| Furnace — "N days tended" line | long-press (1.2 s) | Vault export: share sheet with athanor-vault-<date>.md (core-rendered, everything the store holds) + athanor-<date>.sqlite (consistent raw copy). Store-only: works without a key. AirDrop to the Mac. QA: launch arg export-vault=1. |
