use serde::{Deserialize, Serialize};
use std::sync::{Arc, Barrier};
use std::time::Duration;

/// Throughput declaration for a benchmark group.
///
/// When set on a group, reports will show throughput (MiB/s, GiB/s, ops/s)
/// alongside raw time for every benchmark in the group.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Throughput {
    /// Input size in bytes. Reports show MiB/s or GiB/s.
    Bytes(u64),
    /// Number of elements processed. Reports show ops/s, Kops/s, etc.
    Elements(u64),
}

impl Throughput {
    /// Compute throughput from mean time in nanoseconds.
    ///
    /// Returns (value, unit_string). When `unit` is provided and this
    /// is `Elements`, the unit suffix uses the custom name
    /// (e.g., "checks" → "Gchecks/s").
    pub fn compute(&self, mean_ns: f64, unit: Option<&str>) -> (f64, String) {
        if mean_ns <= 0.0 {
            return (0.0, "?/s".to_string());
        }
        let seconds = mean_ns / 1e9;
        match self {
            Throughput::Bytes(n) => {
                let bytes_per_sec = *n as f64 / seconds;
                let gib = bytes_per_sec / (1024.0 * 1024.0 * 1024.0);
                if gib >= 1.0 {
                    (gib, "GiB/s".to_string())
                } else {
                    (bytes_per_sec / (1024.0 * 1024.0), "MiB/s".to_string())
                }
            }
            Throughput::Elements(n) => {
                let ops_per_sec = *n as f64 / seconds;
                let u = unit.unwrap_or("ops");
                if ops_per_sec >= 1e9 {
                    (ops_per_sec / 1e9, format!("G{u}/s"))
                } else if ops_per_sec >= 1e6 {
                    (ops_per_sec / 1e6, format!("M{u}/s"))
                } else if ops_per_sec >= 1e3 {
                    (ops_per_sec / 1e3, format!("K{u}/s"))
                } else {
                    (ops_per_sec, format!("{u}/s"))
                }
            }
        }
    }

    /// The element count for this throughput (if Elements).
    pub fn element_count(&self) -> Option<u64> {
        match self {
            Throughput::Elements(n) => Some(*n),
            Throughput::Bytes(_) => None,
        }
    }

    /// Format throughput as human-readable string.
    pub fn format(&self, mean_ns: f64, unit: Option<&str>) -> String {
        let (val, unit_str) = self.compute(mean_ns, unit);
        if val >= 100.0 {
            format!("{val:.0} {unit_str}")
        } else if val >= 10.0 {
            format!("{val:.1} {unit_str}")
        } else {
            format!("{val:.2} {unit_str}")
        }
    }
}

/// A complete benchmark suite containing comparison groups and standalone benchmarks.
pub struct Suite {
    pub(crate) groups: Vec<BenchGroup>,
    pub(crate) standalones: Vec<Benchmark>,
    pub(crate) group_filter: Option<String>,
}

impl Suite {
    pub fn new() -> Self {
        Self {
            groups: Vec::new(),
            standalones: Vec::new(),
            group_filter: None,
        }
    }

    /// Add a comparison group. Benchmarks within a group are interleaved
    /// to eliminate system-state bias.
    pub fn compare<F: FnOnce(&mut BenchGroup)>(&mut self, name: impl Into<String>, f: F) {
        let mut group = BenchGroup::new(name);
        f(&mut group);
        if group.benchmarks.len() < 2 {
            eprintln!(
                "[zenbench] warning: comparison group '{}' has fewer than 2 benchmarks",
                group.name
            );
        }
        self.groups.push(group);
    }

    /// Add a standalone benchmark (not compared against anything).
    pub fn bench<F>(&mut self, name: impl Into<String>, f: F)
    where
        F: FnMut(&mut Bencher) + Send + 'static,
    {
        self.standalones.push(Benchmark {
            name: name.into(),
            tags: Vec::new(),
            subgroup: None,
            func: BenchFn::new(f),
        });
    }

    /// Set a group filter — only groups whose name matches or contains
    /// the filter string will be executed. Set via `--group=NAME`.
    pub fn set_group_filter(&mut self, filter: String) {
        self.group_filter = Some(filter);
    }

