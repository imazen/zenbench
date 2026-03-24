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

//! # Zenbench
//!
//! Interleaved microbenchmarking with resource gating, paired statistics,
//! and fire-and-forget subprocess mode.

mod bench;
pub mod baseline;
mod checks;
mod ci;
pub mod criterion_compat;
pub mod daemon;
mod engine;
mod format;
mod gate;
pub mod mcp;
pub mod platform;
mod report;
mod results;
mod stats;
#[cfg(feature = "precise-timing")]
mod timing;
#[cfg(feature = "alloc-profiling")]
mod alloc;

pub use bench::{BenchGroup, Bencher, GroupConfig, Suite, Throughput};
#[cfg(feature = "alloc-profiling")]
pub use alloc::{AllocProfiler, AllocStats};

/// Create an Engine from a Suite (used by criterion_compat macros).
#[doc(hidden)]
pub fn engine_new(suite: Suite) -> engine::Engine {
    engine::Engine::new(suite)
}
pub use format::format_ns;
pub use gate::GateConfig;
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
pub mod prelude {
    pub use crate::bench::{BenchGroup, Bencher, Suite, Throughput};
    pub use crate::black_box;
    pub use crate::gate::GateConfig;
    pub use crate::results::SuiteResult;
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
/// # Example
///
/// ```rust,ignore
/// // benches/my_bench.rs
/// zenbench::main!(|suite| {
///     suite.compare("sorting", |group| {
///         let data: Vec<i32> = (0..1000).rev().collect();
///         group.bench("std_sort", move |b| {
///             let d = data.clone();
///             b.with_input(move || d.clone())
///                 .run(|mut v| { v.sort(); v })
///         });
///         group.bench("sort_unstable", move |b| {
///             let data: Vec<i32> = (0..1000).rev().collect();
///             b.with_input(move || data.clone())
///                 .run(|mut v| { v.sort_unstable(); v })
///         });
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
#[macro_export]
macro_rules! main {
    (|$suite:ident| $body:block) => {
        fn main() {
            // Parse args (cargo bench -- --format=llm --group=sorting --baseline=main)
            let args: Vec<String> = std::env::args().collect();
            let format = args
                .iter()
                .find_map(|a| a.strip_prefix("--format=").map(String::from))
                .or_else(|| std::env::var("ZENBENCH_FORMAT").ok());
            let group_filter: Option<String> = args
                .iter()
                .find_map(|a| a.strip_prefix("--group=").map(String::from));
            let save_baseline: Option<String> = args
                .iter()
                .find_map(|a| a.strip_prefix("--save-baseline=").map(String::from));
            let baseline: Option<String> = args
                .iter()
                .find_map(|a| a.strip_prefix("--baseline=").map(String::from));
            let max_regression: f64 = args
                .iter()
                .find_map(|a| a.strip_prefix("--max-regression=").and_then(|v| v.parse().ok()))
                .unwrap_or(5.0); // default: 5% threshold

            let result = $crate::run(|$suite: &mut $crate::Suite| {
                // Set group filter before user code runs — groups are
                // skipped during execution, not filtered from output.
                if let Some(ref filter) = group_filter {
                    $suite.set_group_filter(filter.clone());
                }
                $body
            });

            // Output in requested format (to stdout)
            match format.as_deref() {
                Some("llm") => print!("{}", result.to_llm()),
                Some("csv") => print!("{}", result.to_csv()),
                Some("markdown" | "md") => print!("{}", result.to_markdown()),
                Some("json") => {
                    if let Ok(json) = serde_json::to_string_pretty(&result) {
                        println!("{json}");
                    }
                }
                _ => {} // default: terminal report already printed to stderr
            }

            // Save as named baseline
            if let Some(ref name) = save_baseline {
                match $crate::baseline::save_baseline(&result, name) {
                    Ok(path) => eprintln!("[zenbench] baseline '{}' saved to {}", name, path.display()),
                    Err(e) => {
                        eprintln!("[zenbench] error saving baseline '{}': {}", name, e);
                        std::process::exit(2);
                    }
                }
            }

            // Compare against named baseline
            if let Some(ref name) = baseline {
                match $crate::baseline::load_baseline(name) {
                    Ok(saved) => {
                        let comparison = $crate::baseline::compare_against_baseline(
                            &saved,
                            &result,
                            max_regression,
                        );
                        $crate::baseline::print_comparison_report(&comparison);

                        if comparison.regressions > 0 {
                            eprintln!(
                                "\n[zenbench] FAIL: {} regression(s) exceed {}% threshold",
                                comparison.regressions, max_regression,
                            );
                            std::process::exit(1);
                        } else {
                            eprintln!("\n[zenbench] PASS: no regressions exceed {}% threshold", max_regression);
                        }
                    }
                    Err(e) => {
                        eprintln!("[zenbench] {e}");
                        std::process::exit(2);
                    }
                }
            }

            // Save results if in fire-and-forget mode
            if let Some(path) = $crate::daemon::result_path_from_env() {
                if let Err(e) = result.save(&path) {
                    eprintln!("[zenbench] error saving results: {e}");
                }
            }
        }
    };
}
