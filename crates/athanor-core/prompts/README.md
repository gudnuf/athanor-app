---
pack: mystagogue
file: README
version: v0
date: 2026-07-06
status: v0 — ingested into athanor-core as compiled prompt assets (include_str!)
---

# The Mystagogue Prompt Pack — v0

The Mystagogue's mind as versioned prompt assets. One resident intelligence for
the Athanor app (a furnace you carry): it knows the learner, conducts each
Socratic session, and condenses realizations honestly into salt — in the
learner's own words.

These prompts will be **spoken aloud** (TTS-adjacent reading) and **heard**
through voice sessions. They are written for the ear: short lines, plain rhythm,
warm and terse. And the repo is **public from day one** — prompts are built to be
seen.

## Layout

These assets live under `crates/athanor-core/prompts/` and are compiled into the
core with `include_str!` (no runtime file IO on device). Prompt assembly
(`crates/athanor-core/src/prompt/`) loads them by name.

| File | What it is | When loaded |
|---|---|---|
| `identity.md` | The one mind: knows-you injection, Socratic discipline, sources rule, pacing, reply-register. | Always, first. |
| `condensation.md` | How mercury becomes salt: watch → offer → learner's words → refuse weak → spiral child thread → trace. Names the engine tools. | Always. |
| `masks/philosophus.md` | The Midwife — only asks; asserts no domain fact. | When mask = philosophus. |
| `masks/adamas.md` | The Diamond — rigor; presses, holds paradoxes. | When mask = adamas. |
| `masks/solve.md` | The Frame-Breaker — enters when stuck; answers the wall with a koan. | When mask = solve. |
| `modes/{trace,explain,predict,challenge,design}.md` | The five work modes, one file each. Each carries the shared composition note + selection/drift rules so it stands alone. | The one selected mode is loaded. |
| `initiation.md` | First-launch script — cold start, about the learner, finds the pull. | Session #1 only, replaces normal opening. |
| `judge.md` | Small eval-judge: grades transcripts on the five checks. | Dev-side evals only, not shipped to device. |

The single `modes.md` of the source pack was split into one file per mode at
ingestion; each per-mode file keeps the shared mask-composition note and the
mode-selection/drift rules verbatim so loading one mode carries the whole
guidance it needs.

## Prompt assembly

The engine composes the session system prompt at session start, in this order:

```
identity.md
  + condensation.md
  + {{profile injection}}        # learner_name, how_they_learn, active_domains,
                                 #   recent_salt, ripe_mercury, last_trace, budget
  + modes/<selected-mode>.md
  + one mask file (philosophus | adamas | solve)
  + tool-availability line
```

Restated as the spec puts it: **profile + ripe mercury + mode + mask.** Identity
and condensation are the invariant spine; profile is the knows-you layer; mode is
the kind of work; mask is the voice. Initiation swaps its file in for session #1
and runs with an empty profile.

Assembly is **deterministic**: given the same store state and assets, the
assembled prompt is byte-identical (no timestamps, no now(), no randomness).
Snapshot tests capture the fully-assembled prompt per `(profile, thread, mode,
mask)` so every change to any file is visible in a diff.

## Versioning

- Pack version in each file's YAML header (`version: v0`). Bump the whole pack
  together; individual files may note a `changed:` date when edited within a
  version.
- Prompt changes flow: edit → hermetic evals (structural/deterministic, CI) →
  gated real-API evals (quality, LLM-judge) → judge report checked in → operator
  taste-check against real session traces.
- The judge is versioned *alongside* the pack it grades; changing the judge is
  itself a reviewed change (LLM-judge circularity risk — keep it small, spot-
  check against real traces).

## The four eval personas

Written against these four scripted learners (replayed through the real engine):

| Persona | Behaviour | Primarily tests |
|---|---|---|
| **The eager parroter** | Repeats the Mystagogue's phrasing back as if it were their own realization. | Salt refusal — `fix_salt` must never fire on the Mystagogue's own words. |
| **The stuck one** | Jams: circles one groove, defends a frame that no longer fits. | Solve's entrance and clean exit; frame-break not frame-install. |
| **The tangent-chaser** | Bolts to new topics; won't stay with a thread. | Thread discipline; pacing; landing the plane instead of chasing. |
| **The silent one** | Minimal, terse, long pauses. | Patience; holding silence; not filling the gap by lecturing; pacing without rushing. |

Each persona stresses a specific promise of the pack; a prompt change that
regresses any of them shows up in the graded report before it ships.

## Design tensions resolved in v0

Recorded here because they matter to the eval lane:

1. **Sources rule vs. "only asks."** The rule is conditional on *assert vs.
   elicit*. Philosophus asserts nothing, so it never cites — by design. Adamas
   asserts most, so citation is fully live there. This keeps "cite when
   asserting, not when eliciting" mechanically consistent across masks.
2. **Philosophus made mechanically checkable.** "Emits no declaratives that
   assert domain facts" is drawn as: every declarative's factual content must
   trace to the learner's own prior words/profile; questions are exempt unless
   they smuggle an asserted fact in a presupposition. Gives the judge a clean
   pass/fail.
3. **Spiral invariant as tool-order law.** `fix_salt` is *never* the last tool
   call — always followed by `open_thread`. Stated in the protocol so the
   deterministic checker and the prose agree.
4. **Pacing vs. honesty.** ~15-min landing must not manufacture salt. Resolved:
   no false salt is an explicit, higher rule than "produce a grain"; days tended
   is the score, not grains.
5. **Cold-start vs. knows-you.** Initiation runs with empty placeholders and
   forbids seeding — the emptiness is the subject, and it's the generalization
   test. Identity tells the model that empty placeholders mean genuine
   not-knowing, never fabrication.
6. **Two registers, aural.** Conversational default keeps sessions feeling like
   voice; the serif reading voice is reserved for genuine lessons so the register
   shift itself signals "this is teaching." Both written to be *heard*.
