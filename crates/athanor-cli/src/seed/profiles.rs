//! Committed demo personas — fiction, safe to ship in the public repo.
//!
//! Unlike the operator's private lived seed (read from a git-ignored academy
//! directory at runtime), these personas are invented, and their source
//! markdown is checked into `fixtures/`. It is embedded here with `include_str!`
//! so `athanor-cli seed --profile <name>` works standalone — the binary carries
//! the fixture. On use, a persona is materialized to a temporary academy tree
//! and run through the exact SAME `seed_from` path as the lived seed: same
//! parsers, same store APIs, same parity kindles. Nothing persona-specific
//! lives in the seeding logic — only the source text differs.

use std::io;
use std::path::Path;

/// One embedded persona: a display name and the (relative-path, contents) of
/// every source file, laid out in the academy-tree shape `seed_from` reads.
pub struct Profile {
    pub name: &'static str,
    pub files: &'static [(&'static str, &'static str)],
}

/// "normy" — Sam, a home baker about three weeks into sourdough. Plain,
/// concrete, a little messy; NO alchemical vocabulary anywhere in the learner's
/// own material (that is the whole point of the demo — does the frame hold for
/// someone who has never heard of nigredo?). The Mystagogue's voice is
/// untouched; only this input changes.
pub const NORMY: Profile = Profile {
    name: "normy",
    files: &[
        (
            "grimoire/journal.md",
            include_str!("../../../../fixtures/normy/grimoire/journal.md"),
        ),
        (
            "STATE.md",
            include_str!("../../../../fixtures/normy/STATE.md"),
        ),
        (
            "profile/learner.md",
            include_str!("../../../../fixtures/normy/profile/learner.md"),
        ),
        (
            "domains/starter/correspondences.md",
            include_str!("../../../../fixtures/normy/domains/starter/correspondences.md"),
        ),
        (
            "domains/proofing/correspondences.md",
            include_str!("../../../../fixtures/normy/domains/proofing/correspondences.md"),
        ),
        (
            "domains/scoring/correspondences.md",
            include_str!("../../../../fixtures/normy/domains/scoring/correspondences.md"),
        ),
        (
            "domains/crumb/correspondences.md",
            include_str!("../../../../fixtures/normy/domains/crumb/correspondences.md"),
        ),
    ],
};

/// Every embedded persona, for `--profile` resolution and help text.
pub const ALL: &[&Profile] = &[&NORMY];

/// Resolve a persona by name (`--profile <name>`).
pub fn by_name(name: &str) -> Option<&'static Profile> {
    ALL.iter().copied().find(|p| p.name == name)
}

/// Comma-separated known names, for error messages.
pub fn known_names() -> String {
    ALL.iter().map(|p| p.name).collect::<Vec<_>>().join(", ")
}

impl Profile {
    /// Write this persona's source tree under `root` (creating parent dirs),
    /// reproducing the on-disk academy layout `seed_from` expects.
    pub fn materialize(&self, root: &Path) -> io::Result<()> {
        for (rel, contents) in self.files {
            let path = root.join(rel);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(path, contents)?;
        }
        Ok(())
    }
}
