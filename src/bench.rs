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
    /// Returns (value, unit_string).
    pub fn compute(&self, mean_ns: f64) -> (f64, &'static str) {
        if mean_ns <= 0.0 {
            return (0.0, "?/s");
        }
        let seconds = mean_ns / 1e9;
        match self {
            Throughput::Bytes(n) => {
                let bytes_per_sec = *n as f64 / seconds;
                let gib = bytes_per_sec / (1024.0 * 1024.0 * 1024.0);
                if gib >= 1.0 {
                    (gib, "GiB/s")
                } else {
                    (bytes_per_sec / (1024.0 * 1024.0), "MiB/s")
                }
            }
            Throughput::Elements(n) => {
                let ops_per_sec = *n as f64 / seconds;
                if ops_per_sec >= 1e9 {
                    (ops_per_sec / 1e9, "Gops/s")
                } else if ops_per_sec >= 1e6 {
                    (ops_per_sec / 1e6, "Mops/s")
                } else if ops_per_sec >= 1e3 {
                    (ops_per_sec / 1e3, "Kops/s")
                } else {
                    (ops_per_sec, "ops/s")
                }
            }
        }
    }

    /// Compute throughput with a custom unit name for Elements.
    ///
    /// When `unit` is provided and this is `Elements`, the unit suffix
    /// uses the custom name (e.g., "checks" → "Gchecks/s").
    pub fn compute_named(&self, mean_ns: f64, unit: Option<&str>) -> (f64, String) {
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

    /// Format throughput from mean time in nanoseconds.
    pub fn format(&self, mean_ns: f64) -> String {
        self.format_named(mean_ns, None)
    }

    /// Format throughput with an optional custom unit name.
    pub fn format_named(&self, mean_ns: f64, unit: Option<&str>) -> String {
        let (val, unit_str) = self.compute_named(mean_ns, unit);
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
}

impl Suite {
    pub fn new() -> Self {
        Self {
            groups: Vec::new(),
            standalones: Vec::new(),
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
    /// Whether to yield to OS scheduler between samples.
    pub yield_between_samples: bool,
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
            yield_between_samples: false,
            baseline_only: None, // auto: true when > 3 benchmarks
            expect_sub_ns: false,
            sort_by_speed: false,
            auto_rounds: true,
            target_precision: 0.02,
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

    pub fn yield_between_samples(&mut self, enabled: bool) -> &mut Self {
        self.yield_between_samples = enabled;
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
/// `with_input().run()` when the return type has expensive drop.
pub struct Bencher {
    /// Number of iterations for this sample.
    pub(crate) iterations: usize,
    /// Total elapsed wall-clock nanoseconds for this sample.
    pub(crate) elapsed_ns: u64,
    /// Total CPU (user) nanoseconds for this sample. 0 when `cpu-time` feature disabled.
    pub(crate) cpu_ns: u64,
}

impl Bencher {
    pub(crate) fn new(iterations: usize) -> Self {
        Self {
            iterations,
            elapsed_ns: 0,
            cpu_ns: 0,
        }
    }

    /// Measure a function that takes no input.
    ///
    /// The function is called `iterations` times and the total time is recorded.
    /// The return value is passed through `black_box` to prevent dead code elimination.
    ///
    /// **Note:** Drop cost of the return value IS included in timing. If the return
    /// type has expensive drop (e.g., large `Vec`), use `with_input().run()` instead.
    #[inline(never)]
    pub fn iter<O, F: FnMut() -> O>(&mut self, mut f: F) {
        #[cfg(feature = "cpu-time")]
        let cpu_start = cpu_time::ThreadTime::now();

        let start = std::time::Instant::now();
        for _ in 0..self.iterations {
            std::hint::black_box(f());
        }
        self.elapsed_ns = start.elapsed().as_nanos() as u64;

        #[cfg(feature = "cpu-time")]
        {
            self.cpu_ns = cpu_start.elapsed().as_nanos() as u64;
        }
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

        for _ in 0..iterations {
            let input = std::hint::black_box(setup());

            #[cfg(feature = "cpu-time")]
            let cpu_start = cpu_time::ThreadTime::now();

            let start = std::time::Instant::now();
            let output = std::hint::black_box(f(input));
            let elapsed = start.elapsed().as_nanos() as u64;
            total_ns += elapsed;

            #[cfg(feature = "cpu-time")]
            {
                total_cpu_ns += cpu_start.elapsed().as_nanos() as u64;
            }

            drop(output); // teardown outside timing
        }
        self.bencher.elapsed_ns = total_ns;
        #[cfg(feature = "cpu-time")]
        {
            self.bencher.cpu_ns = total_cpu_ns;
        }
    }
}
