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

/// Run a benchmark suite N times and aggregate the per-bench means
/// across trials.
///
/// Each "trial" is a complete suite execution: warmup, sample collection,
/// statistics. After all trials complete, every benchmark's `summary` is
/// replaced with a new `Summary` built from the trial-level means
/// (`mean = median of trial means, mad = inter-trial spread, n = trials`).
///
/// Use this when single-run inter-process variance dominates the within-run
/// CV — e.g. mid-latency benchmarks on noisy hosts where one OS interrupt
/// during a 1ms sample window swings the result 5–10%. Three to five trials
/// usually pin the median to within 1–2%.
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
    eprintln!("[zenbench] aggregated across {trials} trials");
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

/// Aggregate N trial-level `SuiteResult`s into a single result whose
/// per-bench `summary` reflects the distribution of trial-level means.
///
/// The first trial is taken as the structural template (for run_id,
/// timestamp, group order, paired analyses, etc). Each benchmark inside
/// it gets its `summary` rebuilt: the inputs are the per-trial mean times
/// for that exact `(group_name, bench_name)`. `Summary::from_slice`
/// computes mean, median, MAD, variance, and min/max from those inputs,
/// so the resulting `mean` reflects all trials and the resulting `mad`
/// captures inter-trial spread.
///
/// `mean_ci` is invalidated (set to `None`) because the underlying
/// per-sample CI no longer corresponds to a single trial's distribution.
fn aggregate_trials(trials: Vec<SuiteResult>) -> SuiteResult {
    use std::collections::HashMap;

    if trials.is_empty() {
        // Caller invariant violation; return a default-constructed result.
        return SuiteResult::default();
    }
    if trials.len() == 1 {
        return trials.into_iter().next().unwrap();
    }

    // Collect per-(group, bench) mean times across all trials.
    let mut means: HashMap<(String, String), Vec<f64>> = HashMap::new();
    for result in &trials {
        for cmp in &result.comparisons {
            for bench in &cmp.benchmarks {
                let key = (cmp.group_name.clone(), bench.name.clone());
                means.entry(key).or_default().push(bench.summary.mean);
            }
        }
    }

    // Use the first trial as the structural template and rewrite its
    // benchmark summaries with the cross-trial aggregates.
    let mut out = trials.into_iter().next().unwrap();
    for cmp in out.comparisons.iter_mut() {
        for bench in cmp.benchmarks.iter_mut() {
            let key = (cmp.group_name.clone(), bench.name.clone());
            if let Some(samples) = means.get(&key) {
                bench.summary = crate::stats::Summary::from_slice(samples);
                // CI was computed from per-sample data inside one trial.
                // It no longer reflects the inter-trial distribution we
                // just substituted.
                bench.mean_ci = None;
            }
        }
    }
    out
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
