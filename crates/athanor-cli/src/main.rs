//! `athanor-cli` binary — parse args, wire the runner, render output.
//!
//! No business logic: this is a shell over `athanor_cli`'s orchestration, which
//! is itself a shell over `athanor-core`. `main` does argument parsing and I/O
//! only.

use std::process::ExitCode;
use std::sync::Arc;

use athanor_cli::{run_scripted_session, script::parse_script};
use athanor_core::engine::AcpUpdate;
use athanor_core::Store;

const USAGE: &str = "\
athanor-cli — dev harness over athanor-core

USAGE:
    athanor-cli session [OPTIONS]
    athanor-cli seed --from <ACADEMY_DIR> --db <PATH>

SESSION OPTIONS:
    --mask <MASK>     philosophus | adamas | solve   (default: philosophus)
    --mode <MODE>     trace | explain | predict | challenge | design  (default: explain)
    --thread <ID>     focal thread id (optional)
    --script <PATH>   JSON session script for the hermetic MockEngine
    --db <PATH>       sqlite path (default: in-memory)
    --goose           use the real engine (build with --features goose; reads
                      ANTHROPIC_API_KEY from the environment at runtime)

SEED OPTIONS (lived-in demo — writes a demo db through the real store APIs):
    --from <DIR>      academy directory (domains/, grimoire/, STATE.md, profile/).
                      Use for the operator's PRIVATE lived seed (git-ignored db).
    --profile <NAME>  a committed demo persona instead of --from (known: normy) —
                      fiction, safe to ship; source lives in fixtures/<NAME>/.
    --db <PATH>       sqlite path to write

    -h, --help        print this help
";

/// A minimal built-in script so `athanor-cli session` runs out of the box
/// without a --script file (a greeting, then a clean landing).
const DEFAULT_SCRIPT: &str = r#"[
    { "text": "The furnace is warm. What have you been chewing on?" },
    { "complete": true }
]"#;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("athanor-cli: {err}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);

    let command = match args.next() {
        Some(c) => c,
        None => {
            print!("{USAGE}");
            return Ok(());
        }
    };
    if command == "-h" || command == "--help" {
        print!("{USAGE}");
        return Ok(());
    }
    if command == "seed" {
        return run_seed(args);
    }
    if command != "session" {
        return Err(format!("unknown command '{command}'\n\n{USAGE}").into());
    }

    let mut mask = "philosophus".to_string();
    let mut mode = "explain".to_string();
    let mut thread: Option<String> = None;
    let mut script_path: Option<String> = None;
    let mut db_path: Option<String> = None;
    let mut goose = false;
    let mut turns: Vec<String> = Vec::new();

    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--mask" => mask = next_value(&mut args, "--mask")?,
            "--mode" => mode = next_value(&mut args, "--mode")?,
            "--thread" => thread = Some(next_value(&mut args, "--thread")?),
            "--turn" => turns.push(next_value(&mut args, "--turn")?),
            "--script" => script_path = Some(next_value(&mut args, "--script")?),
            "--db" => db_path = Some(next_value(&mut args, "--db")?),
            "--goose" => goose = true,
            "-h" | "--help" => {
                print!("{USAGE}");
                return Ok(());
            }
            other => return Err(format!("unknown option '{other}'\n\n{USAGE}").into()),
        }
    }

    let store = match &db_path {
        Some(path) => Store::open(path, "cli")?,
        None => Store::open_in_memory("cli")?,
    };
    let store = Arc::new(store);

    let mut stdout = std::io::stdout();

    if goose {
        return run_goose(store, &mask, &mode, thread.as_deref(), &turns, &mut stdout).await;
    }

    let script: Vec<AcpUpdate> = match &script_path {
        Some(path) => parse_script(&std::fs::read_to_string(path)?)?,
        None => parse_script(DEFAULT_SCRIPT)?,
    };

    let outcome =
        run_scripted_session(store, &mask, &mode, thread.as_deref(), script, &mut stdout).await?;
    eprintln!(
        "\n[session {} landed={} tools={:?}]",
        outcome.session_id, outcome.landed, outcome.tools_called
    );
    Ok(())
}