    /// Merge another suite's groups and standalones into this one.
    pub fn merge(&mut self, other: Suite) {
        self.groups.extend(other.groups);
        self.standalones.extend(other.standalones);
    }

    /// Push a pre-built group (used by criterion_compat).
    pub fn push_group(&mut self, group: BenchGroup) {
        self.groups.push(group);
    }
}

impl Default for Suite {
    fn default() -> Self {
        Self::new()
    }
}

/// A group of benchmarks to compare via interleaved execution.
pub struct BenchGroup {
    pub(crate) name: String,
    pub(crate) benchmarks: Vec<Benchmark>,
    pub(crate) config: GroupConfig,
    pub(crate) throughput: Option<Throughput>,
    pub(crate) throughput_unit: Option<String>,
    pub(crate) baseline_name: Option<String>,
    /// Current subgroup label, applied to subsequent benchmarks.
    current_subgroup: Option<String>,
}

impl BenchGroup {
    /// Create a new group (public, for criterion_compat).
    pub fn new_public(name: impl Into<String>) -> Self {
        Self::new(name)
    }

    pub(crate) fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            benchmarks: Vec::new(),
            config: GroupConfig::default(),
            throughput: None,
            throughput_unit: None,
            baseline_name: None,
            current_subgroup: None,
        }
    }

    /// Add a benchmark to this comparison group.
    pub fn bench<F>(&mut self, name: impl Into<String>, f: F)
    where
        F: FnMut(&mut Bencher) + Send + 'static,
    {
        self.benchmarks.push(Benchmark {
            name: name.into(),
            tags: Vec::new(),
            subgroup: self.current_subgroup.clone(),
            func: BenchFn::new(f),
        });
    }

    /// Add a benchmark with key-value tags for multi-dimensional reporting.
    ///
    /// Tags enable grouping and pivoting in reports. Common tags:
    /// `("library", "zenflate")`, `("level", "L6")`, `("data", "mixed")`.
    pub fn bench_tagged<F>(&mut self, name: impl Into<String>, tags: &[(&str, &str)], f: F)
    where
        F: FnMut(&mut Bencher) + Send + 'static,
    {
        self.benchmarks.push(Benchmark {
            name: name.into(),
            tags: tags
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
            subgroup: self.current_subgroup.clone(),
            func: BenchFn::new(f),
        });
    }

    /// Add a multithreaded contention benchmark.
    ///
    /// Spawns `threads` threads that all start simultaneously (barrier-synchronized)
    /// and run the benchmark closure in parallel. Measures wall-clock time from
    /// barrier release to all threads completing — this is the throughput under
    /// contention that your users experience.
    ///
    /// The `setup` closure runs once to create shared state (typically an `Arc`).
    /// The `work` closure runs on each thread with a reference to the shared state
    /// and the thread index (0..threads).
    ///
    /// ```rust,ignore
    /// group.bench_contended("mutex_map", 8,
    ///     || Arc::new(Mutex::new(HashMap::new())),
    ///     |b, shared, thread_id| {
    ///         b.iter(|| { shared.lock().unwrap().insert(thread_id, 42); })
    ///     },
    /// );
    /// ```
    pub fn bench_contended<S, Setup, Work>(
        &mut self,
        name: impl Into<String>,
        threads: usize,
        setup: Setup,
        work: Work,
    ) where
        S: Send + Sync + 'static,
        Setup: Fn() -> S + Send + 'static,
        Work: Fn(&mut Bencher, &S, usize) + Send + Sync + Clone + 'static,
    {
        let name = name.into();
        let threads = threads.max(1);

        self.benchmarks.push(Benchmark {
            name,
            tags: vec![("threads".to_string(), threads.to_string())],
            subgroup: self.current_subgroup.clone(),
            func: BenchFn::new(move |bencher: &mut Bencher| {
                let shared = setup();
                let shared = Arc::new(shared);
                let iterations = bencher.iterations;
                let barrier = Arc::new(Barrier::new(threads + 1)); // +1 for the timing thread

                let mut handles = Vec::with_capacity(threads);
                for tid in 0..threads {
                    let shared = shared.clone();
                    let barrier = barrier.clone();
                    let work = work.clone();
                    handles.push(std::thread::spawn(move || {
                        // Each thread gets its own bencher for iteration counting
                        let mut thread_bencher = Bencher::new(iterations);
                        barrier.wait(); // synchronized start
                        work(&mut thread_bencher, &shared, tid);
                        barrier.wait(); // synchronized end
                    }));
                }

                // Timing thread: wait for all threads to start, then time until done
                barrier.wait(); // all threads released
                let start = std::time::Instant::now();
                barrier.wait(); // all threads finished
                bencher.elapsed_ns = start.elapsed().as_nanos() as u64;

                for h in handles {
                    h.join().expect("benchmark thread panicked");
                }
            }),
        });
    }

    /// Add a parallel throughput benchmark (no shared state).
    ///
    /// Spawns `threads` threads that each run the same work independently.
    /// Measures total wall-clock time. Use this to find scaling limits —
    /// if 4 threads aren't 4x faster, you're hitting cache/memory bandwidth
    /// or SMT contention.
    ///
    /// Each thread gets its own thread index (0..threads) but no shared state.
    /// For shared-state contention testing, use [`BenchGroup::bench_contended`] instead.
    ///
    /// ```rust,ignore
    /// // Compare 1, 2, 4 threads doing independent work
    /// for threads in [1, 2, 4] {
    ///     group.bench_parallel(format!("{threads}t"), threads, |b, _tid| {
    ///         b.iter(|| expensive_computation())
    ///     });
    /// }
    /// ```
    ///
    /// **Rayon / existing thread pools**: Don't use this for code that manages
    /// its own threads (rayon, tokio, etc.). Just use regular `bench()` —
    /// wall-clock timing already captures all threads' work. `bench_parallel`
    /// spawns its own threads, which would compete with rayon's pool.
    pub fn bench_parallel<F>(&mut self, name: impl Into<String>, threads: usize, work: F)
    where
        F: Fn(&mut Bencher, usize) + Send + Sync + Clone + 'static,
    {
        let name = name.into();
        let threads = threads.max(1);

        self.benchmarks.push(Benchmark {
            name,
            tags: vec![("threads".to_string(), threads.to_string())],
            subgroup: self.current_subgroup.clone(),
            func: BenchFn::new(move |bencher: &mut Bencher| {
                let iterations = bencher.iterations;
                let barrier = Arc::new(Barrier::new(threads + 1));

                let mut handles = Vec::with_capacity(threads);
                for tid in 0..threads {
                    let barrier = barrier.clone();
                    let work = work.clone();
                    handles.push(std::thread::spawn(move || {
                        let mut thread_bencher = Bencher::new(iterations);
                        barrier.wait();
                        work(&mut thread_bencher, tid);
                        barrier.wait();
                    }));
                }

                barrier.wait();
                let start = std::time::Instant::now();
                barrier.wait();
                bencher.elapsed_ns = start.elapsed().as_nanos() as u64;

                for h in handles {
                    h.join().expect("benchmark thread panicked");
                }
            }),
        });
    }

    /// Automatic thread scaling analysis.
    ///
    /// Probes thread counts from 1 up to the system's logical core count
    /// (powers of 2 plus the physical core count). Each thread count becomes
    /// a separate benchmark in the group, interleaved and compared.
    ///
    /// Use with `Throughput::Elements(N)` to see scaling and efficiency:
    /// ```rust,ignore
    /// group.throughput(Throughput::Elements(10_000));
    /// group.bench_scaling("sqrt_work", |b, _tid| {
    ///     b.iter(|| expensive_computation())
    /// });
    /// ```
    ///
    /// The 1-thread benchmark is the baseline. The report shows how
    /// throughput scales (or doesn't) with more threads.
    pub fn bench_scaling<F>(&mut self, name: impl Into<String>, work: F)
    where
        F: Fn(&mut Bencher, usize) + Send + Sync + Clone + 'static,
    {
        let name = name.into();
        let sys = sysinfo::System::new_all();
        let logical_cores = sys.cpus().len().max(1);
        let physical_cores = sysinfo::System::physical_core_count().unwrap_or(logical_cores);

        // Every integer from 1 to physical_cores, then the SMT point.
        // Auto-rounds convergence makes this cheap — far-from-peak counts
        // converge in 30 rounds; near-peak counts get more rounds automatically.
        // Optimal thread counts like 3 or 5 are common and can't be predicted.
        let mut counts: Vec<usize> = (1..=physical_cores).collect();
        if logical_cores > physical_cores {
            counts.push(logical_cores);
        }
        counts.sort_unstable();
        counts.dedup();
        counts.retain(|&c| c >= 1 && c <= logical_cores);

        eprintln!(
            "[zenbench] scaling '{}': probing {} thread counts on {}/{} cores (physical/logical)",
            name,
            counts.len(),
            physical_cores,
            logical_cores,
        );

        for threads in counts {
            let label = format!("{name}_{threads}t");
            self.bench_parallel(label, threads, work.clone());
        }
    }

    /// Declare the throughput for this group.
    ///
    /// All benchmarks in the group process the same amount of data,
    /// so throughput is set at the group level.
    pub fn throughput(&mut self, throughput: Throughput) -> &mut Self {
        self.throughput = Some(throughput);
        self
    }

    /// Set a visual subgroup label for subsequent benchmarks.
    ///
    /// Subgroups are display-only — benchmarks are still interleaved and
    /// compared across subgroups within the same comparison group. The label
    /// appears as a section header in the table and bar chart.
    ///
    /// ```rust,ignore
    /// group.subgroup("Ok path");
    /// group.bench("no_error", |b| { /* ... */ });
    /// group.subgroup("Error path");
    /// group.bench("with_backtrace", |b| { /* ... */ });
    /// ```
    pub fn subgroup(&mut self, label: impl Into<String>) -> &mut Self {
        self.current_subgroup = Some(label.into());
        self
    }

    /// Set a custom unit name for `Throughput::Elements`.
    ///
    /// When set, reports show e.g. "5.0 Gchecks/s" instead of "5.0 Gops/s".
    pub fn throughput_unit(&mut self, unit: impl Into<String>) -> &mut Self {
        self.throughput_unit = Some(unit.into());
        self
    }

    /// Set which benchmark is the baseline for comparisons.
    ///
    /// By default, the first benchmark added is the baseline. Use this
    /// to compare against a different benchmark by name.
    pub fn baseline(&mut self, name: impl Into<String>) -> &mut Self {
        self.baseline_name = Some(name.into());
        self
    }

    /// Configure this group's execution parameters.
    pub fn config(&mut self) -> &mut GroupConfig {
        &mut self.config
    }
}

