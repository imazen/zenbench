//! Drop-in compatibility layer for criterion.rs benchmarks.
//!
//! Change `criterion = "0.5"` to `zenbench = "0.1"` in Cargo.toml, then:
//! ```rust,ignore
//! // Before:
//! use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId, Throughput};
//! // After:
//! use zenbench::criterion_compat::*;
//! use zenbench::{criterion_group, criterion_main};
//! ```
//!
//! The `criterion_group!` / `criterion_main!` macros are re-exported from
//! the crate root (Rust macro scoping requires this). Types like `Criterion`,
//! `BenchmarkId`, `Throughput`, `Bencher`, and `BatchSize` come from `*`.
//!
//! **Note:** Closures passed to `bench_function` must be `'static` (use `move`).
//! If multiple closures capture the same data, clone before each `move` closure.
//! This is because zenbench stores closures for interleaved execution; criterion
//! runs them immediately.

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
    // Stored config to apply to the next group created
    config_max_rounds: Option<usize>,
    config_max_time: Option<std::time::Duration>,
    config_warmup_time: Option<std::time::Duration>,
    config_noise_threshold: Option<f64>,
}

impl Criterion {
    /// Create a new Criterion-compatible runner (mirrors `Criterion::default()`).
    #[allow(clippy::should_implement_trait)]
    pub fn default() -> Self {
        Self {
            suite: Suite::new(),
            config_max_rounds: None,
            config_max_time: None,
            config_warmup_time: None,
            config_noise_threshold: None,
        }
    }

    /// Set the number of samples (criterion-compatible, maps to max_rounds).
    pub fn sample_size(&mut self, n: usize) -> &mut Self {
        self.config_max_rounds = Some(n);
        self
    }

    /// Set measurement time (criterion-compatible, maps to max_time).
    pub fn measurement_time(&mut self, dur: std::time::Duration) -> &mut Self {
        self.config_max_time = Some(dur);
        self
    }

    /// Set warm-up time (criterion-compatible).
    pub fn warm_up_time(&mut self, dur: std::time::Duration) -> &mut Self {
        self.config_warmup_time = Some(dur);
        self
    }

    /// Set significance level (criterion-compatible, accepted but advisory).
    pub fn significance_level(&mut self, _level: f64) -> &mut Self {
        self
    }

    /// Set noise threshold (criterion-compatible).
    pub fn noise_threshold(&mut self, threshold: f64) -> &mut Self {
        self.config_noise_threshold = Some(threshold);
        self
    }

    /// Create a benchmark group (criterion-compatible name).
    pub fn benchmark_group<S: Into<String>>(&mut self, name: S) -> BenchmarkGroup<'_> {
        BenchmarkGroup {
            name: name.into(),
            suite: &mut self.suite,
            group: None,
            immediate_results: Vec::new(),
            config_max_rounds: self.config_max_rounds,
            config_max_time: self.config_max_time,
            config_warmup_time: self.config_warmup_time,
            config_noise_threshold: self.config_noise_threshold,
        }
    }

    /// Benchmark a single function (criterion-compatible).
    /// Runs immediately — no `'static` required.
    pub fn bench_function<S, F>(&mut self, id: S, mut f: F) -> &mut Self
    where
        S: Into<String>,
        F: FnMut(&mut Bencher),
    {
        let mut group = self.benchmark_group(id);
        group.bench_function("_", &mut f);
        group.finish();
        self
    }

    /// Benchmark with input (criterion-compatible).
    /// Runs immediately — no `'static` required.
    pub fn bench_with_input<S, I, F>(&mut self, id: S, input: &I, mut f: F) -> &mut Self
    where
        S: Into<String>,
        I: Clone,
        F: FnMut(&mut Bencher, &I),
    {
        let mut group = self.benchmark_group(id);
        group.bench_with_input("_", input, &mut f);
        group.finish();
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
    /// Results from immediate-mode benchmarks (no 'static required).
    immediate_results: Vec<crate::results::BenchmarkResult>,
    // Config inherited from Criterion
    config_max_rounds: Option<usize>,
    config_max_time: Option<std::time::Duration>,
    config_warmup_time: Option<std::time::Duration>,
    config_noise_threshold: Option<f64>,
}

impl<'a> BenchmarkGroup<'a> {
    fn ensure_group(&mut self) -> &mut crate::bench::BenchGroup {
        if self.group.is_none() {
            let mut g = crate::bench::BenchGroup::new_public(&self.name);
            // Apply Criterion-level config
            if let Some(n) = self.config_max_rounds {
                g.config().max_rounds(n);
            }
            if let Some(d) = self.config_max_time {
                g.config().max_time(d);
            }
            if let Some(d) = self.config_warmup_time {
                g.config().warmup_time(d);
            }
            if let Some(t) = self.config_noise_threshold {
                g.config().noise_threshold(t);
            }
            self.group = Some(g);
        }
        self.group.as_mut().unwrap()
    }

    /// Set throughput for the group (criterion-compatible).
    pub fn throughput(&mut self, throughput: Throughput) -> &mut Self {
        self.ensure_group().throughput(throughput);
        self
    }

    /// Set a custom throughput unit name (zenbench extension).
    /// E.g., `group.throughput_unit("pixels")` → "Gpixels/s" in output.
    pub fn throughput_unit(&mut self, unit: impl Into<String>) -> &mut Self {
        self.ensure_group().throughput_unit(unit);
        self
    }

    /// Set the baseline benchmark by name (zenbench extension).
    /// Comparisons are shown relative to this benchmark.
    pub fn baseline(&mut self, name: impl Into<String>) -> &mut Self {
        self.ensure_group().baseline(name);
        self
    }

