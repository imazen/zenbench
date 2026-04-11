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

/// Run a benchmark suite N times and report the **best** (lowest-mean)
/// trial per benchmark.
///
/// Each "trial" is a complete suite execution: warmup, sample collection,
/// statistics. After all trials complete, every benchmark's reported
/// `summary` is the winning trial's `summary` (preserving its within-run
/// n, min, max, and median), with `mad` overridden to the inter-trial
/// spread so the reader can tell whether trials agreed or not.
///
/// See [`aggregate_trials`] for the rationale behind the min-of-trials
/// policy. Short version: on busy hosts, noise only makes measurements
/// slower, so the fastest trial is closest to the machine's real capability.
///
/// # Why trials matter beyond rounds
///
/// Rounds and iterations-per-sample reduce **within-process** variance:
/// timer noise, per-sample jitter from the occasional OS interrupt, branch
/// prediction warmup. More rounds drive that noise arbitrarily low inside
/// one `cargo bench` invocation — but they can only attack sources of
/// noise that *vary between samples in the same process*.
///
/// Trials reduce **between-process** variance: sources that are
/// constant across all samples in one invocation but differ between
/// invocations. Concretely:
///
///   * **CPU frequency state** at process startup. If the benchmark
///     happens to start with the CPU at 4.5 GHz, every sample measures
///     4.5 GHz performance. A later run that starts at 4.3 GHz measures
///     a different (but self-consistent) baseline.
///   * **Cache and TLB state** from whatever ran before.
///   * **ASLR** → different physical memory layouts → different L1/L2
///     conflict sets across runs.
///   * **Branch-predictor history** keyed by the specific addresses the
///     loader picked for the hot code this time.
///   * **Kernel scheduling** decisions (which core, which NUMA node, when
///     it yields, whether another process is sharing the core).
///   * **Background processes** that happened to be active during the run.
///
/// Every sample in a single run sees the same "this process, this
/// startup" state, so round count can't beat these sources down. Running
/// the suite as N separate engine invocations and averaging is the only
/// attack that works — and because the sources are independent across
/// invocations, the aggregate's precision scales with `sqrt(N)` just
/// like any other independent-sample statistical experiment.
///
/// In practice, a benchmark showing `mad ±0.1%` inside one run but
/// bouncing `±10%` between runs is not measuring what it claims; it's
/// measuring the within-run precision of a moving target.
///
/// Falls back to a single `run()` when `trials <= 1`.
///
/// The closure must be `FnMut` because it is invoked once per trial. In
/// practice the bench-defining closures used with `main!` capture nothing
/// or only `Copy` data, so they satisfy this bound automatically.
pub fn run_trials<F: FnMut(&mut Suite)>(trials: usize, mut f: F) -> SuiteResult {
    if trials <= 1 {
        let mut suite = Suite::new();
        f(&mut suite);
        let engine = engine::Engine::new(suite);
        return engine.run();
    }

    // Run each trial silently, then print one aggregated report at the end.
    let mut all_results: Vec<SuiteResult> = Vec::with_capacity(trials);
    for trial in 0..trials {
        eprintln!("[zenbench] trial {}/{}", trial + 1, trials);
        let mut suite = Suite::new();
        f(&mut suite);
        let engine = engine::Engine::new(suite).quiet(true);
        all_results.push(engine.run());
    }

    let aggregated = aggregate_trials(all_results);

    // Print the final aggregated report just like a single run would.
    report::print_header(
        &aggregated.run_id,
        aggregated.git_hash.as_deref(),
        aggregated.ci_environment.as_deref(),
    );
    eprintln!(
        "[zenbench] best of {trials} trials (min trial mean; mad = inter-trial spread)"
    );
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

/// Aggregate N trial-level `SuiteResult`s into a single result.
///
/// Default policy is **best-of-N**: for each `(group, bench)` we keep
/// the summary from the trial whose mean was lowest. Rationale — on a
/// shared / busy host, noise is **one-sided**. OS interrupts, context
/// switches, contention for shared caches, and thermal throttling only
/// ever make a measurement slower; nothing in the system makes it
/// faster. The fastest trial is therefore closest to the machine's
/// underlying "quiet" capability, and the slower trials represent
/// contamination, not genuine distribution spread.
///
/// This is the same reasoning `measure_loop_overhead` already uses at
/// the per-sample level (its docstring: "noise is additive"), just
/// lifted to the per-trial level.
///
/// We preserve the **winning trial's** full `Summary` (including min,
/// max, n samples, and within-run mad) so the reader can still see
/// that trial's internal precision. We override `mad` with the
/// inter-trial spread (MAD across all trial means) so the bench report
/// answers a different, useful question: "how much did the trials
/// disagree with each other?" A tiny inter-trial MAD says the best
/// result is stable; a large one says some trials were badly
/// contaminated and you should look at the raw per-trial output.
///
/// `mean_ci` is invalidated because the single-trial bootstrap no
/// longer describes the distribution the reported `mean` came from.
///
/// If you want the traditional "mean of trial means" behaviour, call
/// [`aggregate_trials_mean`] explicitly. Use it when the bench host is
/// quiet and you care about expected performance rather than best-case.
fn aggregate_trials(trials: Vec<SuiteResult>) -> SuiteResult {
    aggregate_trials_inner(trials, TrialAggregation::Min)
}

/// Mean-of-trial-means aggregation (the non-default policy). See
/// [`aggregate_trials`] for why "min" is the default.
#[allow(dead_code)]
fn aggregate_trials_mean(trials: Vec<SuiteResult>) -> SuiteResult {
    aggregate_trials_inner(trials, TrialAggregation::Mean)
}

enum TrialAggregation {
    /// Keep the winning trial's summary. Default.
    Min,
    /// Replace the summary with a fresh one built from the distribution
    /// of trial means (mean = mean of trial means, etc).
    Mean,
}

fn aggregate_trials_inner(trials: Vec<SuiteResult>, policy: TrialAggregation) -> SuiteResult {
    use std::collections::HashMap;

    if trials.is_empty() {
        return SuiteResult::default();
    }
    if trials.len() == 1 {
        return trials.into_iter().next().unwrap();
    }

    // Collect per-(group, bench) the full list of trial means. Used by
    // both policies: Min needs to pick the winner and compute inter-trial
    // spread; Mean rebuilds the summary from the full distribution.
    let mut means: HashMap<(String, String), Vec<f64>> = HashMap::new();
    for result in &trials {
        for cmp in &result.comparisons {
            for bench in &cmp.benchmarks {
                let key = (cmp.group_name.clone(), bench.name.clone());
                means.entry(key).or_default().push(bench.summary.mean);
            }
        }
    }

    // For Min policy: for each (group, bench), find which trial had the
    // lowest mean. We'll copy that trial's full summary into the output.
    // Using `&trials` by reference so we can still consume later.
    let best_trial_by_key: HashMap<(String, String), usize> = trials
        .iter()
        .enumerate()
        .fold(HashMap::new(), |mut acc, (ti, result)| {
            for cmp in &result.comparisons {
                for bench in &cmp.benchmarks {
                    let key = (cmp.group_name.clone(), bench.name.clone());
                    let this_mean = bench.summary.mean;
                    acc.entry(key)
                        .and_modify(|best_ti: &mut usize| {
                            let prev_mean = trial_mean(&trials, *best_ti, &cmp.group_name, &bench.name);
                            if this_mean < prev_mean {
                                *best_ti = ti;
                            }
                        })
                        .or_insert(ti);
                }
            }
            acc
        });

    // Template = first trial. Overwrite each bench's summary according
    // to the chosen policy.
    let mut out = trials[0].clone();
    for cmp in out.comparisons.iter_mut() {
        for bench in cmp.benchmarks.iter_mut() {
            let key = (cmp.group_name.clone(), bench.name.clone());
            let samples = match means.get(&key) {
                Some(s) => s,
                None => continue,
            };
            match policy {
                TrialAggregation::Min => {
                    if let Some(&best_ti) = best_trial_by_key.get(&key) {
                        if let Some(best_bench) = find_bench(&trials[best_ti], &key.0, &key.1) {
                            bench.summary = best_bench.summary.clone();
                        }
                    }
                    // Override mad with inter-trial spread — the within-run
                    // mad isn't what the reader of a multi-trial report is
                    // looking for; they want "how much did the trials vary?"
                    let spread = crate::stats::Summary::from_slice(samples);
                    bench.summary.mad = spread.mad;
                }
                TrialAggregation::Mean => {
                    bench.summary = crate::stats::Summary::from_slice(samples);
                }
            }
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

fn trial_mean(trials: &[SuiteResult], trial_idx: usize, group: &str, bench_name: &str) -> f64 {
    find_bench(&trials[trial_idx], group, bench_name)
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
#[macro_export]
macro_rules! main {
    // Form 1: function list — composable, like criterion
    ($($func:path),+ $(,)?) => {
        fn main() {
            let group_filter: Option<String> = std::env::args()
                .find_map(|a| a.strip_prefix("--group=").map(String::from));
            let trials: usize = std::env::args()
                .find_map(|a| a.strip_prefix("--trials=").and_then(|v| v.parse().ok()))
                .unwrap_or(1);

            let result = $crate::run_trials(trials, |suite: &mut $crate::Suite| {
                if let Some(ref filter) = group_filter {
                    suite.set_group_filter(filter.clone());
                }
                $( $func(suite); )+
            });

            $crate::postprocess_result(&result);
        }
    };
    // Form 2: closure — quick single-file benchmarks
    (|$suite:ident| $body:block) => {
        fn main() {
            let group_filter: Option<String> = std::env::args()
                .find_map(|a| a.strip_prefix("--group=").map(String::from));
            let trials: usize = std::env::args()
                .find_map(|a| a.strip_prefix("--trials=").and_then(|v| v.parse().ok()))
                .unwrap_or(1);

            let result = $crate::run_trials(trials, |$suite: &mut $crate::Suite| {
                if let Some(ref filter) = group_filter {
                    $suite.set_group_filter(filter.clone());
                }
                $body
            });

            $crate::postprocess_result(&result);
        }
    };
}
