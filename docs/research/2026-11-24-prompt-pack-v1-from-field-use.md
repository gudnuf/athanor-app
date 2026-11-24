# Prompt pack v1: eight rules from field use

The v0 pack was written against four scripted personas. v1 adds eight rules
drawn from a private review of real sessions on an early build. That review
and the session data it read stay off the repo. Everything here is stated as
a pattern, and every example is invented (a learner working on why the tide
waits for the moon is the running fiction; it was not anyone's thread).

The process for each rule was: write the hermetic check first and watch it
fail on the v0 pack, make the smallest pack edit in the pack's own voice, go
green, and commit. What's left is the gated real-API run (operator-run) and
the operator's taste check. The judge questions below are what that run
should answer.

## How the checks are built

- **Pack contract** (`crates/evals/src/pack.rs`, `FIELD_RULES`). Each rule
  names the surface that must carry it (the session prompt under a given
  mask, or the close-only condense prompt) and a few anchor words. All 8
  failed on v0 and pass on v1. This is the only hermetic check that looks at
  the pack itself: the 16 structure snapshots redact prose on purpose, so no
  snapshot changed in this work.
- **Behavioural graders** (`crates/evals/src/field.rs`). These are
  deterministic surface proxies over a `SessionTrace`. Each one is run by a
  synthetic field persona (`personas.rs`) whose scripted Mystagogue follows
  v1, and each has a test (`run.rs`) that swaps in the v0-shaped reply on the
  same learner lines and shows the grader failing.
- **Limit, stated plainly.** In the hermetic tier the Mystagogue side is
  scripted, so these graders prove they bite, not that the model obeys.
  Whether the model obeys is the gated tier's job: the persona learner lines
  replayed against the real engine, with the same `field` graders plus the
  judge questions below. The gated runner in `evals::run` is still a stub
  (`todo!()` behind `#[ignore]` and `ANTHROPIC_API_KEY`). Wiring it to the
  real engine is a prerequisite for the operator run.

## The rules

### 1. You have no clock

**Rule.** Never state how long it's been or what part of the learner's day it
is unless `{{last_session_ago}}` supplies it. If it's absent, say "last
time". A date on a note is when the note was written, not today.
(`identity.md` §1, §4.)

**Pattern.** The injection carries note dates but no "now", so the model
fills the gap: *"Four days since you raised the tide question"* in a session
that started an hour after the last one. Field use showed this guess running
in both directions: days inside a single sitting, weeks for days, a morning
for an hour ago.

**Eval.** `pack:1_elapsed_time`; `grade_elapsed_time(trace, known)` flags
any interval or time-of-day claim that differs from the injected value
(`None` today, so every claim fails). It runs in the dead_air, leaver and
returner personas.

**Core ask.** The core must supply `last_session_ago` (core-asks §1). Until
then the rule falls back to "last time".

**Judge question.** Did any reply state or imply a duration or a time of
day ("it's been a while", "late for you") that the context did not give?

### 2. An empty turn isn't a turn

**Rule.** Dots or nothing is the mic or a pause. After two in a row, say one
short line that leaves the door open, and land. Don't shrink the question
and ask again, don't say they're back, and write no memory on silence. The
close pass builds no PROFILE line on empty turns. (`identity.md` §2;
`condensation.md` `update_memory` row; `condense.md`.)

**Pattern.** `.` → *"One word is enough?"* → `. . .` → *"You're back. What
did you have for lunch?"* plus an `update_memory` saying they went quiet.
Then "silence is their learning style" lands in the profile, when it was
really the speech path's latency.

**Eval.** `pack:2_empty_turn` and `pack:2_empty_turn_close`;
`grade_empty_turns` (once a run reaches two empty turns: at most two more
replies, none asking; no arrival phrases; no `update_memory` during the run).
Persona: **dead_air**. `grade_mask_fidelity` now skips landing lines, because
otherwise the Philosophus ask-every-turn proxy would punish the landing the
pack asks for.

**Judge question.** After the empty turns, did the Mystagogue leave the door
open and stop? Or did it keep shrinking the question, close and then reopen,
or treat each empty turn as a visit?

### 3. One question per reply; the last turn lands

**Rule.** One question per reply: not two joined by "and", and not a second
one tucked behind a dash. The last thing the Mystagogue says is not a
question. When the learner says they're done, or the budget is spent, it
names what stayed open in their words and stops. (`identity.md` §2, §5.)

**Pattern.** *"Before you go: who holds the other end? And does the sheet
ever tear?"* The learner doesn't answer the last question; they put the
phone down. In the field, most sessions ended this way.

**Eval.** `pack:3_one_question_landing`; `grade_one_question` (at most one
`?` sentence per reply) and `grade_landing` (a done-signal gets a reply
with no question). Persona: **the leaver**.

**Judge question.** Did the last reply name what stayed open in the
learner's words and stop, or did it ask something on the way out? Did any
reply hand the learner a choice of questions?

### 4. Ration the either/or

**Rule.** At most one either/or a session. Two pictures to pick from is the
Mystagogue choosing the frame, and a pick is not a thought. If the learner
turns both down and reaches for a third, follow that. A picture the
Mystagogue offered stays its own after the learner picks it, and it fixes
nothing until they carry it further in their own words. (`identity.md` §2
for all masks; `condensation.md` §4.)

**Pattern.** *"Is the moon pulling, or is the water leaning?"* → "the first
one" → *"A rope or a sheet?"* → "the second" → `fix_salt("the moon pulls a
sheet of water")`. The answers come fast, so it feels productive, but every
image came from the Mystagogue. In the field the learner's strongest moves
came from rejecting a fork.

**Eval.** `pack:4_ration_forks`, checked on the Adamas surface because forks
appeared under every mask; `grade_forks` (more than one either/or question,
or `fix_salt` straight after a short pick). Persona: **the fork-picker**.

**Judge question.** Did the learner's best images come from their own
words or from options the Mystagogue offered? Does any fixed salt rest on an
offered option the learner only picked?

### 5. A stalled question rests

**Rule.** A question asked twice and met with silence or a sidestep has
stalled, so let it rest. The close pass names it in the NOTE and calls it
stalled. When the last note marks a question stalled, the next session does
not open with it: it comes at it sideways or opens something else.
(`identity.md` §2, §4; `condense.md`.)

**Pattern.** *"What's holding the other end?"* is asked three times in one
session, then comes back as the next session's first line. The learner
ignores it and arrives somewhere else. Confidence is lower here, because some
of the field "silences" were empty voice turns (rule 2). Re-asking as the
next opener is still model behaviour on any input path.

**Eval.** `pack:5_stalled_session` and `pack:5_stalled_close`;
`grade_no_reask(trace, stalled)` (the opening re-asks a stalled question at
Dice ≥ 0.5, or any question is asked a third time at Dice ≥ 0.7). Persona:
**the returner**. Plus a two-session plumbing test: a stalled mark in a
close note reaches the next session's `recent_notes`.

**Judge question.** Given a prior note marking a question stalled, did the
opening come at it sideways or open something else, rather than ask it
again in new words?

### 6. Offer on an own-words mechanism; weave a spoken rhyme

**Rule.** A learner explaining *how* something works in their own words is
the shift, even mid-tangent and even about something else, so offer. When
the Mystagogue hears itself name a rhyme across domains, it puts it to the
learner and calls `weave_domains` if they take it up. A rhyme said aloud and
never recorded is lost. (`condensation.md` §1, Kindling & weaving.)

**Pattern.** Mid-tangent, the learner explains why a sourdough starter
rises hours after feeding. The v0 reply is *"Interesting. Back to the
tide."* Later the Mystagogue says *"same shape as the tide"*, the learner
agrees, and nothing is recorded. The store ends with zero correspondences.

**Eval.** `pack:6_offer_and_weave`; `grade_offer_after(trace, mechanism)`
(an offer phrase or `fix_salt` within two replies of a known own-words
mechanism turn) and `grade_weave_on_rhyme` (a rhyme claim needs
`weave_domains` in that reply or the next). Persona: **the explainer**.
The rhyme phrase list is narrow on purpose: "you keep coming back to that
word" is a within-session reflection, not a rhyme.

**Judge question.** When the learner explained how something works in their
own words, did the Mystagogue notice and offer, or steer back to its own
thread? Was any recorded weave one the learner actually took up?

### 7. "Save that" keeps their words

**Rule.** When the learner asks to keep something, keep it in their words:
`bank_pull` if this session's tools list it, otherwise `open_thread` with
their sentence close to verbatim. Not a summary, and not `update_memory`.
Say it's kept, ask one snag question at most, then stop. A `pulls` line in
the close pass quotes them or isn't written. (`condensation.md` after the
tool table; `condense.md`.)

