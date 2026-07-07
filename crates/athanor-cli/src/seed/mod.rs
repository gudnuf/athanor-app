//! Lived-in demo seed (dev-side). Parses gudnuf's real academy markdown at
//! runtime and writes it into a store through the REAL store APIs, so the app
//! can be seen as it looks in active use.
//!
//! **Why this lives in the CLI, not the core.** The seeder parses filesystem
//! markdown and reads `~/athanor` paths — dev-only concerns that must never
//! ship inside `athanor-core` (which compiles to the FFI/mobile surface). It
//! reimplements no store operation: `translate.rs` only *drives* the real
//! `Store` methods (`upsert_domain`, `open_thread`, `fix_salt`, `weave_domains`,
//! `record_tending`, `set_profile_section`), exactly as `script.rs`'s parsing
//! lives here rather than in the core. The rmp invariant holds — no business
//! logic here, only parsing + orchestration.
//!
//! **Privacy.** The DB this writes contains personal material and is git-ignored
//! by path and pattern (see `.gitignore` + docs/research/lived-seed-mapping.md).
//! Only this script is committed; the data never is. All tests use invented
//! synthetic samples.
//!
//! **Demo personas.** `profiles` holds committable FICTION (e.g. `normy`) whose
//! source markdown lives in `fixtures/` and is embedded into the binary — so the
//! public repo carries a shippable, personal-data-free demo that seeds through
//! this exact same path.

pub mod parse;
pub mod profiles;
pub mod translate;

pub use profiles::Profile;
pub use translate::{seed_from, SeedClock, SeedError, SeedReport};
