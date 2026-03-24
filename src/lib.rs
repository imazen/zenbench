#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

//! # Zenbench
//!
//! Interleaved microbenchmarking with resource gating, paired statistics,
//! and fire-and-forget subprocess mode.

mod bench;
mod checks;
mod ci;
pub mod daemon;
mod engine;
mod format;
mod gate;
pub mod mcp;
pub mod platform;
mod report;
mod results;
mod stats;

pub use bench::{BenchFn, BenchGroup, Bencher, Benchmark, GroupConfig, Suite, Throughput};
pub use ci::CiEnvironment;
pub use engine::Engine;
pub use gate::{GateConfig, ResourceGate};
pub use results::{BenchmarkResult, ComparisonResult, RunId, SuiteResult, format_ns};
pub use stats::{PairedAnalysis, Summary};

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
    let engine = Engine::new(suite);
    engine.run()
}

/// Run a benchmark suite with custom gate configuration.
pub fn run_gated<F: FnOnce(&mut Suite)>(gate: GateConfig, f: F) -> SuiteResult {
    let mut suite = Suite::new();
    f(&mut suite);
    let engine = Engine::with_gate(suite, gate);
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
            // Parse --format=X from args (cargo bench -- --format=llm)
            // or fall back to ZENBENCH_FORMAT env var.
            let format = std::env::args()
                .find_map(|a| a.strip_prefix("--format=").map(String::from))
                .or_else(|| std::env::var("ZENBENCH_FORMAT").ok());

            let result = $crate::run(|$suite: &mut $crate::Suite| $body);

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

            // Save results if in fire-and-forget mode
            if let Some(path) = $crate::daemon::result_path_from_env() {
                if let Err(e) = result.save(&path) {
                    eprintln!("[zenbench] error saving results: {e}");
                }
            }
        }
    };
}
