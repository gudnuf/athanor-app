//! `cargo run -p evals --example eval` — replays the four scripted personas
//! through the hermetic runner, prints the human summary table, and writes
//! the machine-comparable `SuiteReport` JSON to `report.json`. Dev-side only;
//! never invoked by the shipped app.

use evals::personas::all_personas;
use evals::report::render_table;
use evals::run::run_suite;

#[tokio::main]
async fn main() {
    let personas = all_personas();
    let suite = run_suite(&personas).await;

    print!("{}", render_table(&suite));

    let json = serde_json::to_string_pretty(&suite).expect("serialize SuiteReport");
    std::fs::write("report.json", format!("{json}\n")).expect("write report.json");
    println!("wrote report.json");
}
