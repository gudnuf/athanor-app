# Lived-in demo seed — mapping

How the Academy's real working state (markdown under an academy directory) is
translated into the app's store schema, so the app can be seen as it looks in
active use rather than with toy fixtures.

**This document describes SHAPES, not substance.** No personal content from the
source material appears here, in the translation code, or in tests. The
translation script reads the real academy paths *at runtime*; the generated
database is never committed (git-ignored by exact path and pattern — see
`.gitignore`: `seeds/`, `*.seed.sqlite`, `lived-seed.*`, etc.). Tests use tiny
synthetic markdown samples invented for the purpose.

## Source → schema map

| Source markdown (shape) | Store write (real API) | Result in app |
|---|---|---|
| `domains/<name>/` subdirectories | `upsert_domain(name)` | Sulfur: the domains a learner tends |
| `grimoire/journal.md` — dated `## YYYY-MM-DD — title` entries, prose body in the practitioner's own voice | `open_thread(theme)` → `fix_salt(thread, body, domains, child_q)` | Salt: each entry is an immutable realization on the **Grimoire** shelf, dated in the past |
| The `open:` bullets inside a journal entry | passed as `fix_salt`'s `child_question` | The spiral child thread born of that realization |
| (every realization, unconditionally) | `fix_salt` births one child thread | Mercury: the "what does this open?" next question — the spiral |
| `STATE.md` open-question / "Open Threads" bullets | `open_thread(question)` (volatile) | Mercury: open questions with real pull |
| oldest two open threads | `set_thread_state(Condensing)` | Mercury shows questions at different stages of settling |
| `domains/<name>/correspondences.md` — `## To <Other>` blocks | `upsert_domain(other)` + `weave_domains(self, other, note)` | Azoth: cross-domain correspondences (incl. links to not-yet-domains) |
| `STATE.md` "Recent Sessions" dated bullets (`~NNmin` if present) | `record_tending(day, minutes)` with the clock wound to that day | Fire: the Furnace's real **wisdom days** count |
| `profile/learner.md` + `STATE.md` sections | `set_profile_section(section, body)` | Learner profile (how_i_learn, frictions, pulls, domains, working_history) |
| (side effect of the first `fix_salt`) | `kindle_passage("SALT")` | Tabula: the SALT passage kindled, as in live use |

## Why these writes, not raw inserts

Every row is written through the **real store API**, never raw SQL. So the
seeded data satisfies exactly the invariants live data does:

- **The spiral.** `fix_salt` is the sole writer of realizations and, in one
  transaction, (a) writes the immutable realization, (b) births its child
  thread, (c) back-links them, (d) condenses-then-fixes the parent, (e) kindles
  SALT. A journal entry can't become a realization without opening the next
  question — the spiral is structural, and the seed inherits it.
- **Thread-state DAG.** Parent threads reach `Fixed` only via the legal
  `Volatile → Condensing → Fixed` path inside `fix_salt`; promoted open threads
  use `set_thread_state`, which rejects illegal moves.
- **Immutability & append-only.** Realizations and tending are written only via
  their real entry points; nothing back-dates by mutating rows.
- **Wisdom-by-day.** Tending is one row per UTC day; re-recording a day merges.
  Wisdom = distinct days, computed the same way the Furnace computes it.

## Historic dates

The store stamps `now()` on writes. To make history land in the past, the
translator injects a **settable clock** (`SeedClock`, an `AtomicU64` behind
`Store::with_clock`) and winds it to each entry's UTC-midnight epoch before the
write. So realization `date`s and thread `born`s are the journal/session dates,
and the Grimoire and Furnace read as a genuine history rather than a burst of
"today". Undated writes (parsed open threads, profile) use the real system
clock so they sort after the historic material.

## Idempotency (natural keys)

Re-running the seed on an existing db must not duplicate. Before writing, the
translator loads the existing natural keys and skips anything already present:

- **Realizations** — by exact realization text (`list_realizations`).
- **Threads** — by prompt (`open_threads`), plus parent threads are gated by
  the realization-text check above.
- **Tending** — by day (`tending_days`).
- **Correspondences** — by `(other_domain_id, note)` (`list_correspondences`).
- **Domains / profile** — `upsert_domain` and `set_profile_section` are
  inherently idempotent (upsert by name / by section).

A second run therefore reports zero new realizations/threads/tending/
correspondences (profile sections are simply re-set, harmless).

## Judgment calls

- **Journal entry → realization + a synthesized parent thread.** Journals record
  the realization but not the originating question. The translator opens a
  parent thread named after the entry's theme (title, or `the work of <date>`),
  then fixes salt on it. The parent becomes `Fixed` (correctly hidden from
  Mercury, which shows only open questions); the realization carries the entry
  body as its salt.
- **Domain classification by keyword.** Entries are linked to domains by a small
  per-domain keyword set (the domain name plus synonyms). An entry that matches
  nothing gets no domain link (a realization with no domain is valid). This is
  deliberately conservative — better an unlinked realization than a wrong link.
- **Correspondences to not-yet-domains.** A `## To Plasma Physics (not yet a
  domain)` block creates the "other" domain via `upsert_domain` and weaves to
  it. This faithfully represents a correspondence reaching toward an interest
  that hasn't been formalized — and is why the seeded domain count exceeds the
  number of `domains/` directories.
- **Condensing promotion.** The spiral children and parsed open questions are
  all volatile. To show Mercury with questions at mixed stages, the oldest two
  open threads are promoted to `Condensing`. A defensible narrative: older
  questions have settled further.
- **Session minutes.** Parsed from a `~NNmin` marker when present, else a nominal
  15. Minutes are secondary — wisdom counts *days*, and the day is exact.

## Deliberately left out

- **Spaced-repetition queue / assessments** (`assessment.md`, STATE's SR queue):
  the store has no schema for review scheduling or per-concept confidence, so
  these are not translated. (A future `assessments` table could carry them.)
- **Curriculum / semester plans / reading lists / lexicons:** no target schema;
  omitted rather than forced into `pull_notes`.
- **Session transcripts:** the seed produces the *state* a history of sessions
  leaves behind (tending, realizations, threads), not fabricated transcript
  text. Transcripts are per-session dialogue, not lived-in reads.
- **The Mystagogue's own commentary** inside journal entries is kept as part of
  the realization body (it's what the practitioner chose to record), not split
  into a separate row.

## Where it lives

- Translation script: `crates/athanor-cli/src/seed/` (`parse.rs` pure parsers +
  `translate.rs` orchestration). Dev-side, in the CLI — it parses filesystem
  markdown and reads academy paths, concerns that must never enter
  `athanor-core` (which compiles to the FFI/mobile surface). It reimplements no
  store operation; it only drives real `Store` methods, exactly as `script.rs`'s
  parsing lives in the CLI.
- Run: `athanor-cli seed --from <academy-dir> --db <path>`. The `--db` path must
  be one of the git-ignored seed patterns.
- App: a `seed-db=<path>` launch argument (the established QA-hook pattern,
  mirroring `screen=`) overrides the real engine's on-device store path so a
  build opens the seeded db. Runs the **real** engine (reads render seeded
  state); `DemoEngine` is untouched. See `RealEngineLoader.databasePath()`.