    /// Set sample size (criterion-compatible). Maps to max_rounds for immediate mode.
    pub fn sample_size(&mut self, n: usize) -> &mut Self {
        self.config_max_rounds = Some(n);
        self
    }

    /// Set measurement time (criterion-compatible). Stored for immediate-mode config.
    pub fn measurement_time(&mut self, dur: std::time::Duration) -> &mut Self {
        self.config_max_time = Some(dur);
        self
    }

    /// Set warmup time (criterion-compatible).
    pub fn warm_up_time(&mut self, dur: std::time::Duration) -> &mut Self {
        self.config_warmup_time = Some(dur);
        self
    }

    /// Set sampling mode (criterion-compatible). Accepted but ignored —
    /// zenbench uses its own adaptive sampling strategy.
    pub fn sampling_mode(&mut self, _mode: impl std::fmt::Debug) -> &mut Self {
        self
    }

    /// Set plot configuration (criterion-compatible). Accepted but ignored —
    /// zenbench does not generate HTML plots.
    pub fn plot_config(&mut self, _config: impl std::fmt::Debug) -> &mut Self {
        self
    }

    /// Set significance level (criterion-compatible). Accepted for compat.
    pub fn significance_level(&mut self, _level: f64) -> &mut Self {
        self
    }

    /// Set number of resamples (criterion-compatible). Maps to bootstrap_resamples.
    pub fn nresamples(&mut self, _n: usize) -> &mut Self {
        self
    }

    /// Sort benchmarks by speed in the report (zenbench extension).
    pub fn sort_by_speed(&mut self) -> &mut Self {
        self.ensure_group().config().sort_by_speed(true);
        self
    }

    /// Set a visual subgroup label for subsequent benchmarks (zenbench extension).
    pub fn subgroup(&mut self, label: impl Into<String>) -> &mut Self {
        self.ensure_group().subgroup(label);
        self
    }

    /// Benchmark a function (criterion-compatible).
    ///
    /// Runs the benchmark **immediately** — the closure doesn't need `'static`.
    /// This matches criterion's actual behavior (sequential execution, no
    /// interleaving). For interleaved execution, use the native `suite.compare()` API.
    /// Benchmark a function (criterion-compatible).
    ///
    /// Runs the benchmark **immediately** — the closure doesn't need `'static`.
    /// This matches criterion's actual behavior (sequential execution).
    /// For interleaved execution, use the native `suite.compare()` API.
    pub fn bench_function<S, F>(&mut self, id: S, mut f: F) -> &mut Self
    where
        S: Into<String>,
        F: FnMut(&mut Bencher),
    {
        let name = id.into();
        let config = self.get_config();

        // Quick calibration
        let mut bencher = crate::bench::Bencher::new(1);
        f(&mut Bencher(&mut bencher));
        let per_iter = bencher.elapsed_ns.max(1);

        // Estimate iterations from timer precision and sample target
        let timer_res = crate::platform::timer_resolution_ns();
        let precision_min = timer_res.saturating_mul(1000).max(10_000);
        let iters = ((config.sample_target_ns.max(precision_min)) / per_iter).max(1) as usize;
        let iters = iters.clamp(config.min_iterations, config.max_iterations);

        // Collect samples (sequential, immediate)
        let n_rounds = config.max_rounds.min(100);
        let mut samples = Vec::with_capacity(n_rounds);
        for _ in 0..n_rounds {
            let mut b = crate::bench::Bencher::new(iters);
            f(&mut Bencher(&mut b));
            samples.push(b.elapsed_ns as f64 / iters as f64);
        }

        let summary = crate::stats::Summary::from_slice(&samples);
        let mean_ci = crate::stats::MeanCi::from_samples(&samples, config.bootstrap_resamples);

        self.immediate_results
            .push(crate::results::BenchmarkResult {
                name,
                summary,
                mean_ci,
                ..Default::default()
            });
        self
    }

    /// Benchmark with input (criterion-compatible).
    /// Runs immediately — no `'static` required.
    pub fn bench_with_input<S, I, F>(&mut self, id: S, input: &I, mut f: F) -> &mut Self
    where
        S: Into<String>,
        I: Clone,
        F: FnMut(&mut Bencher, &I),
    {
        let input = input.clone();
        self.bench_function(id, |b| f(b, &input))
    }

    /// Get config, applying Criterion-level overrides.
    fn get_config(&self) -> crate::bench::GroupConfig {
        let mut config = crate::bench::GroupConfig::default();
        if let Some(n) = self.config_max_rounds {
            config.max_rounds = n;
        }
        if let Some(d) = self.config_max_time {
            config.max_time = d;
        }
        if let Some(d) = self.config_warmup_time {
            config.warmup_time = d;
        }
        if let Some(t) = self.config_noise_threshold {
            config.noise_threshold = t;
        }
        config
    }

    /// Criterion requires `finish()`. Commits results to the suite.
    pub fn finish(mut self) {
        self.commit();
    }

    fn commit(&mut self) {
        // For immediate-mode benchmarks, we already have results.
        // Build a ComparisonResult from them.
        if !self.immediate_results.is_empty() {
            let comp = crate::results::ComparisonResult {
                group_name: self.name.clone(),
                benchmarks: std::mem::take(&mut self.immediate_results),
                ..Default::default()
            };
            // Print group report immediately
            crate::report::print_group(&comp, crate::platform::timer_resolution_ns());
            // Store for final output
            self.suite.push_comparison(comp);
        } else if let Some(group) = self.group.take() {
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
            let mut suite = $crate::Suite::new();

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

            // Shared post-processing: format output, baseline save/compare,
            // --update-on-pass, fire-and-forget mode.
            $crate::postprocess_result(&result);
        }
    };
}
