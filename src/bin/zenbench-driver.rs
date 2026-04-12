//! `zenbench-driver` — cross-OS-process benchmark aggregation driver.
//!
//! Runs a `cargo bench` command N times in **separate OS processes** and
//! combines the per-process [`SuiteResult`]s via
//! [`zenbench::aggregate_results`] under a caller-chosen policy.
//!
//! # Why this exists instead of `--best-of-passes=N`
//!
//! The in-process [`zenbench::run_passes`] entry point runs N suite
//! invocations (passes) sequentially inside ONE OS process. That resets
//! round counts, cache working sets, calibration, warmup, and the heap
//! addresses of benchmark test data — but it does NOT reset
//! between-process noise sources that only vary at OS-process startup:
//!
//!   * ASLR layout (chosen by the kernel at `exec`)
//!   * CPU frequency / C-state at first scheduling
//!   * Kernel scheduler affinity / NUMA binding decisions
//!   * Page cache residency from prior workloads
//!   * Branch predictor / BTB state unrelated to the benchmark code
//!
//! `zenbench-driver` launches each trial as a fresh child process via
//! [`std::process::Command`], so each trial gets a fresh OS-level
//! environment for all of the above. The child writes its
//! [`SuiteResult`] to a unique temp JSON path supplied via the
//! `ZENBENCH_RESULT_PATH` env var; the driver reads each JSON back and
//! feeds the collected results into the same
//! [`aggregate_results`](zenbench::aggregate_results) function the
//! in-process variant uses, so both paths share one aggregation
//! implementation.
//!
//! # Usage
//!
//! ```text
//! zenbench-driver --processes=5 --policy=best -- cargo bench --bench sorting
//! zenbench-driver --processes=10 --policy=median --format=md -- cargo bench
//! ```
//!
//! The `--` separator is required; everything after it is the command
//! to run (argv\[0\] and arguments).
//!
//! See `docs/zenbench_driver.md` for design notes.

#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};

use zenbench::{Aggregation, SuiteResult, aggregate_results};

const USAGE: &str = "\
zenbench-driver — run `cargo bench` N times in separate OS processes and
aggregate the results under a caller-chosen policy.

USAGE:
    zenbench-driver --processes=N --policy=POLICY [--format=FMT] -- CMD [ARGS...]

REQUIRED:
    --processes=N       Number of separate OS processes to run (N >= 1).
    --policy=POLICY     Aggregation policy: best | mean | median.
                        See zenbench::Aggregation docs for semantics.

OPTIONAL:
    --format=FMT        Output format for the aggregated report:
                        llm (default) | csv | md | json
    -h, --help          Show this help.

SEPARATOR:
    --                  Required. Everything after `--` is the child
                        command + args (usually `cargo bench ...`).

EXAMPLES:
    zenbench-driver --processes=5 --policy=best -- cargo bench --bench sorting
    zenbench-driver --processes=10 --policy=median --format=md -- cargo bench

NOTES:
    * Each child process is spawned with ZENBENCH_RESULT_PATH pointing at
      a unique temp file. The child serializes its SuiteResult there; the
      driver reads it back and deletes the temp file.
    * Child stderr is inherited so you see per-process progress live;
      child stdout is discarded (the aggregated report is printed by the
      driver, not the children).
    * If any child fails (non-zero exit, missing results, unreadable
      JSON), the driver cleans up temp files and exits 1.
    * This differs from `cargo bench -- --best-of-passes=N`, which
      runs all N passes in a single OS process. Use the in-process
      variant for quick iteration; use this driver when between-process
      noise (ASLR, CPU frequency state, page cache, scheduler) matters.
";

