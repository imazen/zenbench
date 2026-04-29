// Without precise-timing or alloc-profiling, no unsafe is permitted anywhere.
// With either feature, unsafe is denied (errors) but the timing/alloc modules
// can override with #[allow(unsafe_code)] for TSC reads, asm fences, and GlobalAlloc.
#![cfg_attr(
    not(any(feature = "precise-timing", feature = "alloc-profiling")),
    forbid(unsafe_code)
)]
#![cfg_attr(
    any(feature = "precise-timing", feature = "alloc-profiling"),
    deny(unsafe_code)
)]
#![doc = include_str!("../README.md")]

#[cfg(feature = "alloc-profiling")]
mod alloc;
pub mod baseline;
mod bench;
pub mod calibration;
#[cfg(feature = "charts")]
pub mod charts;
mod checks;
mod ci;
#[cfg(feature = "criterion-compat")]
pub mod criterion_compat;
pub mod daemon;
mod engine;
pub mod exclusive;
mod format;
mod gate;
mod html;
pub mod mcp;
pub mod platform;
pub mod quickchart;
mod report;
mod results;
mod stats;
#[cfg(feature = "precise-timing")]
mod timing;
#[cfg(feature = "wasm")]
pub mod wasm;

pub use bench::{BenchGroup, Bencher, GroupConfig, Suite, Throughput};

