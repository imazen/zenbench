use serde::{Deserialize, Serialize};
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

    /// Format throughput from mean time in nanoseconds.
    pub fn format(&self, mean_ns: f64) -> String {
        let (val, unit) = self.compute(mean_ns);
        if val >= 100.0 {
            format!("{val:.0} {unit}")
        } else if val >= 10.0 {
            format!("{val:.1} {unit}")
        } else {
            format!("{val:.2} {unit}")
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
}

impl BenchGroup {
    pub(crate) fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            benchmarks: Vec::new(),
            config: GroupConfig::default(),
            throughput: None,
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
            func: BenchFn::new(f),
        });
    }

    /// Add a benchmark with key-value tags for multi-dimensional reporting.
    ///
    /// Tags enable grouping and pivoting in reports. Common tags:
    /// `("library", "zenflate")`, `("level", "L6")`, `("data", "mixed")`.
    pub fn bench_tagged<F>(
        &mut self,
        name: impl Into<String>,
        tags: &[(&str, &str)],
        f: F,
    ) where
        F: FnMut(&mut Bencher) + Send + 'static,
    {
        self.benchmarks.push(Benchmark {
            name: name.into(),
            tags: tags
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
            func: BenchFn::new(f),
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

    /// Configure this group's execution parameters.
    pub fn config(&mut self) -> &mut GroupConfig {
        &mut self.config
    }
}

/// Configuration for a benchmark group's execution.
#[derive(Debug, Clone)]
pub struct GroupConfig {
    /// Target number of measurement rounds.
    pub rounds: usize,
    /// Warmup time before measurement begins.
    pub warmup_time: Duration,
    /// Maximum total time for the group.
    pub max_time: Duration,
    /// Minimum iterations per sample.
    pub min_iterations: usize,
    /// Maximum iterations per sample.
    pub max_iterations: usize,
    /// Whether to spoil CPU cache between samples.
    pub cache_firewall: bool,
    /// Cache firewall size in bytes.
    pub cache_firewall_bytes: usize,
    /// Whether to yield to OS scheduler between samples.
    pub yield_between_samples: bool,
}

impl Default for GroupConfig {
    fn default() -> Self {
        Self {
            rounds: 200,
            warmup_time: Duration::from_millis(500),
            max_time: Duration::from_secs(10),
            min_iterations: 1,
            max_iterations: 10_000,
            cache_firewall: true,
            cache_firewall_bytes: 256 * 1024, // 256 KB — enough to spoil L2
            yield_between_samples: false,
        }
    }
}

impl GroupConfig {
    pub fn rounds(&mut self, rounds: usize) -> &mut Self {
        self.rounds = rounds;
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

    pub fn yield_between_samples(&mut self, enabled: bool) -> &mut Self {
        self.yield_between_samples = enabled;
        self
    }
}

/// A named benchmark function with optional tags.
pub struct Benchmark {
    pub(crate) name: String,
    pub(crate) tags: Vec<(String, String)>,
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
}

impl Bencher {
    pub(crate) fn new(iterations: usize) -> Self {
        Self {
            iterations,
            elapsed_ns: 0,
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
        let start = std::time::Instant::now();
        for _ in 0..self.iterations {
            std::hint::black_box(f());
        }
        self.elapsed_ns = start.elapsed().as_nanos() as u64;
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

        for _ in 0..iterations {
            let input = std::hint::black_box(setup());
            let start = std::time::Instant::now();
            let output = std::hint::black_box(f(input));
            let elapsed = start.elapsed().as_nanos() as u64;
            total_ns += elapsed;
            drop(output); // teardown outside timing
        }
        self.bencher.elapsed_ns = total_ns;
    }
}