/// `athanor-cli seed --from <academy> --db <path>` — build the lived-in demo
/// db. Winds a `SeedClock` back to each entry's date so history lands in the
/// past, then translates the real academy markdown through the store APIs.
/// Prints COUNTS ONLY (no personal content) so the output is safe to log.
fn run_seed(mut args: impl Iterator<Item = String>) -> Result<(), Box<dyn std::error::Error>> {
    use athanor_cli::seed::{profiles, seed_from, SeedClock};

    let mut from: Option<String> = None;
    let mut profile: Option<String> = None;
    let mut db_path: Option<String> = None;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--from" => from = Some(next_value(&mut args, "--from")?),
            "--profile" => profile = Some(next_value(&mut args, "--profile")?),
            "--db" => db_path = Some(next_value(&mut args, "--db")?),
            "-h" | "--help" => {
                print!("{USAGE}");
                return Ok(());
            }
            other => return Err(format!("unknown seed option '{other}'\n\n{USAGE}").into()),
        }
    }
    let db_path = db_path.ok_or("seed requires --db <PATH>")?;

    // Source academy tree: either an on-disk --from directory (the private
    // lived seed) or an embedded --profile persona materialized to a temp tree
    // (a committed demo). Both feed the identical `seed_from` path below.
    let (academy_dir, _tmp): (std::path::PathBuf, Option<TempDir>) = match (from, profile) {
        (Some(dir), None) => (dir.into(), None),
        (None, Some(name)) => {
            let persona = profiles::by_name(&name).ok_or_else(|| {
                format!(
                    "unknown --profile '{name}' (known: {})",
                    profiles::known_names()
                )
            })?;
            let dir =
                std::env::temp_dir().join(format!("athanor-seed-{name}-{}", std::process::id()));
            persona.materialize(&dir)?;
            (dir.clone(), Some(TempDir(dir)))
        }
        (Some(_), Some(_)) => return Err("use either --from or --profile, not both".into()),
        (None, None) => return Err("seed requires --from <DIR> or --profile <NAME>".into()),
    };

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let clock = SeedClock::new(now);
    let store = Store::open(&db_path, "seed")?.with_clock(clock.clock());

    let report = seed_from(&store, &clock, &academy_dir)?;
    println!(
        "seeded {db_path}:\n  domains={} realizations={} spiral_children={} \
open_threads={} condensing={} correspondences={} tending_days={} \
profile_sections={} kindled_passages={} skipped={}",
        report.domains,
        report.realizations,
        report.spiral_children,
        report.open_threads,
        report.condensing_promoted,
        report.correspondences,
        report.tending_days,
        report.profile_sections,
        report.kindled_passages,
        report.skipped,
    );
    Ok(())
}

fn next_value(
    args: &mut impl Iterator<Item = String>,
    flag: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    args.next()
        .ok_or_else(|| format!("{flag} requires a value").into())
}

/// Removes a materialized persona's temp academy tree when seeding finishes
/// (or errors out), so `--profile` leaves nothing behind in the temp dir.
struct TempDir(std::path::PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(feature = "goose")]
async fn run_goose(
    store: Arc<Store>,
    mask: &str,
    mode: &str,
    thread: Option<&str>,
    turns: &[String],
    out: &mut (dyn std::io::Write + Send),
) -> Result<(), Box<dyn std::error::Error>> {
    use athanor_core::engine::GooseEngine;
    let key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| "ANTHROPIC_API_KEY must be set in the environment for --goose")?;
    let engine = GooseEngine::new(key, None);
    let outcome = if turns.is_empty() {
        athanor_cli::run_session(store, &engine, mask, mode, thread, out).await?
    } else {
        athanor_cli::run_turns(store, &engine, mask, mode, thread, turns, out).await?
    };
    eprintln!(
        "\n[session {} landed={} tools={:?}]",
        outcome.session_id, outcome.landed, outcome.tools_called
    );
    Ok(())
}

#[cfg(not(feature = "goose"))]
async fn run_goose(
    _store: Arc<Store>,
    _mask: &str,
    _mode: &str,
    _thread: Option<&str>,
    _turns: &[String],
    _out: &mut (dyn std::io::Write + Send),
) -> Result<(), Box<dyn std::error::Error>> {
    Err("--goose requires building with --features goose".into())
}