/// Post-run processing: format output, save baseline, compare against baseline.
///
/// Shared between `main!` and `criterion_main!` macros. Not intended for
/// direct use — call via the macros instead.
#[doc(hidden)]
pub fn postprocess_result(result: &SuiteResult) {
    let args: Vec<String> = std::env::args().collect();
    let format = args
        .iter()
        .find_map(|a| a.strip_prefix("--format=").map(String::from))
        .or_else(|| std::env::var("ZENBENCH_FORMAT").ok());
    let save_baseline: Option<String> = args
        .iter()
        .find_map(|a| a.strip_prefix("--save-baseline=").map(String::from));
    let baseline_name: Option<String> = args
        .iter()
        .find_map(|a| a.strip_prefix("--baseline=").map(String::from));
    let max_regression: f64 = args
        .iter()
        .find_map(|a| {
            a.strip_prefix("--max-regression=")
                .and_then(|v| v.parse().ok())
        })
        .unwrap_or(5.0);
    let update_on_pass = args.iter().any(|a| a == "--update-on-pass");

    // Output in requested format (to stdout)
    match format.as_deref() {
        Some("llm") => print!("{}", result.to_llm()),
        Some("csv") => print!("{}", result.to_csv()),
        Some("markdown" | "md") => print!("{}", result.to_markdown()),
        Some("html") => print!("{}", result.to_html()),
        Some("json") => {
            if let Ok(json) = serde_json::to_string_pretty(result) {
                println!("{json}");
            }
        }
        _ => {} // default: terminal report already printed to stderr
    }

    // Save as named baseline
    if let Some(ref name) = save_baseline {
        match baseline::save_baseline(result, name) {
            Ok(path) => eprintln!("[zenbench] baseline '{name}' saved to {}", path.display()),
            Err(e) => {
                eprintln!("[zenbench] error saving baseline '{name}': {e}");
                std::process::exit(2);
            }
        }
    }

    // Compare against named baseline
    if let Some(ref name) = baseline_name {
        match baseline::load_baseline(name) {
            Ok(saved) => {
                let comparison = baseline::compare_against_baseline(&saved, result, max_regression);
                baseline::print_comparison_report(&comparison);

                if comparison.regressions > 0 {
                    eprintln!(
                        "\n[zenbench] FAIL: {} regression(s) exceed {max_regression}% threshold",
                        comparison.regressions,
                    );
                    std::process::exit(1);
                } else {
                    eprintln!(
                        "\n[zenbench] PASS: no regressions exceed {max_regression}% threshold"
                    );
                    // --update-on-pass: overwrite baseline with current results
                    if update_on_pass {
                        match baseline::save_baseline(result, name) {
                            Ok(path) => eprintln!(
                                "[zenbench] baseline '{name}' updated (--update-on-pass) → {}",
                                path.display()
                            ),
                            Err(e) => {
                                eprintln!("[zenbench] warning: failed to update baseline: {e}");
                            }
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("[zenbench] {e}");
                std::process::exit(2);
            }
        }
    }

    // Save results if in fire-and-forget mode
    if let Some(path) = daemon::result_path_from_env() {
        if let Err(e) = result.save(&path) {
            eprintln!("[zenbench] error saving results: {e}");
        }
    }
}
#[cfg(feature = "alloc-profiling")]
pub use alloc::{AllocProfiler, AllocStats};

/// Create an Engine from a Suite (used by criterion_compat macros).
#[doc(hidden)]
pub fn engine_new(suite: Suite) -> engine::Engine {
    engine::Engine::new(suite)
}
pub use format::format_ns;
pub use gate::GateConfig;
pub use platform::Testbed;
pub use results::{BenchmarkResult, ComparisonResult, RunId, SuiteResult};
pub use stats::{MeanCi, PairedAnalysis, Summary};

// `Aggregation`, `run_passes`, and `aggregate_results` are defined below
// and re-exported via the module root.

/// Re-export `black_box` from std for convenience.
///
/// Prevents the compiler from optimizing away benchmark code.
/// Always use this on benchmark return values and inputs.
#[inline(always)]
pub fn black_box<T>(x: T) -> T {
    std::hint::black_box(x)
}

/// Prelude for convenient imports.
///
/// ```
/// use zenbench::prelude::*;
/// ```
pub mod prelude {
    pub use crate::bench::{BenchGroup, Bencher, GroupConfig, Suite, Throughput};
    pub use crate::black_box;
    pub use crate::gate::GateConfig;
    pub use crate::results::SuiteResult;
    pub use crate::stats::{MeanCi, PairedAnalysis, Summary};
}

/// Run a benchmark suite with default configuration.
///
/// # Example
/// ```no_run
/// zenbench::run(|suite| {
///     suite.compare("sorting", |group| {
///         let data: Vec<i32> = (0..1000).rev().collect();
///         group.bench("std_sort", move |b| {
///             let d = data.clone();
///             b.with_input(move || d.clone())
///                 .run(|mut v| { v.sort(); v })
///         });
///     });
/// });
/// ```
pub fn run<F: FnOnce(&mut Suite)>(f: F) -> SuiteResult {
    let mut suite = Suite::new();
    f(&mut suite);
    let engine = engine::Engine::new(suite);
    engine.run()
}

/// How to combine N `SuiteResult`s into one.
///
/// Used by both [`run_passes`] (in-process) and [`run_processes`]
/// (cross-OS-process). There is intentionally no default — callers
/// must pick one. The correct choice depends on what you're trying to
/// measure, and every policy answers a different question.
#[derive(Clone, Copy, Debug)]
pub enum Aggregation {
    /// Keep the run with the lowest mean per benchmark. The reported
    /// `summary` is that run's full within-run summary — `mean`,
    /// `median`, `mad`, `min`, `max`, `n`, all preserved.
    ///
    /// Use on shared/busy hosts, dev machines, and CI runners: noise
    /// from co-tenants, OS interrupts, and thermal throttling only
    /// makes a run slower, so the fastest run is the closest honest
    /// estimate of the hardware's quiet capability.
    ///
    /// Caveat: best-of-N is a **biased** estimator and does not
    /// converge as N grows — the expected min drifts downward. Don't
    /// interpret "best of 3" as the same underlying quantity that
    /// "best of 100" would give you. It's useful for comparing
    /// versions of the same code on the same host with the same N,
    /// not as a statistical measurement of true mean.
    ///
    /// Inter-run spread is reported as a separate footer line so you
    /// can still tell when runs disagreed badly.
    Best,
    /// Replace each bench `summary` with a fresh `Summary` built from
    /// the distribution of per-run means. The resulting `mean` is the
    /// mean of run means, `mad` is the inter-run spread.
    ///
    /// Use on quiet hosts when you want expected-case performance.
    /// Converges via CLT and is unbiased. Pays for every contaminated
    /// run by dragging the average up — that's the point.
    Mean,
    /// Median of run means. Robust to one or two contaminated runs
    /// without the downward bias of Best. A reasonable middle ground
    /// when you can't characterize the host's noise and don't want to
    /// argue about which policy is correct.
    Median,
}

/// Run a benchmark suite in N sequential "passes" inside the current
/// OS process and combine the per-pass results under the chosen
/// [`Aggregation`] policy.
///
/// A "pass" is one complete suite execution (warmup, calibration,
/// sample collection, within-pass statistics) with a fresh `Suite`
/// and `Engine`. All N passes share the **same OS process** — no
/// `fork` / `exec` — which limits what they can reset.
///
/// # What `run_passes` resets between passes
///
/// These are genuinely re-done for each pass, so passes are useful
/// against noise sources that depend on them:
///
///   * **Calibration** (iterations-per-sample estimation) — redone
///     from scratch.
///   * **Warmup** — re-run for the hot loop.
///   * **Heap addresses of benchmark test data** — the bench-defining
///     closure re-allocates its inputs each pass, so the data lands
///     at a different heap address, shuffling TLB entries and L1/L2
///     set assignments for the data.
///   * **Data-dependent branch-predictor history** — because the
///     data moved, branches keyed by data address re-train.
///
/// # What `run_passes` does **not** reset
///
/// These are constant across all samples in one OS process and
/// therefore **constant across passes**. `run_passes` cannot attack
/// them — they are exactly why you might run `cargo bench` twice from
/// the shell and get different numbers:
///
///   * **CPU frequency / turbo / thermal state.**
///   * **ASLR layout for the binary's code pages** (set at `execve`).
///   * **Kernel page cache** for the binary mappings.
///   * **Kernel scheduler state** (affinity, NUMA node, cpuset).
///   * **Branch predictor tables for the hot code addresses** —
///     same code pages, same training history.
///   * **Background contention** on co-tenant cores.
///
/// For these, you need actually separate OS processes. Use
/// [`run_processes`] / `--best-of-processes=N`:
///
/// ```text
/// cargo bench -- --best-of-processes=3
/// ```
///
/// # When to use passes vs processes
///
/// * Your benchmark allocates large per-iteration test data, and you
///   suspect lucky heap alignment is skewing results → **passes**.
/// * Your benchmark's outer loop calibrates differently each run
///   (e.g. data-dependent iteration count estimation) → **passes**.
/// * Your measurements bounce between cargo bench invocations but
///   individual runs report `mad ±0.1%` → **processes**
///   (this is the between-OS-process variance signature).
/// * You want both → `--best-of-processes=3 --best-of-passes=2`.
///   Each of 3 OS processes does 2 in-process passes. Total = 6 runs.
///
/// # Why a policy is required
///
/// Rounds reduce **within-pass** variance (timer noise, single-sample
/// interrupts). Passes reduce a **subset of between-OS-process**
/// variance (the parts listed above). Different noise sources want
/// different aggregation rules, and the answer depends on what you're
/// trying to measure:
///
///   * "What's the lowest this code can achieve?" → [`Best`]
///   * "What will my users see on average?" → [`Mean`]
///   * "I don't know and don't want to bias the estimate" → [`Median`]
///
/// Every option answers a different question, and zenbench refuses to
/// pick for you. No default.
///
/// # CLI flags
///
/// The `main!` macro parses these from `cargo bench`:
///
/// ```text
/// --best-of-passes=N          -> run_passes(N, Best,   ...)
/// --mean-of-passes=N          -> run_passes(N, Mean,   ...)
/// --median-of-passes=N        -> run_passes(N, Median, ...)
/// ```
///
/// No flag → single `run()` call, exactly like before.
///
/// [`Best`]: Aggregation::Best
/// [`Mean`]: Aggregation::Mean
/// [`Median`]: Aggregation::Median
pub fn run_passes<F: FnMut(&mut Suite)>(
    passes: usize,
    policy: Aggregation,
    mut f: F,
) -> SuiteResult {
    if passes <= 1 {
        let mut suite = Suite::new();
        f(&mut suite);
        let engine = engine::Engine::new(suite);
        return engine.run();
    }

    // Run each pass silently, then print one aggregated report at the end.
    let mut all_results: Vec<SuiteResult> = Vec::with_capacity(passes);
    for i in 0..passes {
        eprintln!("[zenbench] pass {}/{}", i + 1, passes);
        let mut suite = Suite::new();
        f(&mut suite);
        let engine = engine::Engine::new(suite).quiet(true);
        all_results.push(engine.run());
    }

    let aggregated = aggregate_results(all_results, policy);

    // Final aggregated report (header + groups + footer).
    report::print_header(
        &aggregated.run_id,
        aggregated.git_hash.as_deref(),
        aggregated.ci_environment.as_deref(),
    );
    let banner = match policy {
        Aggregation::Best => {
            format!("[zenbench] best of {passes} passes (min pass mean; within-run mad preserved)")
        }
        Aggregation::Mean => {
            format!("[zenbench] mean of {passes} passes (mad = inter-pass spread)")
        }
        Aggregation::Median => {
            format!("[zenbench] median of {passes} passes (mad = inter-pass spread)")
        }
    };
    eprintln!("{banner}");
    for cmp in &aggregated.comparisons {
        report::print_group(cmp, aggregated.timer_resolution_ns);
    }
    report::print_footer(
        aggregated.total_time,
        aggregated.gate_waits,
        aggregated.gate_wait_time,
        aggregated.unreliable,
    );

    aggregated
}

/// Combine N `SuiteResult`s into one under the caller-chosen policy.
///
/// Source-agnostic: the inputs can be sequential passes produced by
/// [`run_passes`] (inside one OS process) or separate OS processes
/// collected by `run_processes`. The policy treats each `SuiteResult`
/// as one observation regardless of how it was produced.
///
/// See [`Aggregation`] for the policy definitions.
pub fn aggregate_results(runs: Vec<SuiteResult>, policy: Aggregation) -> SuiteResult {
    use std::collections::HashMap;

    if runs.is_empty() {
        return SuiteResult::default();
    }
    if runs.len() == 1 {
        return runs.into_iter().next().unwrap();
    }

    // Collect per-(group, bench) the full list of per-run means.
    // Used by all policies: Best needs to find the winner; Mean and
    // Median rebuild the summary from this distribution; Best reports
    // inter-run spread as a footer line.
    let mut means: HashMap<(String, String), Vec<f64>> = HashMap::new();
    for result in &runs {
        for cmp in &result.comparisons {
            for bench in &cmp.benchmarks {
                let key = (cmp.group_name.clone(), bench.name.clone());
                means.entry(key).or_default().push(bench.summary.mean);
            }
        }
    }

    // For Best policy: find the run index with the lowest mean
    // per (group, bench).
    let winners: HashMap<(String, String), usize> =
        runs.iter()
            .enumerate()
            .fold(HashMap::new(), |mut acc, (ri, result)| {
                for cmp in &result.comparisons {
                    for bench in &cmp.benchmarks {
                        let key = (cmp.group_name.clone(), bench.name.clone());
                        let this_mean = bench.summary.mean;
                        acc.entry(key)
                            .and_modify(|best: &mut usize| {
                                let prev_mean =
                                    run_mean(&runs, *best, &cmp.group_name, &bench.name);
                                if this_mean < prev_mean {
                                    *best = ri;
                                }
                            })
                            .or_insert(ri);
                    }
                }
                acc
            });

    // Template = first run. Overwrite each bench's summary according
    // to the chosen policy.
    let mut out = runs[0].clone();
    for cmp in out.comparisons.iter_mut() {
        for bench in cmp.benchmarks.iter_mut() {
            let key = (cmp.group_name.clone(), bench.name.clone());
            let samples = match means.get(&key) {
                Some(s) => s,
                None => continue,
            };
            match policy {
                Aggregation::Best => {
                    // Copy the winning run's full summary verbatim,
                    // including its within-run mad. Inter-run spread
                    // is reported separately in the banner/footer so
                    // this field continues to answer "how jittery was
                    // this specific run?".
                    if let Some(&best_ri) = winners.get(&key) {
                        if let Some(best_bench) = find_bench(&runs[best_ri], &key.0, &key.1) {
                            bench.summary = best_bench.summary.clone();
                        }
                    }
                }
                Aggregation::Mean => {
                    // Rebuild summary from the distribution of per-run
                    // means. The new `mad` is inter-run spread by
                    // construction (Summary::from_slice computes it from
                    // the input slice).
                    bench.summary = crate::stats::Summary::from_slice(samples);
                }
                Aggregation::Median => {
                    // Same rebuild, but replace the Welford-mean with the
                    // median so it matches the advertised policy name.
                    let fresh = crate::stats::Summary::from_slice(samples);
                    bench.summary = fresh.clone();
                    bench.summary.mean = fresh.median;
                }
            }
            // The single-run bootstrap CI no longer describes the
            // distribution the reported mean came from in any policy.
            bench.mean_ci = None;
        }
    }
    out
}

fn find_bench<'a>(
    result: &'a SuiteResult,
    group: &str,
    bench_name: &str,
) -> Option<&'a results::BenchmarkResult> {
    result
        .comparisons
        .iter()
        .find(|c| c.group_name == group)?
        .benchmarks
        .iter()
        .find(|b| b.name == bench_name)
}

fn run_mean(results: &[SuiteResult], idx: usize, group: &str, bench_name: &str) -> f64 {
    find_bench(&results[idx], group, bench_name)
        .map(|b| b.summary.mean)
        .unwrap_or(f64::INFINITY)
}

/// Run a benchmark suite with custom gate configuration.
pub fn run_gated<F: FnOnce(&mut Suite)>(gate: GateConfig, f: F) -> SuiteResult {
    let mut suite = Suite::new();
    f(&mut suite);
    let engine = engine::Engine::with_gate(suite, gate);
    engine.run()
}

/// Run a benchmark suite and save results to a JSON file.
///
/// If the `ZENBENCH_RESULT_PATH` env var is set (fire-and-forget mode),
/// results are saved there. Otherwise, results are saved to a timestamped
/// file in the current directory.
pub fn run_and_save<F: FnOnce(&mut Suite)>(f: F) -> SuiteResult {
    let result = run(f);

    let path = daemon::result_path_from_env().unwrap_or_else(|| {
        let name = format!("zenbench-{}.json", result.run_id);
        std::path::PathBuf::from(name)
    });

    if let Err(e) = result.save(&path) {
        eprintln!("[zenbench] error saving results to {}: {e}", path.display());
    } else {
        eprintln!("[zenbench] results saved to {}", path.display());
    }

    result
}

/// Macro for defining benchmark binaries with `cargo bench`.
///
/// Use this in a `benches/*.rs` file with `harness = false` in `Cargo.toml`.
///
/// # Examples
///
/// **Function list** (composable — recommended):
/// ```rust,ignore
/// use zenbench::prelude::*;
///
/// fn bench_sort(suite: &mut Suite) {
///     suite.group("sort", |g| {
///         g.throughput(Throughput::Elements(1000));
///         g.bench("std_sort", |b| {
///             b.with_input(|| (0..1000).rev().collect::<Vec<i32>>())
///                 .run(|mut v| { v.sort(); v })
///         });
///         g.bench("sort_unstable", |b| {
///             b.with_input(|| (0..1000).rev().collect::<Vec<i32>>())
///                 .run(|mut v| { v.sort_unstable(); v })
///         });
///     });
/// }
///
/// fn bench_fib(suite: &mut Suite) {
///     suite.bench_fn("fibonacci", || black_box(fib(20)));
/// }
///
/// zenbench::main!(bench_sort, bench_fib);
/// ```
///
/// **Closure** (quick single-file):
/// ```rust,ignore
/// zenbench::main!(|suite| {
///     suite.group("sort", |g| {
///         g.bench("std", |b| b.iter(|| data.sort()));
///         g.bench("unstable", |b| b.iter(|| data.sort_unstable()));
///     });
/// });
/// ```
///
/// In `Cargo.toml`:
/// ```toml
/// [[bench]]
/// name = "my_bench"
/// harness = false
/// ```
///
/// Parse the `--{best,mean,median}-of-passes=N` flags. Returns the
/// requested pass count + aggregation policy, or `None` for a
/// single-run invocation. Errors and exits on conflicting flags or
/// invalid values.
///
/// Exposed so both `main!` arms can share one implementation.
#[doc(hidden)]
pub fn parse_pass_args() -> Option<(usize, Aggregation)> {
    let mut found: Option<(usize, Aggregation, &'static str)> = None;
    // Prefix → policy → canonical flag name.
    let flags: &[(&str, Aggregation, &str)] = &[
        ("--best-of-passes=", Aggregation::Best, "--best-of-passes"),
        ("--mean-of-passes=", Aggregation::Mean, "--mean-of-passes"),
        (
            "--median-of-passes=",
            Aggregation::Median,
            "--median-of-passes",
        ),
    ];

    for arg in std::env::args() {
        let parsed: Option<(usize, Aggregation, &'static str)> =
            flags.iter().find_map(|(prefix, policy, name)| {
                arg.strip_prefix(prefix)
                    .and_then(|v| v.parse().ok())
                    .map(|n: usize| (n, *policy, *name))
            });
        if let Some((n, p, name)) = parsed {
            if let Some((_, _, prev_name)) = found {
                eprintln!("[zenbench] error: {prev_name} and {name} are mutually exclusive");
                std::process::exit(2);
            }
            if n == 0 {
                eprintln!("[zenbench] error: {name}=0 is not meaningful");
                std::process::exit(2);
            }
            found = Some((n, p, name));
        }
    }
    found.map(|(n, p, _)| (n, p))
}

/// Parse `--{best,mean,median}-of-processes=N` flags for cross-OS-process
/// aggregation. Returns `None` if `ZENBENCH_SUBPROCESS=1` is set (recursion
/// guard) or if no process flag is present.
#[doc(hidden)]
pub fn parse_process_args() -> Option<(usize, Aggregation)> {
    // Recursion guard: children spawned by run_processes set this.
    if std::env::var("ZENBENCH_SUBPROCESS").as_deref() == Ok("1") {
        return None;
    }
    let mut found: Option<(usize, Aggregation, &'static str)> = None;
    let flags: &[(&str, Aggregation, &str)] = &[
        (
            "--best-of-processes=",
            Aggregation::Best,
            "--best-of-processes",
        ),
        (
            "--mean-of-processes=",
            Aggregation::Mean,
            "--mean-of-processes",
        ),
        (
            "--median-of-processes=",
            Aggregation::Median,
            "--median-of-processes",
        ),
    ];

    for arg in std::env::args() {
        let parsed: Option<(usize, Aggregation, &'static str)> =
            flags.iter().find_map(|(prefix, policy, name)| {
                arg.strip_prefix(prefix)
                    .and_then(|v| v.parse().ok())
                    .map(|n: usize| (n, *policy, *name))
            });
        if let Some((n, p, name)) = parsed {
            if let Some((_, _, prev_name)) = found {
                eprintln!("[zenbench] error: {prev_name} and {name} are mutually exclusive");
                std::process::exit(2);
            }
            if n == 0 {
                eprintln!("[zenbench] error: {name}=0 is not meaningful");
                std::process::exit(2);
            }
            found = Some((n, p, name));
        }
    }
    found.map(|(n, p, _)| (n, p))
}

/// Re-exec the current benchmark binary N times in separate OS processes
/// and aggregate the results.
///
/// Each child gets a fresh ASLR layout, CPU frequency state, scheduler
/// affinity, and page cache — noise sources that in-process `run_passes`
/// cannot reset. The child writes its `SuiteResult` to a temp JSON file
/// via `ZENBENCH_RESULT_PATH`; the parent reads it back and aggregates.
///
/// # CLI flags
///
/// The `main!` macro parses these from `cargo bench`:
///
/// ```text
/// --best-of-processes=N          -> run_processes(N, Best)
/// --mean-of-processes=N          -> run_processes(N, Mean)
/// --median-of-processes=N        -> run_processes(N, Median)
/// ```
///
/// Composable with passes: `--best-of-processes=3 --best-of-passes=2`
/// runs 3 OS processes, each doing 2 in-process passes. Total = 6 runs.
pub fn run_processes(processes: usize, policy: Aggregation) -> SuiteResult {
    let exe = std::env::current_exe().unwrap_or_else(|e| {
        eprintln!("[zenbench] error: cannot determine current executable: {e}");
        std::process::exit(2);
    });

    // Build child argv: strip process flags and post-processing flags
    // (parent handles --format, --save-baseline, --baseline, --update-on-pass
    // on the aggregated result; children just measure and save JSON).
    let child_args: Vec<String> = std::env::args()
        .skip(1) // skip argv[0]
        .filter(|a| {
            !a.starts_with("--best-of-processes=")
                && !a.starts_with("--mean-of-processes=")
                && !a.starts_with("--median-of-processes=")
                && !a.starts_with("--format=")
                && !a.starts_with("--save-baseline=")
                && !a.starts_with("--baseline=")
                && !a.starts_with("--max-regression=")
                && a != "--update-on-pass"
        })
        .collect();

    // Unique run ID for temp files.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let pid = std::process::id();
    let run_id = format!("{now:x}-{pid:x}");

    // Launcher PID chain for the benchmark-process gate (issue #5).
    let launcher_pids = match std::env::var("ZENBENCH_LAUNCHER_PIDS") {
        Ok(existing) => format!("{existing},{pid}"),
        Err(_) => pid.to_string(),
    };

    let temp_dir = std::env::temp_dir();
    let temp_paths: Vec<std::path::PathBuf> = (0..processes)
        .map(|i| temp_dir.join(format!("zenbench-proc-{run_id}-{i}.json")))
        .collect();

    let mut results: Vec<SuiteResult> = Vec::with_capacity(processes);
    for (i, path) in temp_paths.iter().enumerate() {
        eprintln!("[zenbench] process {}/{processes}", i + 1);
        let status = std::process::Command::new(&exe)
            .args(&child_args)
            .env("ZENBENCH_SUBPROCESS", "1")
            .env("ZENBENCH_RESULT_PATH", path)
            .env("ZENBENCH_LAUNCHER_PIDS", &launcher_pids)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::inherit())
            .status();

        match status {
            Ok(s) if s.success() => {}
            Ok(s) => {
                eprintln!("[zenbench] process {} exited with {s}", i + 1);
                cleanup_temp(&temp_paths);
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!("[zenbench] failed to spawn process {}: {e}", i + 1);
                cleanup_temp(&temp_paths);
                std::process::exit(1);
            }
        }

        match SuiteResult::load(path) {
            Ok(r) => results.push(r),
            Err(e) => {
                eprintln!("[zenbench] process {} produced no results: {e}", i + 1);
                cleanup_temp(&temp_paths);
                std::process::exit(1);
            }
        }
    }

    cleanup_temp(&temp_paths);

    let aggregated = aggregate_results(results, policy);

    // Print the aggregated report.
    report::print_header(
        &aggregated.run_id,
        aggregated.git_hash.as_deref(),
        aggregated.ci_environment.as_deref(),
    );
    let policy_name = match policy {
        Aggregation::Best => "best",
        Aggregation::Mean => "mean",
        Aggregation::Median => "median",
    };
    eprintln!("[zenbench] {policy_name} of {processes} processes (cross-OS-process isolation)");
    for cmp in &aggregated.comparisons {
        report::print_group(cmp, aggregated.timer_resolution_ns);
    }
    report::print_footer(
        aggregated.total_time,
        aggregated.gate_waits,
        aggregated.gate_wait_time,
        aggregated.unreliable,
    );

    aggregated
}

fn cleanup_temp(paths: &[std::path::PathBuf]) {
    for p in paths {
        let _ = std::fs::remove_file(p);
    }
}

#[macro_export]
macro_rules! main {
    // Form 1: function list — composable, like criterion
    ($($func:path),+ $(,)?) => {
        fn main() {
            // Self-trampoline: re-exec in separate OS processes if requested.
            if let Some((n, policy)) = $crate::parse_process_args() {
                let result = $crate::run_processes(n, policy);
                $crate::postprocess_result(&result);
                return;
            }

            let group_filter: Option<String> = std::env::args()
                .find_map(|a| a.strip_prefix("--group=").map(String::from));
            let passes = $crate::parse_pass_args();

            let closure = |suite: &mut $crate::Suite| {
                if let Some(ref filter) = group_filter {
                    suite.set_group_filter(filter.clone());
                }
                $( $func(suite); )+
            };

            let result = match passes {
                Some((n, policy)) => $crate::run_passes(n, policy, closure),
                None => $crate::run(closure),
            };

            $crate::postprocess_result(&result);
        }
    };
    // Form 2: closure — quick single-file benchmarks
    (|$suite:ident| $body:block) => {
        fn main() {
            // Self-trampoline: re-exec in separate OS processes if requested.
            if let Some((n, policy)) = $crate::parse_process_args() {
                let result = $crate::run_processes(n, policy);
                $crate::postprocess_result(&result);
                return;
            }

            let group_filter: Option<String> = std::env::args()
                .find_map(|a| a.strip_prefix("--group=").map(String::from));
            let passes = $crate::parse_pass_args();

            let closure = |$suite: &mut $crate::Suite| {
                if let Some(ref filter) = group_filter {
                    $suite.set_group_filter(filter.clone());
                }
                $body
            };

            let result = match passes {
                Some((n, policy)) => $crate::run_passes(n, policy, closure),
                None => $crate::run(closure),
            };

            $crate::postprocess_result(&result);
        }
    };
}