/// Configuration for a benchmark group's execution.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct GroupConfig {
    /// Target number of measurement rounds.
    pub max_rounds: usize,
    /// Minimum number of rounds before `max_time` is checked.
    /// Guarantees at least this many measurements even on slow benchmarks.
    /// Default: 5.
    pub min_rounds: usize,
    /// Warmup time before measurement begins.
    pub warmup_time: Duration,
    /// Maximum total time for the group (measured in benchmark time, not wall time).
    pub max_time: Duration,
    /// Minimum iterations per sample.
    pub min_iterations: usize,
    /// Maximum iterations per sample (default: 10M, high enough for sub-ns operations).
    pub max_iterations: usize,
    /// Whether to spoil CPU cache between samples.
    ///
    /// When `true`, reads a large buffer between benchmarks in each round
    /// to evict hot cache lines. This prevents one benchmark's output from
    /// remaining in L1/L2 where the next benchmark picks it up for free.
    ///
    /// **Default: `false`.** Most microbenchmarks measure hot-path code where
    /// pointer-chasing (Box, Arc, vtable dispatch) stays in cache. The firewall
    /// penalizes these unfairly. Enable it when benchmarks touch different memory
    /// regions and you want cold-cache behavior.
    pub cache_firewall: bool,
    /// Cache firewall size in bytes (default: 2 MiB, enough to spoil L2).
    pub cache_firewall_bytes: usize,
    /// Only compare against the baseline (first benchmark) in reports.
    ///
    /// When `false` (default for <= 3 benchmarks), shows all pairwise comparisons.
    /// When `true` (default for > 3 benchmarks), only compares each benchmark
    /// against the first. Full pairwise data is always available in JSON output.
    ///
    /// Set explicitly to override the auto-detection.
    pub baseline_only: Option<bool>,
    /// Suppress "likely optimized away" warnings for sub-nanosecond benchmarks.
    ///
    /// Set to `true` when you know your benchmark genuinely runs in sub-ns time
    /// (e.g., a constant return or a single branch-predicted check).
    pub expect_sub_ns: bool,
    /// Sort benchmarks by speed (fastest first) in report output.
    ///
    /// Default: `false` (definition order). When `true`, the table rows
    /// are sorted by mean time ascending.
    pub sort_by_speed: bool,
    /// Stop early when results are precise enough.
    ///
    /// When `true` (default), measurement stops before `rounds` if the
    /// relative CI half-width drops below `target_precision` for all
    /// benchmarks. This saves time on clean systems and uses more rounds
    /// on noisy ones.
    pub auto_rounds: bool,
    /// Target relative precision for auto-rounds (default: 0.02 = 2%).
    ///
    /// Measurement stops when `1.96 * stddev / (sqrt(n) * mean)` drops
    /// below this threshold — i.e., the 95% CI half-width is less than
    /// this fraction of the mean.
    pub target_precision: f64,
    /// Hard wall-clock time limit for the entire group (including gate waits).
    ///
    /// Default: 120 seconds. Prevents runaway benchmarks from blocking CI
    /// or interactive use indefinitely. This is a safety net, not a tuning
    /// parameter — if you're hitting it, your benchmarks are too slow or
    /// the system is too noisy.
    pub max_wall_time: Duration,
    /// Noise threshold for practical significance (default: 0.01 = 1%).
    ///
    /// When set, a difference is only reported as significant if the entire
    /// 95% CI falls outside ±noise_threshold of zero (relative to baseline).
    /// This prevents "statistically significant but unmeasurably small"
    /// reports from triggering CI failures or green/red coloring.
    ///
    /// Set to 0.0 to disable (pure CI-based significance).
    pub noise_threshold: f64,
    /// Number of bootstrap resamples for confidence intervals (default: 10,000).
    ///
    /// Higher values give more precise CI bounds at the tails. 10K is fine
    /// for 95% CIs; increase to 100K for 99% CIs or extreme quantile work.
    pub bootstrap_resamples: usize,
    /// Cold-start measurement mode.
    ///
    /// When `true`, forces `min_iterations = 1`, `max_iterations = 1`,
    /// and `cache_firewall = true`. Each sample is a single cold call
    /// with L2 cache spoiled between samples. Results reflect first-call
    /// performance, not hot-loop throughput.
    ///
    /// Use for: CLI tools, serverless cold starts, first-request latency.
    pub cold_start: bool,
}

