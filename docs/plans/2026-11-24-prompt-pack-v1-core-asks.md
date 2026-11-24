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

## 2. `bank_pull`: a tool that stores the learner's words as a pull note

**Rule it serves:** v1 rule 7, "keep it in their words" (`condensation.md`,
after the tool table).

**Today:** the store has `Store::add_pull_note(domain_id, text)` and a
`pull_notes` table, but no Mystagogue tool reaches it. When a learner says
"save that", the only ways to store it are `open_thread` (a question, which
an idea isn't) or `update_memory("pulls", …)` (see ask 3). Field use: a keep
request was answered "saved", the Mystagogue's paraphrase went into the
profile, and `pull_notes` stayed at 0.

**Requirement:**

- New tool in `Mystagogue::tool_specs`: `bank_pull { text: string, domain?:
  string }`. `text` description: "the learner's own sentence, verbatim or
  lightly cleaned for legibility. Never a summary." Dispatch:
  `add_pull_note(domain_id_or_null, text)`; returns `{ id, ok }`.
- Order it after `open_thread` in the specs so the tool-availability line
  lists it (the pack keys on the name being listed: "Use `bank_pull` if this
  session's tools list it").
- Snapshot impact: the `tools` line in all 16 snapshots gains `bank_pull`.
  Intended; regenerate with `UPDATE_SNAPSHOTS=1`.
- Until it lands the pack falls back to `open_thread` in the learner's words.

**Eval that covers it:** `evals::field::grade_bank_verbatim` already treats
`bank_pull.text` as a storing arg (60% of stored tokens must come from the
learner's turn).

## 3. `update_memory` replaces a whole profile section

**Rule it serves:** v1 rules 2 and 7 (memory is written at the landing, in
their words).

**Today:** the `update_memory` tool calls `Store::set_profile_section`, an
upsert that REPLACES the section's content. The close-only condense pass uses
the merge path (`Store::merge_profile_section`: append, dedupe). So a single
mid-session `update_memory("pulls", "<one new line>")` silently erases every
earlier pull in that section. The model is not told this; the v1 pack now
says it in one parenthetical, as a stopgap.

**Requirement (pick one, operator's call):**

- (a) make the tool merge (same semantics as the condense pass) and keep a
  separate explicit `replace: true` for rewrites; or
- (b) drop `update_memory` from the live tool list entirely and let the
  condense pass own profile writes (v1 rule 2 already says memory is for the
  landing).

Either way, remove the parenthetical in `condensation.md` once it's no longer
true.
