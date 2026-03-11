#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

//! # Zenbench
//!
//! Interleaved microbenchmarking with resource gating, paired statistics,
//! and fire-and-forget subprocess mode.

mod bench;
pub mod checks;
mod ci;
pub mod daemon;
mod engine;
mod gate;
pub mod platform;
mod results;
mod stats;

pub use bench::{BenchFn, BenchGroup, Bencher, Benchmark, GroupConfig, Suite};
pub use ci::CiEnvironment;
pub use engine::Engine;
pub use gate::{GateConfig, ResourceGate};
pub use results::{BenchmarkResult, ComparisonResult, RunId, SuiteResult};
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
    pub use crate::bench::{BenchGroup, Bencher, Suite};
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
