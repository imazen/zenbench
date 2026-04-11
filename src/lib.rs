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

// `ProcessAggregation`, `run_processes`, and `aggregate_processes` are
// defined below in this file and re-exported via the module root.

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

/// How to combine results when running a benchmark suite across
/// multiple separate processes.
///
/// There is intentionally no default — callers must pick one. The
/// correct choice depends on what you're trying to measure, and every
/// policy answers a different question (see each variant's doc).
#[derive(Clone, Copy, Debug)]
pub enum ProcessAggregation {
    /// Keep the process with the lowest mean per benchmark. The
    /// reported `summary` is that process's full within-run summary —
    /// `mean`, `median`, `mad`, `min`, `max`, `n`, all preserved.
    ///
    /// Use on shared/busy hosts, dev machines, and CI runners: noise
    /// from co-tenants, OS interrupts, and thermal throttling only
    /// makes a process slower, so the fastest process is the closest
    /// honest estimate of the hardware's quiet capability.
    ///
    /// Caveat: best-of-N is a **biased** estimator and does not
    /// converge as N grows — the expected min drifts downward. Don't
    /// interpret "best of 3" as the same underlying quantity that
    /// "best of 100" would give you. It's useful for comparing
    /// versions of the same code on the same host with the same N,
    /// not as a statistical measurement of true mean.
    ///
    /// Inter-process spread is reported as a separate footer line so
    /// you can still tell when processes disagreed badly.
    Best,
    /// Replace each bench `summary` with a fresh `Summary` built from
    /// the distribution of per-process means. The resulting `mean` is
    /// the mean of process means, `mad` is the inter-process spread.
    ///
    /// Use on quiet hosts when you want expected-case performance.
    /// Converges via CLT and is unbiased. Pays for every contaminated
    /// process by dragging the average up — that's the point.
    Mean,
    /// Median of process means. Robust to one or two contaminated
    /// processes without the downward bias of Best. A reasonable
    /// middle ground when you can't characterize the host's noise
    /// and don't want to argue about which policy is correct.
    Median,
}

