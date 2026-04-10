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
    /// Run a benchmark as a background process.
    ///
    /// Spawns `cargo bench --bench <name>` in the background and tracks it.
    /// Use --wait to block until the benchmark completes.
    Run {
        /// Benchmark target name (as in `cargo bench --bench <name>`).
        bench: String,

        /// Project root directory.
        #[arg(short, long, default_value = ".")]
        project: PathBuf,

        /// Wait for the benchmark to complete and print results.
        #[arg(long)]
        wait: bool,

        /// Timeout in seconds when using --wait (default: 600).
        #[arg(long, default_value = "600")]
        timeout: u64,

        /// Extra arguments to pass to `cargo bench` (after --).
        #[arg(long)]
        cargo_args: Option<String>,
    },

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
    ///
    /// Use "latest" to find the most recent run with results.
    /// You can also pass a path to a results JSON file directly.
    Results {
        /// Run ID, "latest", or path to a results JSON file.
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

        /// Save standalone SVG charts to a directory.
        ///
        /// Produces dual-theme SVGs (light/dark via prefers-color-scheme)
        /// with ±MAD error bars, suitable for README embedding.
        #[arg(long, value_name = "DIR")]
        save_charts: Option<PathBuf>,

        /// Save publication-quality SVG charts via charts-rs.
        ///
        /// Requires the `charts` feature. Theme: light, dark, grafana, vintage, etc.
        #[cfg(feature = "charts")]
        #[arg(long, value_name = "DIR")]
        publish_charts: Option<PathBuf>,

        /// Theme for --publish-charts (default: light).
        #[cfg(feature = "charts")]
        #[arg(long, default_value = "light")]
        chart_theme: String,

        /// Use vertical bars instead of horizontal for --publish-charts.
        #[cfg(feature = "charts")]
        #[arg(long)]
        vertical: bool,
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

    /// Manage saved baselines for CI regression testing.
    Baseline {
        #[command(subcommand)]
        action: BaselineAction,
    },
}

#[derive(Subcommand)]
enum BaselineAction {
    /// List all saved baselines.
    List,

    /// Show details of a baseline.
    Show {
        /// Baseline name.
        name: String,
    },