impl Default for GroupConfig {
    fn default() -> Self {
        Self {
            max_rounds: 200,
            min_rounds: 5,
            warmup_time: Duration::from_millis(500),
            max_time: Duration::from_secs(10),
            min_iterations: 1,
            max_iterations: 10_000_000,
            cache_firewall: false,
            cache_firewall_bytes: 2 * 1024 * 1024, // 2 MiB — enough to spoil L2 on most modern CPUs
            baseline_only: None,                   // auto: true when > 3 benchmarks
            expect_sub_ns: false,
            sort_by_speed: false,
            auto_rounds: true,
            target_precision: 0.02,
            max_wall_time: Duration::from_secs(120),
            noise_threshold: 0.01, // 1% — suppress sub-1% differences
            bootstrap_resamples: 10_000,
            cold_start: false,
        }
    }
}

impl GroupConfig {
    pub fn max_rounds(&mut self, max_rounds: usize) -> &mut Self {
        self.max_rounds = max_rounds;
        self
    }

    pub fn min_rounds(&mut self, min_rounds: usize) -> &mut Self {
        self.min_rounds = min_rounds;
        self
    }

    pub fn warmup_time(&mut self, dur: Duration) -> &mut Self {
        self.warmup_time = dur;
        self
    }

    pub fn max_time(&mut self, dur: Duration) -> &mut Self {
        self.max_time = dur;
        self
    }

