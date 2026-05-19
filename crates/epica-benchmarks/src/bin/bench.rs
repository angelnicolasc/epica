//! `epica-bench` — run the Sprint-4 benchmark suites and emit CSV +
//! Markdown reports.
//!
//! Two subcommands:
//!
//! - `run`: execute a suite (`alfworld_like` or `webshop_like`) for N
//!   trajectories and write `<suite>_per_trajectory.csv`,
//!   `<suite>_summary.csv`, and `<suite>.md` to `--out-dir`.
//! - `run-all`: shorthand for `run alfworld_like && run webshop_like`
//!   into the same directory.
//!
//! The CLI is intentionally thin — all the work lives in the library.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use epica_benchmarks::{
    harness::{run_suite, HarnessConfig},
    reporters,
    traces::Suite,
};

#[derive(Debug, Parser)]
#[command(
    name = "epica-bench",
    version,
    about = "Run Epica benchmark suites and emit CSV / Markdown reports.",
    long_about = None,
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run a single suite.
    Run {
        /// Suite slug: `alfworld_like` | `webshop_like`.
        #[arg(long)]
        suite: Suite,
        /// Number of trajectories.
        #[arg(long, default_value_t = 50)]
        trajectories: u32,
        /// Output directory. Created if missing.
        #[arg(long)]
        out_dir: PathBuf,
        /// Disable the active-inference monitor for this run.
        #[arg(long, default_value_t = false)]
        no_active_inference: bool,
    },
    /// Run every available synthetic suite into the same `--out-dir`.
    RunAll {
        /// Number of trajectories per suite.
        #[arg(long, default_value_t = 50)]
        trajectories: u32,
        /// Output directory.
        #[arg(long)]
        out_dir: PathBuf,
        /// Disable the active-inference monitor for this run.
        #[arg(long, default_value_t = false)]
        no_active_inference: bool,
    },
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("epica-bench: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<(), String> {
    match cli.command {
        Command::Run {
            suite,
            trajectories,
            out_dir,
            no_active_inference,
        } => {
            let cfg = HarnessConfig {
                enable_active_inference: !no_active_inference,
                ..Default::default()
            };
            run_one(suite, trajectories, &out_dir, &cfg).await
        }
        Command::RunAll {
            trajectories,
            out_dir,
            no_active_inference,
        } => {
            let cfg = HarnessConfig {
                enable_active_inference: !no_active_inference,
                ..Default::default()
            };
            for suite in [Suite::AlfworldLike, Suite::WebshopLike] {
                run_one(suite, trajectories, &out_dir, &cfg).await?;
            }
            Ok(())
        }
    }
}

async fn run_one(
    suite: Suite,
    trajectories: u32,
    out_dir: &PathBuf,
    cfg: &HarnessConfig,
) -> Result<(), String> {
    std::fs::create_dir_all(out_dir)
        .map_err(|e| format!("create out dir {}: {e}", out_dir.display()))?;
    eprintln!(
        "epica-bench: running suite={} trajectories={trajectories} \
         active_inference={}",
        suite, cfg.enable_active_inference
    );
    let report = run_suite(suite, trajectories, cfg).await;

    let per_path = out_dir.join(format!("{}_per_trajectory.csv", suite.slug()));
    let summary_path = out_dir.join(format!("{}_summary.csv", suite.slug()));
    let md_path = out_dir.join(format!("{}.md", suite.slug()));

    reporters::write_per_trajectory_csv(&report, &per_path)
        .map_err(|e| format!("write per-trajectory CSV: {e}"))?;
    reporters::write_summary_csv(&report, &summary_path)
        .map_err(|e| format!("write summary CSV: {e}"))?;
    reporters::write_markdown_summary(&report, &md_path)
        .map_err(|e| format!("write markdown summary: {e}"))?;

    // Echo the markdown to stdout so CI logs surface the numbers.
    print!("{}", reporters::render_markdown_summary(&report));
    eprintln!(
        "epica-bench: wrote {} / {} / {}",
        per_path.display(),
        summary_path.display(),
        md_path.display()
    );
    Ok(())
}
