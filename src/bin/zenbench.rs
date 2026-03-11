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

        /// Output as markdown tables with bar charts.
        #[arg(long)]
        markdown: bool,

        /// Output as CSV.
        #[arg(long)]
        csv: bool,
    },

    /// Compare two result files.
    Compare {
        /// Baseline result file.
        baseline: PathBuf,

        /// Candidate result file.
        candidate: PathBuf,
    },

    /// Compare current code against a previous git version.
    ///
    /// Builds and runs the specified benchmark at both the old and current
    /// commits, then prints a comparison. Requires the benchmark to use
    /// zenbench::main!() so results can be captured via ZENBENCH_RESULT_PATH.
    SelfCompare {
        /// Benchmark target name (as in `cargo bench --bench <name>`).
        #[arg(long)]
        bench: String,

        /// Git ref to compare against (tag, branch, or commit hash).
        /// Defaults to the most recent version tag (v*).
        #[arg(long, name = "ref")]
        git_ref: Option<String>,

        /// Extra arguments to pass to `cargo bench`.
        #[arg(long)]
        cargo_args: Option<String>,
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
            markdown,
            csv,
        } => cmd_results(&project, &run_id, json, markdown, csv),
        Commands::Compare {
            baseline,
            candidate,
        } => cmd_compare(&baseline, &candidate),
        Commands::SelfCompare {
            bench,
            git_ref,
            cargo_args,
        } => cmd_self_compare(&bench, git_ref.as_deref(), cargo_args.as_deref()),
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

fn cmd_results(project: &Path, run_id: &str, json: bool, markdown: bool, csv: bool) {
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
                        } else if markdown {
                            print!("{}", result.to_markdown());
                        } else if csv {
                            print!("{}", result.to_csv());
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

    print_comparison(&baseline, &candidate);
}

fn cmd_self_compare(bench_name: &str, git_ref: Option<&str>, cargo_args: Option<&str>) {
    // Resolve the git ref to compare against
    let reference = match git_ref {
        Some(r) => r.to_string(),
        None => match find_last_version_tag() {
            Some(tag) => {
                eprintln!("[zenbench] comparing against tag: {tag}");
                tag
            }
            None => {
                eprintln!("Error: no version tags found and no --ref specified.");
                eprintln!("  Create a tag first (git tag v0.1.0) or specify --ref <commit>");
                std::process::exit(1);
            }
        },
    };

    // Verify the ref exists
    if !git_ref_exists(&reference) {
        eprintln!("Error: git ref '{reference}' does not exist.");
        std::process::exit(1);
    }

    let current_hash = zenbench::platform::git_short_hash().unwrap_or_else(|| "HEAD".to_string());
    eprintln!("[zenbench] self-compare: {reference} (baseline) vs {current_hash} (candidate)");

    // Create temp directory for results
    let tmp_dir = std::env::temp_dir().join("zenbench-self-compare");
    std::fs::create_dir_all(&tmp_dir).unwrap_or_else(|e| {
        eprintln!("Error creating temp dir: {e}");
        std::process::exit(1);
    });
    let baseline_result_path = tmp_dir.join("baseline.json");
    let candidate_result_path = tmp_dir.join("candidate.json");

    // Step 1: Create worktree for the old ref
    let worktree_path = tmp_dir.join("worktree");
    eprintln!("[zenbench] creating worktree at {reference}...");
    if worktree_path.exists() {
        // Clean up leftover worktree
        run_git(&[
            "worktree",
            "remove",
            "--force",
            worktree_path.to_str().unwrap(),
        ]);
    }
    if !run_git(&[
        "worktree",
        "add",
        "--detach",
        worktree_path.to_str().unwrap(),
        &reference,
    ]) {
        eprintln!("Error: failed to create git worktree at {reference}.");
        std::process::exit(1);
    }

    // Step 2: Build and run baseline (old version)
    eprintln!("[zenbench] building and running baseline ({reference})...");
    let baseline_ok = run_bench_in_dir(
        &worktree_path,
        bench_name,
        &baseline_result_path,
        cargo_args,
    );

    // Step 3: Build and run candidate (current version)
    eprintln!("[zenbench] building and running candidate ({current_hash})...");
    let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let candidate_ok =
        run_bench_in_dir(&current_dir, bench_name, &candidate_result_path, cargo_args);

    // Step 4: Clean up worktree
    eprintln!("[zenbench] cleaning up worktree...");
    run_git(&[
        "worktree",
        "remove",
        "--force",
        worktree_path.to_str().unwrap(),
    ]);

    // Step 5: Compare results
    if !baseline_ok {
        eprintln!("Error: baseline benchmark failed to produce results.");
        eprintln!("  Make sure the benchmark uses zenbench::main!() macro.");
        std::process::exit(1);
    }
    if !candidate_ok {
        eprintln!("Error: candidate benchmark failed to produce results.");
        std::process::exit(1);
    }

    let baseline = match zenbench::SuiteResult::load(&baseline_result_path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error loading baseline results: {e}");
            std::process::exit(1);
        }
    };
    let candidate = match zenbench::SuiteResult::load(&candidate_result_path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error loading candidate results: {e}");
            std::process::exit(1);
        }
    };

    eprintln!();
    print_comparison(&baseline, &candidate);

    // Clean up temp files
    let _ = std::fs::remove_file(&baseline_result_path);
    let _ = std::fs::remove_file(&candidate_result_path);
}