    /// Delete a baseline.
    Delete {
        /// Baseline name.
        name: String,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Run {
            bench,
            project,
            wait,
            timeout,
            cargo_args,
        } => cmd_run(&project, &bench, wait, timeout, cargo_args.as_deref()),
        Commands::List { project } => cmd_list(&project),
        Commands::Status { run_id, project } => cmd_status(&project, &run_id),
        Commands::Kill { target, project } => cmd_kill(&project, &target),
        Commands::Results {
            run_id,
            project,
            json,
            markdown,
            csv,
            save_charts,
            #[cfg(feature = "charts")]
            publish_charts,
            #[cfg(feature = "charts")]
            chart_theme,
            #[cfg(feature = "charts")]
            vertical,
        } => {
            #[cfg(feature = "charts")]
            let pub_config = publish_charts.as_deref().map(|d| {
                let config = zenbench::charts::ChartConfig {
                    theme: chart_theme.clone(),
                    orientation: if vertical {
                        zenbench::charts::ChartOrientation::Vertical
                    } else {
                        zenbench::charts::ChartOrientation::Horizontal
                    },
                    ..Default::default()
                };
                (d, config)
            });
            #[cfg(not(feature = "charts"))]
            let pub_config: Option<(&Path, ())> = None;
            cmd_results(
                &project,
                &run_id,
                json,
                markdown,
                csv,
                save_charts.as_deref(),
                pub_config,
            )
        }
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
        Commands::Baseline { action } => match action {
            BaselineAction::List => {
                let names = zenbench::baseline::list_baselines();
                if names.is_empty() {
                    println!("No baselines saved. Use: cargo bench -- --save-baseline=<name>");
                } else {
                    for name in &names {
                        let path = format!(".zenbench/baselines/{name}.json");
                        let meta = std::fs::metadata(&path);
                        let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
                        println!("  {name:<20} ({size} bytes)");
                    }
                }
            }
            BaselineAction::Show { name } => match zenbench::baseline::load_baseline(&name) {
                Ok(result) => {
                    println!("Baseline: {name}");
                    println!("  git: {}", result.git_hash.as_deref().unwrap_or("unknown"));
                    println!("  timestamp: {}", result.timestamp);
                    for comp in &result.comparisons {
                        println!("  group: {}", comp.group_name);
                        for bench in &comp.benchmarks {
                            println!(
                                "    {}: mean={:.1}ns min={:.1}ns",
                                bench.name, bench.summary.mean, bench.summary.min
                            );
                        }
                    }
                }
                Err(e) => {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
            },
            BaselineAction::Delete { name } => match zenbench::baseline::delete_baseline(&name) {
                Ok(()) => println!("Deleted baseline '{name}'"),
                Err(e) => {
                    eprintln!("Failed to delete baseline '{name}': {e}");
                    std::process::exit(1);
                }
            },
        },
    }
}

fn cmd_run(
    project: &Path,
    bench_name: &str,
    wait: bool,
    timeout_secs: u64,
    cargo_args: Option<&str>,
) {
    let mut args = vec!["bench", "--bench", bench_name];
    let extra_args: Vec<&str>;
    if let Some(extra) = cargo_args {
        extra_args = extra.split_whitespace().collect();
        args.extend(&extra_args);
    }

    let str_args: Vec<&str> = args.to_vec();

    match daemon::spawn_detached(project, "cargo", &str_args) {
        Ok((run_id, mut child)) => {
            eprintln!("[zenbench] run {run_id} spawned");

            if wait {
                // Wait for the child to prevent zombie processes
                let _exit_status = child.wait();
                eprintln!("[zenbench] waiting for completion (timeout: {timeout_secs}s)...");
                match daemon::wait_for_run(
                    project,
                    &run_id,
                    std::time::Duration::from_secs(2),
                    std::time::Duration::from_secs(timeout_secs),
                ) {
                    Ok(state) => match &state.status {
                        daemon::RunStatus::Completed => {
                            eprintln!("[zenbench] run {run_id} completed");
                            if let Some(result_path) = &state.result_path {
                                match zenbench::SuiteResult::load(result_path) {
                                    Ok(result) => result.print_report(),
                                    Err(e) => eprintln!("Error loading results: {e}"),
                                }
                            }
                        }
                        daemon::RunStatus::Failed(msg) => {
                            eprintln!("[zenbench] run {run_id} failed: {msg}");
                            // Print stderr log if available
                            let stderr_path =
                                daemon::runs_dir(project).join(format!("{run_id}.stderr.log"));
                            if let Ok(log) = std::fs::read_to_string(&stderr_path) {
                                let trimmed = log.trim();
                                if !trimmed.is_empty() {
                                    eprintln!("--- stderr ---");
                                    // Print last 2000 chars
                                    let start = trimmed.len().saturating_sub(2000);
                                    eprintln!("{}", &trimmed[start..]);
                                    eprintln!("--- end stderr ---");
                                }
                            }
                            std::process::exit(1);
                        }
                        daemon::RunStatus::Killed => {
                            eprintln!("[zenbench] run {run_id} was killed");
                            std::process::exit(1);
                        }
                        _ => {
                            eprintln!(
                                "[zenbench] run {run_id} in unexpected state: {:?}",
                                state.status
                            );
                            std::process::exit(1);
                        }
                    },
                    Err(e) => {
                        eprintln!("[zenbench] error: {e}");
                        std::process::exit(1);
                    }
                }
            } else {
                // Drop child without waiting — zombie will be reaped when this process exits
                drop(child);
                eprintln!("[zenbench] use `zenbench status {run_id}` to check progress");
                eprintln!("[zenbench] use `zenbench results {run_id}` when complete");
            }
        }
        Err(e) => {
            eprintln!("[zenbench] error spawning benchmark: {e}");
            std::process::exit(1);
        }
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
                "{:<24} {:<12} {:<8} {:<10} COMMAND",
                "ID", "STATUS", "PID", "GIT"
            );
            for run in runs {
                let status = match &run.status {
                    daemon::RunStatus::Queued => "queued".to_string(),
                    daemon::RunStatus::Running => {
                        if daemon::is_process_alive(run.pid) {
                            "running".to_string()
                        } else {
                            "running?".to_string() // shouldn't happen with reconciliation
                        }
                    }
                    daemon::RunStatus::Completed => "completed".to_string(),
                    daemon::RunStatus::Failed(_) => "failed".to_string(),
                    daemon::RunStatus::Killed => "killed".to_string(),
                };
                let git = run.git_hash.as_deref().unwrap_or("-");
                let git_short = if git.len() > 8 { &git[..8] } else { git };
                println!(
                    "{:<24} {:<12} {:<8} {:<10} {}",
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
                let exists = path.exists();
                println!(
                    "Results: {} ({})",
                    path.display(),
                    if exists { "exists" } else { "pending" }
                );
            }
            // Show elapsed time
            let now = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let elapsed = now.saturating_sub(state.started_at);
            if let Some(finished) = state.finished_at {
                let duration = finished.saturating_sub(state.started_at);
                println!("Duration: {duration}s");
            } else {
                println!("Elapsed: {elapsed}s");
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

fn cmd_results(
    project: &Path,
    run_id: &str,
    json: bool,
    markdown: bool,
    csv: bool,
    save_charts: Option<&Path>,
    #[cfg(feature = "charts")] publish_config: Option<(&Path, zenbench::charts::ChartConfig)>,
    #[cfg(not(feature = "charts"))] publish_config: Option<(&Path, ())>,
) {
    let save_all_charts = |result: &zenbench::SuiteResult| {
        if let Some(dir) = save_charts {
            if let Err(e) = result.save_charts(dir) {
                eprintln!("Error saving charts: {e}");
            } else {
                eprintln!("Charts saved to {}", dir.display());
            }
        }
        #[cfg(feature = "charts")]
        if let Some((dir, ref config)) = publish_config {
            if let Err(e) = result.save_publication_charts(dir, config) {
                eprintln!("Error saving publication charts: {e}");
            } else {
                eprintln!(
                    "Publication charts ({}, {:?}) saved to {}",
                    config.theme,
                    config.orientation,
                    dir.display()
                );
            }
        }
        #[cfg(not(feature = "charts"))]
        let _ = publish_config;
    };

    // Check if it's a direct file path first
    let file_path = PathBuf::from(run_id);
    if file_path.exists() && file_path.extension().is_some_and(|e| e == "json") {
        match zenbench::SuiteResult::load(&file_path) {
            Ok(result) => {
                output_result(&result, json, markdown, csv);
                save_all_charts(&result);
                return;
            }
            Err(e) => {
                eprintln!("Error loading results from {}: {e}", file_path.display());
                return;
            }
        }
    }

    // Resolve "latest" to the most recent completed run
    let actual_id = if run_id == "latest" {
        match daemon::find_latest_with_results(project) {
            Ok(Some(state)) => state.id,
            Ok(None) => {
                eprintln!("No completed runs with results found.");
                return;
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
                        output_result(&result, json, markdown, csv);
                        save_all_charts(&result);
                    }
                    Err(e) => {
                        eprintln!("Error loading results from {}: {e}", result_path.display());
                        if !result_path.exists() {
                            eprintln!(
                                "Run {} status: {:?}. Results file does not exist.",
                                actual_id, state.status
                            );
                        }
                    }
                }
            } else {
                eprintln!(
                    "Run {} has no results path (status: {:?})",
                    actual_id, state.status
                );
            }
        }
        Err(e) => eprintln!("Error: {e}"),
    }
}

fn output_result(result: &zenbench::SuiteResult, json: bool, markdown: bool, csv: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(result).unwrap());
    } else if markdown {
        print!("{}", result.to_markdown());
    } else if csv {
        print!("{}", result.to_csv());
    } else {
        result.print_report();
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

    // Tell the child benchmark to exclude the launcher's PID from the
    // benchmark-process gate. Without this, the child detects the parent
    // `zenbench self-compare` process and waits for it to exit (deadlock).
    let our_pid = std::process::id();
    let launcher_pids = match std::env::var("ZENBENCH_LAUNCHER_PIDS") {
        Ok(existing) => format!("{existing},{our_pid}"),
        Err(_) => our_pid.to_string(),
    };
    cmd.env("ZENBENCH_LAUNCHER_PIDS", &launcher_pids);

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

use std::time::SystemTime;
