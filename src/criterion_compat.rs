//! Drop-in compatibility layer for criterion.rs benchmarks.
//!
//! Change `criterion = "0.5"` to `zenbench = "0.1"` in Cargo.toml, then:
//! ```rust,ignore
//! // Before:
//! use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId, Throughput};
//! // After:
//! use zenbench::criterion_compat::*;
//! ```
//!
//! Your `criterion_group!` / `criterion_main!` macros, `bench_function`,
//! `bench_with_input`, `BenchmarkId`, `Throughput`, and `group.finish()`
//! all work unchanged. Under the hood, you get zenbench's interleaved
//! execution, paired statistics, and resource gating for free.

use crate::bench::{Bencher as ZenBencher, Suite};

// Re-export types criterion users expect to import
pub use crate::bench::Throughput;
pub use crate::black_box;

/// Criterion-compatible benchmark ID for parameterized benchmarks.
///
/// In criterion, `BenchmarkId::new("sort", size)` creates an ID like "sort/1000".
/// Here it's just a string builder.
pub struct BenchmarkId(String);

impl BenchmarkId {
    pub fn new<S: std::fmt::Display, P: std::fmt::Display>(function_name: S, parameter: P) -> Self {
        Self(format!("{function_name}/{parameter}"))
    }

    pub fn from_parameter<P: std::fmt::Display>(parameter: P) -> Self {
        Self(format!("{parameter}"))
    }
}

impl std::fmt::Display for BenchmarkId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<BenchmarkId> for String {
    fn from(id: BenchmarkId) -> String {
        id.0
    }
}

/// Criterion-compatible top-level benchmark runner.
///
/// Maps `c.benchmark_group("name")` → `suite.compare("name", ...)` and
/// `c.bench_function("name", ...)` → `suite.bench("name", ...)`.
pub struct Criterion {
    suite: Suite,
}

impl Criterion {
    /// Create a new Criterion-compatible runner (mirrors `Criterion::default()`).
    #[allow(clippy::should_implement_trait)]
    pub fn default() -> Self {
        Self {
            suite: Suite::new(),
        }
    }

    /// Create a benchmark group (criterion-compatible name).
    pub fn benchmark_group<S: Into<String>>(&mut self, name: S) -> BenchmarkGroup<'_> {
        BenchmarkGroup {
            name: name.into(),
            suite: &mut self.suite,
            group: None,
        }
    }

    /// Benchmark a single function (criterion-compatible name).
    pub fn bench_function<S, F>(&mut self, id: S, mut f: F) -> &mut Self
    where
        S: Into<String>,
        F: FnMut(&mut Bencher) + Send + 'static,
    {
        let name = id.into();
        let bench_name = name.clone();
        self.suite.compare(name, move |group| {
            group.bench(bench_name.clone(), move |b| f(&mut Bencher(b)));
        });
        self
    }

    /// Benchmark with input (criterion-compatible name).
    pub fn bench_with_input<S, I, F>(&mut self, id: S, input: &I, mut f: F) -> &mut Self
    where
        S: Into<String>,
        I: Clone + Send + 'static,
        F: FnMut(&mut Bencher, &I) + Send + 'static,
    {
        let name = id.into();
        let bench_name = name.clone();
        let input = input.clone();
        self.suite.compare(name, move |group| {
            let input = input.clone();
            group.bench(bench_name.clone(), move |b| {
                let input = input.clone();
                f(&mut Bencher(b), &input);
            });
        });
        self
    }

    #[doc(hidden)]
    pub fn into_suite(self) -> Suite {
        self.suite
    }
}

/// Criterion-compatible benchmark group.
///
/// Collects benchmarks, then registers them as a zenbench comparison group
/// when `finish()` is called or the group is dropped.
pub struct BenchmarkGroup<'a> {
    name: String,
    suite: &'a mut Suite,
    group: Option<crate::bench::BenchGroup>,
}

impl<'a> BenchmarkGroup<'a> {
    fn ensure_group(&mut self) -> &mut crate::bench::BenchGroup {
        if self.group.is_none() {
            self.group = Some(crate::bench::BenchGroup::new_public(&self.name));
        }
        self.group.as_mut().unwrap()
    }

    /// Set throughput for the group (criterion-compatible).
    pub fn throughput(&mut self, throughput: Throughput) -> &mut Self {
        self.ensure_group().throughput(throughput);
        self
    }

    /// Benchmark a function (criterion-compatible name).
    pub fn bench_function<S, F>(&mut self, id: S, mut f: F) -> &mut Self
    where
        S: Into<String>,
        F: FnMut(&mut Bencher) + Send + 'static,
    {
        self.ensure_group().bench(id, move |b| f(&mut Bencher(b)));
        self
    }