    pub fn cache_firewall(&mut self, enabled: bool) -> &mut Self {
        self.cache_firewall = enabled;
        self
    }

    pub fn cache_firewall_bytes(&mut self, bytes: usize) -> &mut Self {
        self.cache_firewall_bytes = bytes;
        self
    }

    pub fn sort_by_speed(&mut self, enabled: bool) -> &mut Self {
        self.sort_by_speed = enabled;
        self
    }

    pub fn baseline_only(&mut self, enabled: bool) -> &mut Self {
        self.baseline_only = Some(enabled);
        self
    }

    pub fn expect_sub_ns(&mut self, enabled: bool) -> &mut Self {
        self.expect_sub_ns = enabled;
        self
    }

    pub fn auto_rounds(&mut self, enabled: bool) -> &mut Self {
        self.auto_rounds = enabled;
        self
    }

    pub fn target_precision(&mut self, precision: f64) -> &mut Self {
        self.target_precision = precision;
        self
    }

    pub fn max_wall_time(&mut self, dur: Duration) -> &mut Self {
        self.max_wall_time = dur;
        self
    }

    /// Set the noise threshold for practical significance (default: 0.01 = 1%).
    ///
    /// Changes smaller than this (as a fraction of baseline) are reported as
    /// "within noise" even if statistically significant. Set to 0.0 to disable.
    pub fn noise_threshold(&mut self, threshold: f64) -> &mut Self {
        self.noise_threshold = threshold;
        self
    }

