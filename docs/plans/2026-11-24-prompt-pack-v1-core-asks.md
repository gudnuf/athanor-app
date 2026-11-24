# Prompt pack v1: what the core has to supply

Prompt pack v1 (see `docs/research/2026-11-24-prompt-pack-v1-from-field-use.md`)
adds rules that the pack can state but can't fully keep alone. Each item
below is a core change the prompt-smith lane did NOT build. The pack is
written so it is correct both before and after the change: each rule is
conditional on the placeholder or tool named here, and says what to do while
it is missing.

## 1. `last_session_ago`: a real elapsed-time value in the injection

**Rule it serves:** v1 rule 1, "you have no clock" (`identity.md` §1, §4).

**Today:** `profile_injection` (`crates/athanor-core/src/prompt/mod.rs`)
renders no "now". `recent_notes` lines carry the UTC day each note was
written (`[YYYY-MM-DD · mask]`), but nothing tells the model what today is, so
any interval it states is a guess. Field use showed it guessing, confidently
and wrongly, in both directions.

**Requirement:**

- Add one injection line after `last_trace:`:
  `last_session_ago: <interval>; their time: <part of day>`.
  - `<interval>` is coarse, spoken, and computed from the most recent
    *closed* session with at least one learner turn: `earlier today`,
    `1 day`, `<n> days` (2–13), `<n> weeks` (2–8), `<n> months`. Sessions
    with zero learner turns do not count as "last time".
  - `<part of day>` is `morning` / `afternoon` / `evening` / `night` in the
    device's local time zone.
  - Missing either value: render the line as `last_session_ago: (not
    supplied)`. The pack reads absent and "(not supplied)" the same way.
- **Determinism stays load-bearing.** `assemble` must not call `now()`.
  Pass the instant in: e.g. `assemble_with_clock(mask, mode, thread, store,
  budget, now: Option<Timestamp>, tz_offset_min: Option<i32>)`, with
  `assemble` passing `None` (so the 16 snapshots and every existing caller
  stay byte-identical). The Conductor passes the real clock at session open.
- Snapshot impact: the 15 session snapshots gain one literal line in the
  profile-injection section. That is an intended structure change; regenerate
  with `UPDATE_SNAPSHOTS=1` and say so in the commit.

**Eval that covers it:** `evals::field::grade_elapsed_time(trace,
known)` already accepts the supplied interval; once the line exists, the
gated tier passes the injected value as `known`.