/// Run a benchmark suite in N separate engine invocations ("processes")
/// and combine the per-process results under the chosen [`ProcessAggregation`]
/// policy.
///
/// Each "process" is a complete suite execution: warmup, calibration,
/// sample collection, within-process statistics. The processes run
/// sequentially inside the same OS process (no fork/exec), but each
/// gets a fresh `Engine` / `Suite` pair, so between-process noise
/// sources that vary run-to-run are partially reset:
///
///   * Cache/TLB state from the previous process's workload — mostly
///     flushed because the previous process's working set gets
///     displaced by the new warmup.
///   * Branch predictor state — partly reset by the warmup phase,
///     though not as thoroughly as a fresh OS process.
///   * Round/iteration count that zenbench picks via calibration —
///     re-estimated per process from scratch.
///
/// Run-to-run sources that we *can't* reset this way — CPU frequency
/// state at OS-process startup, ASLR layout, kernel scheduling
/// affinity — require actually launching separate OS processes.
/// `run_processes` does NOT do that. For those, use a shell loop or
/// external `just bench-all` wrapper that invokes `cargo bench` N
/// times and feeds the results into `aggregate_processes`.
///
/// # Why a policy is required
///
/// Rounds reduce **within-process** variance. Processes reduce
/// **between-process** variance. The two attack different noise
/// sources, and how you should combine them depends on what question
/// you're trying to answer:
///
///   * "What's the lowest this code can achieve?" → [`Best`]
///   * "What will my users see on average?" → [`Mean`]
///   * "I don't know and don't want to bias the estimate" → [`Median`]
///
/// Every option answers a different question, and zenbench refuses
/// to pick for you. No default.
///
/// # CLI flags
///
/// The `main!` macro parses these from `cargo bench`:
///
/// ```text
/// --best-of-processes=N       -> run_processes(N, Best, ...)
/// --mean-of-processes=N       -> run_processes(N, Mean, ...)
/// --median-of-processes=N     -> run_processes(N, Median, ...)
/// ```
///
/// No flag → single `run()` call, exactly like before.
///
/// [`Best`]: ProcessAggregation::Best
/// [`Mean`]: ProcessAggregation::Mean
/// [`Median`]: ProcessAggregation::Median
pub fn run_processes<F: FnMut(&mut Suite)>(
    processes: usize,
    policy: ProcessAggregation,
    mut f: F,
) -> SuiteResult {
    if processes <= 1 {
        let mut suite = Suite::new();
        f(&mut suite);
        let engine = engine::Engine::new(suite);
        return engine.run();
    }

    // Run each process silently, then print one aggregated report at the end.
    let mut all_results: Vec<SuiteResult> = Vec::with_capacity(processes);
    for i in 0..processes {
        eprintln!("[zenbench] process {}/{}", i + 1, processes);
        let mut suite = Suite::new();
        f(&mut suite);
        let engine = engine::Engine::new(suite).quiet(true);
        all_results.push(engine.run());
    }

    let aggregated = aggregate_processes(all_results, policy);

    // Final aggregated report (header + groups + footer).
    report::print_header(
        &aggregated.run_id,
        aggregated.git_hash.as_deref(),
        aggregated.ci_environment.as_deref(),
    );
    let banner = match policy {
        ProcessAggregation::Best => format!(
            "[zenbench] best of {processes} processes (min process mean; within-run mad preserved)"
        ),
        ProcessAggregation::Mean => {
            format!("[zenbench] mean of {processes} processes (mad = inter-process spread)")
        }
        ProcessAggregation::Median => format!(
            "[zenbench] median of {processes} processes (mad = inter-process spread)"
        ),
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

/// Combine N `SuiteResult`s produced by separate engine invocations
/// into a single result under the caller-chosen policy.
///
/// Public so external drivers (e.g. a shell wrapper that runs `cargo
/// bench` N times in separate OS processes and deserializes each
/// JSON) can reuse the same aggregation logic zenbench's in-process
/// [`run_processes`] uses.
///
/// See [`ProcessAggregation`] for the policy definitions.
pub fn aggregate_processes(
    processes: Vec<SuiteResult>,
    policy: ProcessAggregation,
) -> SuiteResult {
    use std::collections::HashMap;

    if processes.is_empty() {
        return SuiteResult::default();
    }
    if processes.len() == 1 {
        return processes.into_iter().next().unwrap();
    }

    // Collect per-(group, bench) the full list of per-process means.
    // Used by all policies: Best needs to find the winner; Mean and
    // Median rebuild the summary from this distribution; Best reports
    // inter-process spread as a footer line.
    let mut means: HashMap<(String, String), Vec<f64>> = HashMap::new();
    for result in &processes {
        for cmp in &result.comparisons {
            for bench in &cmp.benchmarks {
                let key = (cmp.group_name.clone(), bench.name.clone());
                means.entry(key).or_default().push(bench.summary.mean);
            }
        }
    }

    // For Best policy: find the process index with the lowest mean
    // per (group, bench).
    let winners: HashMap<(String, String), usize> = processes
        .iter()
        .enumerate()
        .fold(HashMap::new(), |mut acc, (pi, result)| {
            for cmp in &result.comparisons {
                for bench in &cmp.benchmarks {
                    let key = (cmp.group_name.clone(), bench.name.clone());
                    let this_mean = bench.summary.mean;
                    acc.entry(key)
                        .and_modify(|best: &mut usize| {
                            let prev_mean =
                                process_mean(&processes, *best, &cmp.group_name, &bench.name);
                            if this_mean < prev_mean {
                                *best = pi;
                            }
                        })
                        .or_insert(pi);
                }
            }
            acc
        });

    // Template = first process. Overwrite each bench's summary
    // according to the chosen policy.
    let mut out = processes[0].clone();
    for cmp in out.comparisons.iter_mut() {
        for bench in cmp.benchmarks.iter_mut() {
            let key = (cmp.group_name.clone(), bench.name.clone());
            let samples = match means.get(&key) {
                Some(s) => s,
                None => continue,
            };
            match policy {
                ProcessAggregation::Best => {
                    // Copy the winning process's full summary verbatim,
                    // including its within-run mad. Inter-process spread
                    // is reported separately in the banner/footer so
                    // this field continues to answer "how jittery was
                    // this specific run?".
                    if let Some(&best_pi) = winners.get(&key) {
                        if let Some(best_bench) =
                            find_bench(&processes[best_pi], &key.0, &key.1)
                        {
                            bench.summary = best_bench.summary.clone();
                        }
                    }
                }
                ProcessAggregation::Mean => {
                    // Rebuild summary from the distribution of per-process
                    // means. The new `mad` is inter-process spread by
                    // construction (Summary::from_slice computes it from
                    // the input slice).
                    bench.summary = crate::stats::Summary::from_slice(samples);
                }
                ProcessAggregation::Median => {
                    // Same rebuild, but replace the Welford-mean with the
                    // median so it matches the advertised policy name.
                    let fresh = crate::stats::Summary::from_slice(samples);
                    bench.summary = fresh.clone();
                    bench.summary.mean = fresh.median;
                }
            }
            // The single-process bootstrap CI no longer describes the
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

fn process_mean(results: &[SuiteResult], idx: usize, group: &str, bench_name: &str) -> f64 {
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
/// Macro for defining benchmark binaries.
///
/// Two forms:
///
/// **Function list** (composable, like criterion_group + criterion_main):
/// ```rust,ignore
/// fn bench_sort(suite: &mut zenbench::Suite) {
///     suite.compare("sort", |group| {
///         group.bench("std", |b| b.iter(|| data.sort()));
///         group.bench("unstable", |b| b.iter(|| data.sort_unstable()));
///     });
/// }
///
/// fn bench_hash(suite: &mut zenbench::Suite) {
///     suite.compare("hash", |group| { /* ... */ });
/// }
///
/// zenbench::main!(bench_sort, bench_hash);
/// ```
///
/// **Closure** (quick and simple):
/// ```rust,ignore
/// zenbench::main!(|suite| {
///     suite.compare("sort", |group| { /* ... */ });
/// });
/// ```
/// Parse the `--{best,mean,median}-of-processes=N` flags. Returns the
/// requested process count + aggregation policy, or `None` for a
/// single-run invocation. Errors and exits on conflicting flags or
/// invalid values.
///
/// Exposed so both `main!` arms can share one implementation.
#[doc(hidden)]
pub fn parse_process_args() -> Option<(usize, ProcessAggregation)> {
    let mut found: Option<(usize, ProcessAggregation, &'static str)> = None;
    for arg in std::env::args() {
        let parsed: Option<(usize, ProcessAggregation, &'static str)> = [
            ("--best-of-processes=", ProcessAggregation::Best, "--best-of-processes"),
            ("--mean-of-processes=", ProcessAggregation::Mean, "--mean-of-processes"),
            (
                "--median-of-processes=",
                ProcessAggregation::Median,
                "--median-of-processes",
            ),
        ]
        .iter()
        .find_map(|(prefix, policy, name)| {
            arg.strip_prefix(prefix).and_then(|v| v.parse().ok()).map(|n: usize| (n, *policy, *name))
        });
        if let Some((n, p, name)) = parsed {
            if let Some((_, _, prev_name)) = found {
                eprintln!(
                    "[zenbench] error: {prev_name} and {name} are mutually exclusive"
                );
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

#[macro_export]
macro_rules! main {
    // Form 1: function list — composable, like criterion
    ($($func:path),+ $(,)?) => {
        fn main() {
            let group_filter: Option<String> = std::env::args()
                .find_map(|a| a.strip_prefix("--group=").map(String::from));
            let processes = $crate::parse_process_args();

            let closure = |suite: &mut $crate::Suite| {
                if let Some(ref filter) = group_filter {
                    suite.set_group_filter(filter.clone());
                }
                $( $func(suite); )+
            };

            let result = match processes {
                Some((n, policy)) => $crate::run_processes(n, policy, closure),
                None => $crate::run(closure),
            };

            $crate::postprocess_result(&result);
        }
    };
    // Form 2: closure — quick single-file benchmarks
    (|$suite:ident| $body:block) => {
        fn main() {
            let group_filter: Option<String> = std::env::args()
                .find_map(|a| a.strip_prefix("--group=").map(String::from));
            let processes = $crate::parse_process_args();

            let closure = |$suite: &mut $crate::Suite| {
                if let Some(ref filter) = group_filter {
                    $suite.set_group_filter(filter.clone());
                }
                $body
            };

            let result = match processes {
                Some((n, policy)) => $crate::run_processes(n, policy, closure),
                None => $crate::run(closure),
            };

            $crate::postprocess_result(&result);
        }
    };
}
