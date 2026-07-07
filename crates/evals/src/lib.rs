//! Eval harness scaffold for athanor-app (dev-side only — never shipped in
//! the app). Hermetic by default: no network, no clock, no randomness in the
//! deterministic tiers. Any real-API runner (the gated LLM-judge tier) is
//! env/feature-gated OFF and never runs in CI.
//!
//! `normalize` provides the Dice-similarity primitives (copied from the rmp
//! evals crate) used by the salt-refusal grader. `report` defines the
//! timestamp-free, comparable-across-runs report shape.

pub mod grade;
pub mod normalize;
pub mod personas;
pub mod report;
pub mod run;
