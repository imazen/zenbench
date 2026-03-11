use std::time::Duration;

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
}

impl BenchGroup {
    pub(crate) fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            benchmarks: Vec::new(),
            config: GroupConfig::default(),
        }
    }

    /// Add a benchmark to this comparison group.
    pub fn bench<F>(&mut self, name: impl Into<String>, f: F)
    where
        F: FnMut(&mut Bencher) + Send + 'static,
    {
        self.benchmarks.push(Benchmark {
            name: name.into(),
            func: BenchFn::new(f),
        });
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

/// A named benchmark function.
pub struct Benchmark {
    pub(crate) name: String,
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
pub struct Bencher {
    /// Number of iterations for this sample.
    pub(crate) iterations: usize,
    /// Total elapsed nanoseconds for this sample (set after measurement).
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
    /// Use `black_box` on the return value to prevent dead code elimination.
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
    /// Setup time is NOT included in the measurement.
    pub fn with_input<I, S: FnMut() -> I + 'static>(
        &mut self,
        setup: S,
    ) -> InputBencher<'_, I, S> {
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
    /// Setup time is excluded from measurement. Only the `f` closure is timed.
    #[inline(never)]
    pub fn run<O, F: FnMut(I) -> O>(self, mut f: F) {
        let iterations = self.bencher.iterations;
        let mut setup = self.setup;
        let mut total_ns: u64 = 0;

        for _ in 0..iterations {
            let input = setup();
            let start = std::time::Instant::now();
            std::hint::black_box(f(input));
            total_ns += start.elapsed().as_nanos() as u64;
        }
        self.bencher.elapsed_ns = total_ns;
    }
}