    /// Benchmark with input (criterion-compatible name).
    pub fn bench_with_input<S, I, F>(&mut self, id: S, input: &I, mut f: F) -> &mut Self
    where
        S: Into<String>,
        I: Clone + Send + 'static,
        F: FnMut(&mut Bencher, &I) + Send + 'static,
    {
        let input = input.clone();
        self.ensure_group().bench(id, move |b| {
            let input = input.clone();
            f(&mut Bencher(b), &input);
        });
        self
    }

    /// Criterion requires `finish()`. In zenbench it's a no-op that
    /// commits the group to the suite.
    pub fn finish(mut self) {
        self.commit();
    }

    fn commit(&mut self) {
        if let Some(group) = self.group.take() {
            self.suite.push_group(group);
        }
    }
}

impl Drop for BenchmarkGroup<'_> {
    fn drop(&mut self) {
        self.commit();
    }
}

/// Criterion-compatible bencher wrapper.
///
/// Wraps zenbench's `Bencher` with criterion's method names.
pub struct Bencher<'a>(&'a mut ZenBencher);

impl<'a> Bencher<'a> {
    /// Same as criterion's `iter` — runs the function N times.
    pub fn iter<O, F: FnMut() -> O>(&mut self, f: F) {
        self.0.iter(f);
    }

    /// Maps to `with_input(setup).run(routine)`.
    /// `BatchSize` is accepted but ignored (zenbench always does per-iteration setup).
    pub fn iter_batched<I, O, S, R>(&mut self, setup: S, routine: R, _batch_size: BatchSize)
    where
        S: FnMut() -> I + 'static,
        R: FnMut(I) -> O,
    {
        self.0.with_input(setup).run(routine);
    }

    /// Maps to `with_input(setup).run(|mut input| routine(&mut input))`.
    pub fn iter_batched_ref<I, O, S, R>(&mut self, setup: S, mut routine: R, _batch_size: BatchSize)
    where
        S: FnMut() -> I + 'static,
        R: FnMut(&mut I) -> O,
    {
        self.0
            .with_input(setup)
            .run(move |mut input| routine(&mut input));
    }
}

/// Criterion's BatchSize enum. Accepted for API compatibility but ignored —
/// zenbench always does per-iteration setup.
#[derive(Debug, Clone, Copy)]
pub enum BatchSize {
    SmallInput,
    LargeInput,
    PerIteration,
    NumBatches(u64),
    NumIterations(u64),
}

/// Macro that mimics `criterion_group!`.
///
/// ```rust,ignore
/// criterion_group!(benches, bench_sort, bench_fib);
/// criterion_main!(benches);
/// ```
#[macro_export]
macro_rules! criterion_group {
    ($name:ident, $($func:path),+ $(,)?) => {
        fn $name() -> $crate::criterion_compat::Criterion {
            let mut criterion = $crate::criterion_compat::Criterion::default();
            $(
                $func(&mut criterion);
            )+
            criterion
        }
    };
}

/// Macro that mimics `criterion_main!`.
///
/// ```rust,ignore
/// criterion_main!(benches);
/// ```
#[macro_export]
macro_rules! criterion_main {
    ($($group:path),+ $(,)?) => {
        fn main() {
            let format = std::env::args()
                .find_map(|a| a.strip_prefix("--format=").map(String::from))
                .or_else(|| std::env::var("ZENBENCH_FORMAT").ok());

            // Collect all groups into one suite
            let mut suite = $crate::bench::Suite::new();

            // Parse --group filter
            let group_filter: Option<String> = std::env::args()
                .find_map(|a| a.strip_prefix("--group=").map(String::from));
            if let Some(ref filter) = group_filter {
                suite.set_group_filter(filter.clone());
            }

            $(
                let criterion = $group();
                suite.merge(criterion.into_suite());
            )+

            let engine = $crate::engine_new(suite);
            let result = engine.run();

            match format.as_deref() {
                Some("llm") => print!("{}", result.to_llm()),
                Some("csv") => print!("{}", result.to_csv()),
                Some("markdown" | "md") => print!("{}", result.to_markdown()),
                Some("json") => {
                    if let Ok(json) = serde_json::to_string_pretty(&result) {
                        println!("{json}");
                    }
                }
                _ => {}
            }

            if let Some(path) = $crate::daemon::result_path_from_env() {
                if let Err(e) = result.save(&path) {
                    eprintln!("[zenbench] error saving results: {e}");
                }
            }
        }
    };
}