**Pattern.** *"…a tide clock app that buzzes when the water turns. Save
that."* The v0 reply stores *"Event-driven ambient notification versus
scheduled alerting"* and asks two questions. It is credited as the
learner's, and nothing lands in pull notes.

**Eval.** `pack:7_keep_in_their_words` and `pack:7_pulls_quote_them`;
`grade_bank_verbatim` (at least 60% of the stored text's tokens come from the
learner's turn; storing it as salt, storing nothing, or asking more than one
question all fail). Persona: **the banker**.

**Core asks.** Core-asks §2 is a `bank_pull` tool over the existing
`add_pull_note`. Core-asks §3: `update_memory` replaces whole sections today.

**Judge question.** Does the stored text read as the learner's sentence, or
as the Mystagogue's summary of it?

### 8. Not every voice is for you

**Rule.** The mic hears the room. When a turn is a greeting, a "they" the
Mystagogue can't place, or someone being told about the app, it asks once,
lightly, whether that was for it. It doesn't answer the turn and builds
nothing on it. The close pass rests no NOTE or PROFILE line on such a turn.
(`identity.md` §2; `condense.md`.)

**Pattern.** *"They think it's a meditation app but it's more like a
sparring partner"*, said to someone across the room. The v0 reply answers
it, and then `how_i_learn` gains a line about how the learner sees the app.

**Eval.** `pack:8_room_speech_session` and `pack:8_room_speech_close`;
`grade_room_speech(trace, room)` (exactly one addressee check, no reply
answering a room line's content, no `update_memory` drawn from one).
Persona: **room noise**.

**Judge question.** This one covers the close NOTE and PROFILE too, which
the live trace doesn't carry: did anything the Mystagogue said, noted or
profiled rest on a turn that was plausibly for someone else?

## Outside the prompt pack

These came out of the same review. They are not prompt changes, and nothing
here builds them:

- **Engine.** Don't send a turn when no speech was heard. Put a separator
  between text blocks that a tool call splits. Store the real one-line
  trace per session, not a prefix of the first reply.
- **Store.** Sessions should record their thread, `fix_salt` should bind to
  the thread actually worked, and tending should record minutes and
  threads. The cold room can't be computed honestly until this is done.
- **Solve.** v0 field use broke "one koan, then silence" and "never
  announce". Re-run the Solve scenarios on the gated tier against the
  current model before assuming it's fixed.
- **Operator, one-time.** Rules 2 and 8 stop new latency artifacts and
  room-speech inferences from entering the profile. They don't remove ones
  already stored on a device. Retracting those is an operator task on the
  device's own data.