    /// Set the number of bootstrap resamples (default: 10,000).
    pub fn bootstrap_resamples(&mut self, n: usize) -> &mut Self {
        self.bootstrap_resamples = n.max(100); // minimum 100
        self
    }

    /// Enable cold-start mode: 1 call/sample with L2 cache spoiling.
    ///
    /// Measures first-call performance, not hot-loop throughput.
    pub fn cold_start(&mut self, enabled: bool) -> &mut Self {
        self.cold_start = enabled;
        if enabled {
            self.min_iterations = 1;
            self.max_iterations = 1;
            self.cache_firewall = true;
        }
        self
    }
}

/// A named benchmark function with optional tags.
pub struct Benchmark {
    pub(crate) name: String,
    pub(crate) tags: Vec<(String, String)>,
    pub(crate) subgroup: Option<String>,
    pub(crate) func: BenchFn,
}

/// Type-erased benchmark function.
pub struct BenchFn {
    inner: Box<dyn FnMut(&mut Bencher) + Send>,
}

impl BenchFn {
    pub fn new<F: FnMut(&mut Bencher) + Send + 'static>(f: F) -> Self {
        Self { inner: Box::new(f) }
    }

    pub(crate) fn call(&mut self, bencher: &mut Bencher) {
        (self.inner)(bencher);
    }
}

/// Controls the measurement of a single benchmark iteration.
///
/// The `Bencher` is passed to your benchmark function. Call `iter` or
/// `with_input` + `run` to define what gets measured.
///
/// # Teardown
///
/// `with_input().run()` excludes both setup AND teardown from timing.
/// `iter()` includes teardown (drop of return value) in timing — use
/// [`iter_deferred_drop`](Self::iter_deferred_drop) or `with_input().run()`
/// when the return type has expensive drop.
pub struct Bencher {
    /// Number of iterations for this sample.
    pub(crate) iterations: usize,
    /// Total elapsed wall-clock nanoseconds for this sample.
    pub(crate) elapsed_ns: u64,
    /// Total CPU (user) nanoseconds for this sample. 0 when `cpu-time` feature disabled.
    pub(crate) cpu_ns: u64,
    /// TSC frequency in ticks/ns. When `Some`, uses hardware TSC for timing.
    /// When `None`, falls back to `Instant::now()`.
    /// Only populated when `precise-timing` feature is active and hardware supports it.
    /// Always present in the struct for simpler code paths — read only with the feature.
    #[cfg_attr(not(feature = "precise-timing"), allow(dead_code))]
    pub(crate) tsc_ticks_per_ns: Option<f64>,
    /// Allocation delta for this sample (when alloc-profiling is active).
    #[cfg(feature = "alloc-profiling")]
    pub(crate) alloc_delta: Option<crate::alloc::AllocSnapshot>,
}

impl Bencher {
    pub(crate) fn new(iterations: usize) -> Self {
        Self {
            iterations,
            elapsed_ns: 0,
            cpu_ns: 0,
            tsc_ticks_per_ns: None,
            #[cfg(feature = "alloc-profiling")]
            alloc_delta: None,
        }
    }