#[derive(Debug)]
struct Args {
    processes: usize,
    policy: Aggregation,
    format: OutputFormat,
    cmd: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
enum OutputFormat {
    Llm,
    Csv,
    Md,
    Json,
}

impl OutputFormat {
    fn parse(s: &str) -> Result<Self, String> {
        match s {
            "llm" => Ok(Self::Llm),
            "csv" => Ok(Self::Csv),
            "md" | "markdown" => Ok(Self::Md),
            "json" => Ok(Self::Json),
            other => Err(format!(
                "unknown --format={other}: expected llm | csv | md | json"
            )),
        }
    }
}

fn parse_policy(s: &str) -> Result<Aggregation, String> {
    match s {
        "best" => Ok(Aggregation::Best),
        "mean" => Ok(Aggregation::Mean),
        "median" => Ok(Aggregation::Median),
        other => Err(format!(
            "unknown --policy={other}: expected best | mean | median"
        )),
    }
}

fn parse_args(raw: Vec<String>) -> Result<Args, String> {
    let mut processes: Option<usize> = None;
    let mut policy: Option<Aggregation> = None;
    let mut format = OutputFormat::Llm;
    let mut cmd: Vec<String> = Vec::new();
    let mut saw_separator = false;

    // Skip argv[0]
    for arg in raw.into_iter().skip(1) {
        if saw_separator {
            cmd.push(arg);
            continue;
        }
        match arg.as_str() {
            "--" => {
                saw_separator = true;
            }
            "-h" | "--help" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            s if s.starts_with("--processes=") => {
                let v = &s["--processes=".len()..];
                processes = Some(
                    v.parse::<usize>()
                        .map_err(|e| format!("--processes: {e}"))?,
                );
            }
            s if s.starts_with("--policy=") => {
                let v = &s["--policy=".len()..];
                policy = Some(parse_policy(v)?);
            }
            s if s.starts_with("--format=") => {
                let v = &s["--format=".len()..];
                format = OutputFormat::parse(v)?;
            }
            other => {
                return Err(format!(
                    "unexpected argument `{other}` before `--` separator.\n\
                     Pass the child command after `--`, e.g.:\n\
                     \tzenbench-driver --processes=3 --policy=best -- cargo bench"
                ));
            }
        }
    }

    let processes = processes.ok_or("missing required --processes=N")?;
    let policy = policy.ok_or("missing required --policy=best|mean|median")?;

    if processes == 0 {
        return Err("--processes=0 is not meaningful".into());
    }
    if !saw_separator {
        return Err("missing `--` separator before the child command".into());
    }
    if cmd.is_empty() {
        return Err("no child command given after `--`".into());
    }

    Ok(Args {
        processes,
        policy,
        format,
        cmd,
    })
}

/// Produce a unique run id for this driver invocation. Not security
/// sensitive — just needs to be unique across concurrent drivers on
/// the same host.
fn run_id() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let pid = std::process::id();
    format!("{now:x}-{pid:x}")
}

fn temp_path(run_id: &str, i: usize) -> PathBuf {
    std::env::temp_dir().join(format!("zenbench-driver-{run_id}-{i}.json"))
}

/// Best-effort cleanup of any temp file created for this run. Never
/// fails — if the file is already gone, fine.
fn cleanup(paths: &[PathBuf]) {
    for p in paths {
        let _ = std::fs::remove_file(p);
    }
}

fn run_one_process(cmd: &[String], result_path: &PathBuf) -> Result<SuiteResult, String> {
    let (program, child_args) = cmd
        .split_first()
        .ok_or_else(|| "empty child command".to_string())?;

    let status = Command::new(program)
        .args(child_args)
        // Follow the task spec: set both. postprocess_result only
        // writes to ZENBENCH_RESULT_PATH; ZENBENCH_FORMAT=json is
        // redundant (stdout is discarded anyway) but harmless.
        .env("ZENBENCH_FORMAT", "json")
        .env("ZENBENCH_RESULT_PATH", result_path)
        // Children talk progress to stderr; the driver pipes that
        // through so users see live feedback per trial.
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| format!("failed to spawn child `{program}`: {e}"))?;

    if !status.success() {
        return Err(format!(
            "child exited with {status} (program: {program}, args: {child_args:?})"
        ));
    }

    if !result_path.exists() {
        return Err(format!(
            "child exited 0 but no result written to {}",
            result_path.display()
        ));
    }

    SuiteResult::load(result_path).map_err(|e| {
        format!(
            "failed to load result JSON from {}: {e}",
            result_path.display()
        )
    })
}

fn print_report(result: &SuiteResult, format: OutputFormat) {
    match format {
        OutputFormat::Llm => print!("{}", result.to_llm()),
        OutputFormat::Csv => print!("{}", result.to_csv()),
        OutputFormat::Md => print!("{}", result.to_markdown()),
        OutputFormat::Json => match serde_json::to_string_pretty(result) {
            Ok(json) => println!("{json}"),
            Err(e) => eprintln!("[zenbench-driver] error serializing JSON: {e}"),
        },
    }
}

fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().collect();
    let args = match parse_args(raw) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("[zenbench-driver] error: {e}\n");
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };

    let run_id = run_id();
    let mut paths: Vec<PathBuf> = (0..args.processes).map(|i| temp_path(&run_id, i)).collect();
    // Make sure no stale files from a previous run with the same pid+
    // nanosecond clash (vanishingly unlikely, but costs nothing).
    cleanup(&paths);

    let mut results: Vec<SuiteResult> = Vec::with_capacity(args.processes);
    for i in 0..args.processes {
        eprintln!(
            "[zenbench-driver] process {}/{} (pid-isolated)",
            i + 1,
            args.processes
        );
        match run_one_process(&args.cmd, &paths[i]) {
            Ok(r) => results.push(r),
            Err(e) => {
                eprintln!("[zenbench-driver] process {} FAILED: {e}", i + 1);
                cleanup(&paths);
                return ExitCode::from(1);
            }
        }
    }

    // Delete temp files before printing the report, so we don't leave
    // cruft around if the aggregation step panics.
    cleanup(&paths);
    // Avoid dangling references — paths already cleaned.
    paths.clear();

    let aggregated = aggregate_results(results, args.policy);

    // Driver-level banner on stderr (so stdout formats stay clean for
    // pipes). We can't reuse `run_passes`'s internal report helpers
    // — they're private to the crate — so fall back to the public
    // SuiteResult::print_report() for terminal output, and only print
    // the aggregated stats in the requested format on stdout.
    let policy_name = match args.policy {
        Aggregation::Best => "best",
        Aggregation::Mean => "mean",
        Aggregation::Median => "median",
    };
    eprintln!(
        "[zenbench-driver] aggregated {} of {} processes (cross-OS-process isolation)",
        policy_name, args.processes
    );
    aggregated.print_report();
    print_report(&aggregated, args.format);

    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args_from(v: &[&str]) -> Vec<String> {
        std::iter::once("zenbench-driver")
            .chain(v.iter().copied())
            .map(String::from)
            .collect()
    }

    #[test]
    fn parses_minimal_valid_invocation() {
        let a = parse_args(args_from(&[
            "--processes=3",
            "--policy=best",
            "--",
            "cargo",
            "bench",
        ]))
        .unwrap();
        assert_eq!(a.processes, 3);
        assert!(matches!(a.policy, Aggregation::Best));
        assert!(matches!(a.format, OutputFormat::Llm));
        assert_eq!(a.cmd, vec!["cargo".to_string(), "bench".to_string()]);
    }

    #[test]
    fn parses_all_policies() {
        for (name, expected) in [
            ("best", Aggregation::Best),
            ("mean", Aggregation::Mean),
            ("median", Aggregation::Median),
        ] {
            let a = parse_args(args_from(&[
                "--processes=2",
                &format!("--policy={name}"),
                "--",
                "cargo",
            ]))
            .unwrap();
            assert!(
                std::mem::discriminant(&a.policy) == std::mem::discriminant(&expected),
                "policy {name} mismatch"
            );
        }
    }

    #[test]
    fn parses_all_formats() {
        for name in ["llm", "csv", "md", "markdown", "json"] {
            let a = parse_args(args_from(&[
                "--processes=1",
                "--policy=best",
                &format!("--format={name}"),
                "--",
                "cargo",
            ]))
            .unwrap();
            let _ = a.format; // just verifying parse success
        }
    }

    #[test]
    fn rejects_missing_processes() {
        assert!(parse_args(args_from(&["--policy=best", "--", "cargo"])).is_err());
    }

    #[test]
    fn rejects_missing_policy() {
        assert!(parse_args(args_from(&["--processes=2", "--", "cargo"])).is_err());
    }

    #[test]
    fn rejects_missing_separator() {
        assert!(
            parse_args(args_from(&[
                "--processes=2",
                "--policy=best",
                "cargo",
                "bench",
            ]))
            .is_err()
        );
    }

    #[test]
    fn rejects_missing_command() {
        assert!(parse_args(args_from(&["--processes=2", "--policy=best", "--"])).is_err());
    }

    #[test]
    fn rejects_zero_processes() {
        assert!(
            parse_args(args_from(&[
                "--processes=0",
                "--policy=best",
                "--",
                "cargo"
            ]))
            .is_err()
        );
    }

    #[test]
    fn rejects_unknown_policy() {
        assert!(
            parse_args(args_from(&[
                "--processes=2",
                "--policy=fastest",
                "--",
                "cargo"
            ]))
            .is_err()
        );
    }

    #[test]
    fn rejects_unknown_format() {
        assert!(
            parse_args(args_from(&[
                "--processes=2",
                "--policy=best",
                "--format=yaml",
                "--",
                "cargo"
            ]))
            .is_err()
        );
    }

    #[test]
    fn rejects_stray_arg_before_separator() {
        assert!(
            parse_args(args_from(&[
                "--processes=2",
                "stray",
                "--policy=best",
                "--",
                "cargo",
            ]))
            .is_err()
        );
    }

    #[test]
    fn collects_all_args_after_separator() {
        let a = parse_args(args_from(&[
            "--processes=2",
            "--policy=mean",
            "--",
            "cargo",
            "bench",
            "--bench",
            "sorting",
            "--",
            "--group=foo",
        ]))
        .unwrap();
        assert_eq!(
            a.cmd,
            vec!["cargo", "bench", "--bench", "sorting", "--", "--group=foo"]
        );
    }
}