/// Find the most recent version tag (matching v* pattern).
fn find_last_version_tag() -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["tag", "--sort=-version:refname", "--list", "v*"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.lines().next().map(|s| s.trim().to_string())
}

/// Check if a git ref exists.
fn git_ref_exists(git_ref: &str) -> bool {
    std::process::Command::new("git")
        .args(["rev-parse", "--verify", git_ref])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Run a git command, returning true on success.
fn run_git(args: &[&str]) -> bool {
    std::process::Command::new("git")
        .args(args)
        .status()
        .is_ok_and(|s| s.success())
}

/// Build and run a benchmark in a specific directory.
/// Returns true if the result file was produced.
fn run_bench_in_dir(
    dir: &Path,
    bench_name: &str,
    result_path: &Path,
    cargo_args: Option<&str>,
) -> bool {
    let mut cmd = std::process::Command::new("cargo");
    cmd.current_dir(dir)
        .args(["bench", "--bench", bench_name])
        .env("ZENBENCH_RESULT_PATH", result_path);

    if let Some(args) = cargo_args {
        for arg in args.split_whitespace() {
            cmd.arg(arg);
        }
    }

    let status = cmd.status();

    match status {
        Ok(s) if s.success() => result_path.exists(),
        Ok(s) => {
            eprintln!(
                "  cargo bench exited with status {} in {}",
                s,
                dir.display()
            );
            result_path.exists() // might still have results
        }
        Err(e) => {
            eprintln!("  failed to run cargo bench in {}: {e}", dir.display());
            false
        }
    }
}

fn format_ns(ns: f64) -> String {
    zenbench::format_ns(ns)
}

/// Print a colored comparison between two suite results.
fn print_comparison(baseline: &zenbench::SuiteResult, candidate: &zenbench::SuiteResult) {
    let base_git = baseline
        .git_hash
        .as_deref()
        .map(|h| if h.len() > 8 { &h[..8] } else { h })
        .unwrap_or("?");
    let cand_git = candidate
        .git_hash
        .as_deref()
        .map(|h| if h.len() > 8 { &h[..8] } else { h })
        .unwrap_or("?");

    eprintln!("═══════════════════════════════════════════════════════════════");
    eprintln!("  zenbench comparison");
    eprintln!("  baseline:  {} (git: {})", baseline.run_id, base_git);
    eprintln!("  candidate: {} (git: {})", candidate.run_id, cand_git);
    eprintln!("═══════════════════════════════════════════════════════════════");

    // Compare comparison groups by name
    for cand_group in &candidate.comparisons {
        if let Some(base_group) = baseline
            .comparisons
            .iter()
            .find(|g| g.group_name == cand_group.group_name)
        {
            eprintln!();
            eprintln!("  group: {}", cand_group.group_name);
            eprintln!("  ───────────────────────────────────────────────────────────");

            for cand_bench in &cand_group.benchmarks {
                if let Some(base_bench) = base_group
                    .benchmarks
                    .iter()
                    .find(|b| b.name == cand_bench.name)
                {
                    print_bench_diff(
                        &cand_bench.name,
                        base_bench.summary.mean,
                        cand_bench.summary.mean,
                    );
                } else {
                    eprintln!(
                        "    {:<30}  {:>10}  (new)",
                        cand_bench.name,
                        format_ns(cand_bench.summary.mean)
                    );
                }
            }
        }
    }

    // Compare standalone benchmarks by name
    let mut has_standalone = false;
    for cand_bench in &candidate.standalones {
        if let Some(base_bench) = baseline
            .standalones
            .iter()
            .find(|b| b.name == cand_bench.name)
        {
            if !has_standalone {
                eprintln!();
                eprintln!("  standalone:");
                eprintln!("  ───────────────────────────────────────────────────────────");
                has_standalone = true;
            }
            print_bench_diff(
                &cand_bench.name,
                base_bench.summary.mean,
                cand_bench.summary.mean,
            );
        }
    }

    eprintln!();
    eprintln!("═══════════════════════════════════════════════════════════════");
    eprintln!();
}

/// Print a single benchmark comparison line with color.
fn print_bench_diff(name: &str, base_mean: f64, cand_mean: f64) {
    let pct = if base_mean.abs() > f64::EPSILON {
        (cand_mean - base_mean) / base_mean * 100.0
    } else {
        0.0
    };

    let (arrow, reset) = if pct < -1.0 {
        ("\x1b[32m", "\x1b[0m") // green = faster
    } else if pct > 1.0 {
        ("\x1b[31m", "\x1b[0m") // red = slower
    } else {
        ("", "") // no change
    };

    eprintln!(
        "    {:<30}  {:>10} → {:>10}  {}{:+.2}%{}",
        name,
        format_ns(base_mean),
        format_ns(cand_mean),
        arrow,
        pct,
        reset,
    );
}

fn cmd_clean(project: &Path, max_age_hours: u64) {
    let max_age_secs = max_age_hours * 3600;
    match daemon::cleanup_old_runs(project, max_age_secs) {
        Ok(n) => eprintln!("Cleaned up {n} old run(s)."),
        Err(e) => eprintln!("Error: {e}"),
    }
}