    pub(crate) fn new_with_tsc(iterations: usize, tsc_ticks_per_ns: Option<f64>) -> Self {
        Self {
            iterations,
            elapsed_ns: 0,
            cpu_ns: 0,
            tsc_ticks_per_ns,
            #[cfg(feature = "alloc-profiling")]
            alloc_delta: None,
        }
    }

    /// Measure a function that takes no input.
    ///
    /// The function is called `iterations` times and the total time is recorded.
    /// The return value is passed through `black_box` to prevent dead code elimination.
    ///
    /// **Note:** Drop cost of the return value IS included in timing. If the return
    /// type has expensive drop (e.g., large `Vec`), use
    /// [`iter_deferred_drop`](Self::iter_deferred_drop) or `with_input().run()` instead.
    #[inline(never)]
    pub fn iter<O, F: FnMut() -> O>(&mut self, mut f: F) {
        #[cfg(feature = "alloc-profiling")]
        let alloc_before = crate::alloc::AllocSnapshot::now();

        #[cfg(feature = "cpu-time")]
        let cpu_start = cpu_time::ThreadTime::now();

        // Use TSC when available (sub-ns precision, properly serialized).
        // Fall back to Instant::now() otherwise.
        #[cfg(feature = "precise-timing")]
        if let Some(ticks_per_ns) = self.tsc_ticks_per_ns {
            crate::timing::compiler_fence();
            let start = crate::timing::tsc_start();
            for _ in 0..self.iterations {
                std::hint::black_box(f());
            }
            let end = crate::timing::tsc_end();
            crate::timing::compiler_fence();
            self.elapsed_ns = crate::timing::ticks_to_ns(end.wrapping_sub(start), ticks_per_ns);
        } else {
            crate::timing::compiler_fence();
            let start = std::time::Instant::now();
            for _ in 0..self.iterations {
                std::hint::black_box(f());
            }
            self.elapsed_ns = start.elapsed().as_nanos() as u64;
            crate::timing::compiler_fence();
        }

        #[cfg(not(feature = "precise-timing"))]
        {
            let start = std::time::Instant::now();
            for _ in 0..self.iterations {
                std::hint::black_box(f());
            }
            self.elapsed_ns = start.elapsed().as_nanos() as u64;
        }

        #[cfg(feature = "cpu-time")]
        {
            self.cpu_ns = cpu_start.elapsed().as_nanos() as u64;
        }

        #[cfg(feature = "alloc-profiling")]
        {
            self.alloc_delta = Some(crate::alloc::AllocSnapshot::now().delta(alloc_before));
        }
    }

    /// Measure a function, deferring drop of outputs until after timing.
    ///
    /// Like [`iter`](Self::iter), but outputs are collected in a pre-allocated
    /// buffer during the timed loop and dropped only after timing ends. Use when
    /// the return type has an expensive [`Drop`] (e.g., `Vec`, `String`, file
    /// handles, database connections).
    ///
    /// For types where `Drop` is trivial (integers, small structs, `Copy` types),
    /// prefer [`iter`](Self::iter) — it avoids the buffer allocation and has less
    /// per-iteration overhead.
    ///
    /// # Example
    /// ```rust,ignore
    /// b.iter_deferred_drop(|| {
    ///     let mut v = Vec::with_capacity(1024);
    ///     v.extend(0..1024);
    ///     v  // Drop of this Vec is excluded from timing
    /// });
    /// ```
    #[inline(never)]
    pub fn iter_deferred_drop<O, F: FnMut() -> O>(&mut self, mut f: F) {
        let mut outputs: Vec<O> = Vec::with_capacity(self.iterations);

        #[cfg(feature = "alloc-profiling")]
        let alloc_before = crate::alloc::AllocSnapshot::now();

        #[cfg(feature = "cpu-time")]
        let cpu_start = cpu_time::ThreadTime::now();

        #[cfg(feature = "precise-timing")]
        if let Some(ticks_per_ns) = self.tsc_ticks_per_ns {
            crate::timing::compiler_fence();
            let start = crate::timing::tsc_start();
            for _ in 0..self.iterations {
                outputs.push(std::hint::black_box(f()));
            }
            let end = crate::timing::tsc_end();
            crate::timing::compiler_fence();
            self.elapsed_ns = crate::timing::ticks_to_ns(end.wrapping_sub(start), ticks_per_ns);
        } else {
            crate::timing::compiler_fence();
            let start = std::time::Instant::now();
            for _ in 0..self.iterations {
                outputs.push(std::hint::black_box(f()));
            }
            self.elapsed_ns = start.elapsed().as_nanos() as u64;
            crate::timing::compiler_fence();
        }

        #[cfg(not(feature = "precise-timing"))]
        {
            let start = std::time::Instant::now();
            for _ in 0..self.iterations {
                outputs.push(std::hint::black_box(f()));
            }
            self.elapsed_ns = start.elapsed().as_nanos() as u64;
        }

        #[cfg(feature = "cpu-time")]
        {
            self.cpu_ns = cpu_start.elapsed().as_nanos() as u64;
        }

        #[cfg(feature = "alloc-profiling")]
        {
            self.alloc_delta = Some(crate::alloc::AllocSnapshot::now().delta(alloc_before));
        }

        // Prevent the compiler from seeing the writes as dead stores.
        // Without this, LLVM could reason that outputs is only dropped
        // and remove the pushes entirely.
        std::hint::black_box(&outputs);
        drop(outputs);
    }

