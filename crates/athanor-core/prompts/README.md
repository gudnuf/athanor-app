---
pack: mystagogue
file: README
version: v1
date: 2026-07-06
changed: 2026-11-24
status: v1 — v0 plus eight rules from field use; compiled assets (include_str!)
---

# The Mystagogue Prompt Pack — v1

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

## v1: nine rules from field use

v1 keeps v0 whole and adds nine short rules, each from a failure shape that
only showed up in real sessions. The full write-up (rule, synthetic pattern,
eval, gated judge question) is
`docs/research/2026-11-24-prompt-pack-v1-from-field-use.md`; what the core
must still supply is `docs/plans/2026-11-24-prompt-pack-v1-core-asks.md`.

| # | Rule | Where |
|---|---|---|
| 1 | You have no clock: no interval or time of day unless `{{last_session_ago}}` gives it. | `identity.md` §1, §4 |
| 2 | An empty turn isn't a turn: two in a row, leave the door open and land. | `identity.md` §2; `condensation.md` tools; `condense.md` |
| 3 | One question per reply; the last thing you say is not a question. | `identity.md` §2, §5 |
| 4 | Ration the either/or: at most one a session; a pick isn't salt. | `identity.md` §2; `condensation.md` §4 |
| 5 | A question met twice with silence has stalled: let it rest, don't open with it. | `identity.md` §2, §4; `condense.md` |
| 6 | An own-words mechanism, even mid-tangent, is the moment to offer; a rhyme said aloud gets woven. | `condensation.md` §1, weaving |
| 7 | "Save that" keeps their words verbatim, one snag question at most. | `condensation.md` tools; `condense.md` |
| 8 | Not every voice is for you: ask once, build nothing on room speech. | `identity.md` §2; `condense.md` |
| 9 | Never infer how they learn from silence, short turns or gaps; one session is not a pattern. | `condense.md`; `condensation.md` tools |

Each rule has two hermetic checks in `crates/evals`:

- **Pack contract** (`evals::pack::FIELD_RULES`): the assembled prompt (or
  the close-only condense prompt) must carry the rule's anchor words. The 16
  structure snapshots redact prose by design; this is what stops a rewording
  from silently dropping a rule. Reword freely around the anchors; changing
  an anchor is a deliberate edit to that table.
- **Behavioural grader** (`evals::field`): a deterministic surface proxy
  over a session trace, run by a synthetic field persona (rule 9's grader
  reads the close-pass output instead, through the real `parse_condensation`). Hermetically the
  Mystagogue side is scripted, so these prove the grader bites (each has a
  test where the v0-shaped reply fails it); the gated tier runs the same
  graders over real-engine transcripts.

The field personas, alongside the four above:

| Persona | Behaviour | Rule |
|---|---|---|
| **Dead air** | Sends empty turns (dots). | 2 |
| **The leaver** | Works a turn, then says they have to go. | 3 |
| **The fork-picker** | Answers every either/or with a pick and nothing more. | 4 |
| **The returner** | Comes back after a note marked a question stalled. | 5 |
| **The explainer** | Explains a mechanism in their own words, inside a tangent. | 6 |
| **The banker** | Drops an idea and asks to save it. | 7 |
| **Room noise** | Talks to someone else in the room mid-session. | 8 |

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
3. **Spiral invariant as tool-order law.** Every `fix_salt` names exactly one
   child as its required `child_question`, which the core opens inside
   `fix_salt`. A following `open_thread` for the same question is a twin. (v1: v0 said "always followed by `open_thread`", which the core's
   `child_question` path made a duplicate-maker.) Stated in the protocol so
   the deterministic checker and the prose agree.
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
