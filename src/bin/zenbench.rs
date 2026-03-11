#![forbid(unsafe_code)]

use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use zenbench::daemon;

#[derive(Parser)]
#[command(name = "zenbench", about = "Interleaved microbenchmarking harness")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List all benchmark runs (active and completed).
    List {
        /// Project root directory (default: current directory).
        #[arg(short, long, default_value = ".")]
        project: PathBuf,
    },

    /// Show status of a specific run.
    Status {
        /// Run ID to check.
        run_id: String,

        /// Project root directory.
        #[arg(short, long, default_value = ".")]
        project: PathBuf,
    },

    /// Kill a running benchmark.
    Kill {
        /// Run ID to kill, or "stale" to kill all stale runs.
        target: String,

        /// Project root directory.
        #[arg(short, long, default_value = ".")]
        project: PathBuf,
    },

    /// Show results of a completed run.
    Results {
        /// Run ID, or "latest" for most recent.
        run_id: String,

        /// Project root directory.
        #[arg(short, long, default_value = ".")]
        project: PathBuf,

        /// Output as JSON.
        #[arg(long)]
        json: bool,
    },

    /// Compare two result files.
    Compare {
        /// Baseline result file.
        baseline: PathBuf,

        /// Candidate result file.
        candidate: PathBuf,
    },

    /// Clean up old run metadata.
    Clean {
        /// Project root directory.
        #[arg(short, long, default_value = ".")]
        project: PathBuf,

        /// Maximum age of run metadata in hours (default: 168 = 7 days).
        #[arg(long, default_value = "168")]
        max_age_hours: u64,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::List { project } => cmd_list(&project),
        Commands::Status { run_id, project } => cmd_status(&project, &run_id),
        Commands::Kill { target, project } => cmd_kill(&project, &target),
        Commands::Results {
            run_id,
            project,
            json,
        } => cmd_results(&project, &run_id, json),
        Commands::Compare {
            baseline,
            candidate,
        } => cmd_compare(&baseline, &candidate),
        Commands::Clean {
            project,
            max_age_hours,
        } => cmd_clean(&project, max_age_hours),
    }
}

fn cmd_list(project: &Path) {
    match daemon::list_runs(project) {
        Ok(runs) => {
            if runs.is_empty() {
                eprintln!("No benchmark runs found.");
                return;
            }
            println!(
                "{:<20} {:<10} {:<8} {:<12} COMMAND",
                "ID", "STATUS", "PID", "GIT"
            );
            for run in runs {
                let status = format!("{:?}", run.status);
                let git = run.git_hash.as_deref().unwrap_or("-");
                let git_short = if git.len() > 8 { &git[..8] } else { git };
                println!(
                    "{:<20} {:<10} {:<8} {:<12} {}",
                    run.id, status, run.pid, git_short, run.command
                );
            }
        }
        Err(e) => eprintln!("Error listing runs: {e}"),
    }
}

fn cmd_status(project: &Path, run_id: &str) {
    match daemon::load_run_state(project, run_id) {
        Ok(state) => {
            let alive = daemon::is_process_alive(state.pid);
            println!("Run: {}", state.id);
            println!("Status: {:?}", state.status);
            println!(
                "PID: {} ({})",
                state.pid,
                if alive { "alive" } else { "dead" }
            );
            println!("Git: {}", state.git_hash.as_deref().unwrap_or("unknown"));
            println!("Command: {}", state.command);
            if let Some(path) = &state.result_path {
                println!("Results: {}", path.display());
            }
        }
        Err(e) => eprintln!("Error loading run {run_id}: {e}"),
    }
}

fn cmd_kill(project: &Path, target: &str) {
    if target == "stale" {
        let hash = zenbench::platform::git_commit_hash().unwrap_or_default();
        if hash.is_empty() {
            eprintln!("Cannot determine current git hash. Use a specific run ID.");
            return;
        }
        match daemon::kill_stale_runs(project, &hash) {
            Ok(n) => eprintln!("Killed {n} stale run(s)."),
            Err(e) => eprintln!("Error: {e}"),
        }
    } else {
        match daemon::kill_run(project, target) {
            Ok(true) => eprintln!("Killed run {target}."),
            Ok(false) => eprintln!("Run {target} was not running."),
            Err(e) => eprintln!("Error: {e}"),
        }
    }
}

fn cmd_results(project: &Path, run_id: &str, json: bool) {
    let actual_id = if run_id == "latest" {
        match daemon::list_runs(project) {
            Ok(runs) => {
                if let Some(last) = runs.last() {
                    last.id.clone()
                } else {
                    eprintln!("No runs found.");
                    return;
                }
            }
            Err(e) => {
                eprintln!("Error: {e}");
                return;
            }
        }
    } else {
        run_id.to_string()
    };

    match daemon::load_run_state(project, &actual_id) {
        Ok(state) => {
            if let Some(result_path) = &state.result_path {
                match zenbench::SuiteResult::load(result_path) {
                    Ok(result) => {
                        if json {
                            println!("{}", serde_json::to_string_pretty(&result).unwrap());
                        } else {
                            result.print_report();
                        }
                    }
                    Err(e) => eprintln!("Error loading results: {e}"),
                }
            } else {
                eprintln!(
                    "Run {} has no results yet (status: {:?})",
                    actual_id, state.status
                );
            }
        }
        Err(e) => eprintln!("Error: {e}"),
    }
}

fn cmd_compare(baseline_path: &Path, candidate_path: &Path) {
    let baseline = match zenbench::SuiteResult::load(baseline_path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error loading baseline: {e}");
            return;
        }
    };
    let candidate = match zenbench::SuiteResult::load(candidate_path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error loading candidate: {e}");
            return;
        }
    };

    eprintln!("Baseline:  {} ({})", baseline.run_id, baseline.timestamp);
    eprintln!("Candidate: {} ({})", candidate.run_id, candidate.timestamp);
    eprintln!();

    // Compare standalone benchmarks by name
    for cand_bench in &candidate.standalones {
        if let Some(base_bench) = baseline
            .standalones
            .iter()
            .find(|b| b.name == cand_bench.name)
        {
            let pct = if base_bench.summary.mean.abs() > f64::EPSILON {
                (cand_bench.summary.mean - base_bench.summary.mean) / base_bench.summary.mean
                    * 100.0
            } else {
                0.0
            };

            let arrow = if pct < -1.0 {
                "\x1b[32m"
            } else if pct > 1.0 {
                "\x1b[31m"
            } else {
                ""
            };
            let reset = if arrow.is_empty() { "" } else { "\x1b[0m" };

            println!("  {:<30}  {}{:+.2}%{}", cand_bench.name, arrow, pct, reset);
        }
    }
}

fn cmd_clean(project: &Path, max_age_hours: u64) {
    let max_age_secs = max_age_hours * 3600;
    match daemon::cleanup_old_runs(project, max_age_secs) {
        Ok(n) => eprintln!("Cleaned up {n} old run(s)."),
        Err(e) => eprintln!("Error: {e}"),
    }
}