    /// Create a builder that provides fresh input for each iteration.
    ///
    /// The `setup` closure is called before each iteration to produce input.
    /// Both setup time and teardown (drop of output) are excluded from measurement.
    pub fn with_input<I, S: FnMut() -> I + 'static>(&mut self, setup: S) -> InputBencher<'_, I, S> {
        InputBencher {
            bencher: self,
            setup,
        }
    }
}

/// Builder for benchmarks that need fresh input per iteration.
pub struct InputBencher<'a, I, S: FnMut() -> I> {
    bencher: &'a mut Bencher,
    setup: S,
}

impl<I, S: FnMut() -> I> InputBencher<'_, I, S> {
    /// Run the benchmark function with input from the setup closure.
    ///
    /// Setup time and teardown (drop of output) are both excluded from measurement.
    /// Only the `f` closure execution is timed.
    #[inline(never)]
    pub fn run<O, F: FnMut(I) -> O>(self, mut f: F) {
        let iterations = self.bencher.iterations;
        let mut setup = self.setup;
        let mut total_ns: u64 = 0;
        #[cfg(feature = "cpu-time")]
        let mut total_cpu_ns: u64 = 0;

        #[cfg(feature = "precise-timing")]
        let tsc = self.bencher.tsc_ticks_per_ns;

        for _ in 0..iterations {
            let input = std::hint::black_box(setup());

            #[cfg(feature = "cpu-time")]
            let cpu_start = cpu_time::ThreadTime::now();

            #[cfg(feature = "precise-timing")]
            let elapsed = if let Some(ticks_per_ns) = tsc {
                crate::timing::compiler_fence();
                let start = crate::timing::tsc_start();
                let output = std::hint::black_box(f(input));
                let end = crate::timing::tsc_end();
                crate::timing::compiler_fence();
                let ns =
                    crate::timing::ticks_to_ns(end.wrapping_sub(start), ticks_per_ns);
                drop(output);
                ns
            } else {
                crate::timing::compiler_fence();
                let start = std::time::Instant::now();
                let output = std::hint::black_box(f(input));
                let elapsed = start.elapsed().as_nanos() as u64;
                crate::timing::compiler_fence();
                drop(output);
                elapsed
            };

            #[cfg(not(feature = "precise-timing"))]
            let elapsed = {
                let start = std::time::Instant::now();
                let output = std::hint::black_box(f(input));
                let elapsed = start.elapsed().as_nanos() as u64;
                drop(output);
                elapsed
            };

            total_ns += elapsed;

            #[cfg(feature = "cpu-time")]
            {
                total_cpu_ns += cpu_start.elapsed().as_nanos() as u64;
            }
        }
        self.bencher.elapsed_ns = total_ns;
        #[cfg(feature = "cpu-time")]
        {
            self.bencher.cpu_ns = total_cpu_ns;
        }
    }
}
