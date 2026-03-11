use crate::bench::{BenchGroup, Bencher, GroupConfig, Suite};
use crate::checks;
use crate::gate::{GateConfig, ResourceGate};
use crate::platform;
use crate::results::{BenchmarkResult, ComparisonResult, RunId, SuiteResult};
use crate::stats::{PairedAnalysis, Summary, Xoshiro256SS};
use std::path::PathBuf;
use std::time::Instant;

/// The benchmark execution engine.
///
/// Handles interleaved scheduling, resource gating, cross-process
/// coordination, and result collection.
pub struct Engine {
    suite: Suite,
    gate: ResourceGate,
    /// Directory for the cross-process lock file.
    lock_dir: Option<PathBuf>,
}

impl Engine {
    pub fn new(suite: Suite) -> Self {
        // Auto-detect CI and use appropriate gate config
        let gate_config = if platform::detect_ci().is_some() {
            GateConfig::ci()
        } else {
            GateConfig::default()
        };
        Self {
            suite,
            gate: ResourceGate::new(gate_config),
            lock_dir: default_lock_dir(),
        }
    }

    pub fn with_gate(suite: Suite, gate_config: GateConfig) -> Self {
        Self {
            suite,
            gate: ResourceGate::new(gate_config),
            lock_dir: default_lock_dir(),
        }
    }

    /// Set the directory for the cross-process lock file.
    /// Other zenbench processes using the same lock dir will wait their turn.
    pub fn lock_dir(mut self, dir: PathBuf) -> Self {
        self.lock_dir = Some(dir);
        self
    }

    /// Run all benchmarks and return results.
    pub fn run(mut self) -> SuiteResult {
        let run_id = RunId::generate();
        let ci = platform::detect_ci().map(String::from);
        let git_hash = platform::git_commit_hash();
        let start = Instant::now();

        // Acquire cross-process lock if configured
        let _lock = self
            .lock_dir
            .as_ref()
            .and_then(|dir| match ProcessLock::acquire(dir) {
                Ok(lock) => Some(lock),
                Err(e) => {
                    eprintln!("[zenbench] warning: could not acquire process lock: {e}");
                    None
                }
            });

        let mut comparisons = Vec::new();
        let mut standalones = Vec::new();

        // Run comparison groups (interleaved)
        for group in &mut self.suite.groups {
            if group.benchmarks.is_empty() {
                continue;
            }
            let result = run_comparison_group(group, &mut self.gate);
            comparisons.push(result);
        }

        // Run standalone benchmarks
        for bench in &mut self.suite.standalones {
            let result = run_standalone(bench, &mut self.gate, &GroupConfig::default());
            standalones.push(result);
        }

        let total_time = start.elapsed();
        let gate_unreliable = self.gate.is_unreliable();

        let result = SuiteResult {
            run_id,
            timestamp: chrono_now(),
            git_hash,
            ci_environment: ci,
            comparisons,
            standalones,
            total_time,
            gate_waits: self.gate.total_waits(),
            gate_wait_time: self.gate.total_wait_time(),
            unreliable: gate_unreliable,
        };

        // Print results to stderr
        result.print_report();

        // Run diagnostic checks and print warnings
        let mut warnings = Vec::new();
        for comp in &result.comparisons {
            for bench in &comp.benchmarks {
                warnings.extend(checks::check_benchmark(
                    &bench.name,
                    &bench.summary,
                    comp.completed_rounds,
                ));
            }
            for (base, cand, analysis) in &comp.analyses {
                if let Some(w) = checks::check_drift(base, cand, analysis.drift_correlation) {
                    warnings.push(w);
                }
            }
            if let Some(w) =
                checks::check_multiple_comparisons(&comp.group_name, comp.benchmarks.len())
            {
                warnings.push(w);
            }
        }
        for bench in &result.standalones {
            warnings.extend(checks::check_benchmark(
                &bench.name,
                &bench.summary,
                bench.summary.n,
            ));
        }
        if !warnings.is_empty() {
            eprintln!("  warnings:");
            for w in &warnings {
                eprintln!("    \x1b[33m⚠ {w}\x1b[0m");
            }
            eprintln!();
        }

        // Lock is released when _lock drops
        result
    }
}

/// Run a comparison group with interleaved execution.
fn run_comparison_group(group: &mut BenchGroup, gate: &mut ResourceGate) -> ComparisonResult {
    let config = &group.config;
    let n_benchmarks = group.benchmarks.len();
    let group_start = Instant::now();

    // Phase 1: Warmup — run each benchmark to fill caches and estimate iteration count
    eprintln!("[zenbench] warming up group '{}'...", group.name);
    let mut estimates = Vec::with_capacity(n_benchmarks);
    for bench in group.benchmarks.iter_mut() {
        let est = estimate_iterations(&mut bench.func, config);
        estimates.push(est);
    }
    let iterations_per_sample = estimates.iter().copied().min().unwrap_or(1).max(1);

    // Phase 2: Interleaved measurement
    eprintln!(
        "[zenbench] measuring group '{}' ({} benchmarks, ~{} iters/sample)...",
        group.name, n_benchmarks, iterations_per_sample
    );

    // Storage: samples[bench_idx] = vec of raw elapsed_ns per round
    let mut samples: Vec<Vec<u64>> = vec![Vec::with_capacity(config.rounds); n_benchmarks];
    let mut iters_per_round: Vec<usize> = Vec::with_capacity(config.rounds);
    let mut rng = Xoshiro256SS::seed(0xBE0C_0BAD_0000_0001);

    // Cache firewall buffer
    let firewall = if config.cache_firewall {
        Some(CacheFirewall::new(config.cache_firewall_bytes))
    } else {
        None
    };

    let mut completed_rounds = 0;

    for _round in 0..config.rounds {
        // Check time limit
        if group_start.elapsed() >= config.max_time {
            break;
        }

        // Resource gate: wait for clear
        gate.wait_for_clear();

        // Randomize benchmark order for this round
        let order = random_permutation(n_benchmarks, &mut rng);

        // Anti-aliasing jitter: vary iteration count ±20% per round.
        // Prevents synchronization with periodic system events (timer
        // interrupts, scheduling quanta). Inspired by nanobench.
        let jitter = (rng.next_u64() % 41) as i64 - 20; // -20..+20
        let round_iters = ((iterations_per_sample as i64
            + iterations_per_sample as i64 * jitter / 100)
            .max(1)) as usize;
        iters_per_round.push(round_iters);

        for &bench_idx in &order {
            // Cache firewall between benchmarks
            if let Some(fw) = &firewall {
                fw.spoil();
            }

            // Yield to OS if configured
            if config.yield_between_samples {
                std::thread::yield_now();
            }

            // Run the benchmark
            let bench = &mut group.benchmarks[bench_idx];
            let mut bencher = Bencher::new(round_iters);
            bench.func.call(&mut bencher);

            samples[bench_idx].push(bencher.elapsed_ns);
        }

        completed_rounds += 1;
    }

    // Phase 3: Compute paired statistics for all pairs
    let mut analyses = Vec::new();
    let names: Vec<String> = group.benchmarks.iter().map(|b| b.name.clone()).collect();

    // Use first benchmark as baseline, compare all others against it
    if n_benchmarks >= 2 {
        let baseline_samples = &samples[0];
        for i in 1..n_benchmarks {
            let candidate_samples = &samples[i];
            let base_f64: Vec<f64> = baseline_samples.iter().map(|&v| v as f64).collect();
            let cand_f64: Vec<f64> = candidate_samples.iter().map(|&v| v as f64).collect();

            if let Some(analysis) = PairedAnalysis::compute(&base_f64, &cand_f64, &iters_per_round)
            {
                analyses.push((names[0].clone(), names[i].clone(), analysis));
            }
        }

        // Also compute all-pairs if there are more than 2 benchmarks
        if n_benchmarks > 2 {
            for i in 1..n_benchmarks {
                for j in (i + 1)..n_benchmarks {
                    let base_f64: Vec<f64> = samples[i].iter().map(|&v| v as f64).collect();
                    let cand_f64: Vec<f64> = samples[j].iter().map(|&v| v as f64).collect();
                    if let Some(analysis) =
                        PairedAnalysis::compute(&base_f64, &cand_f64, &iters_per_round)
                    {
                        analyses.push((names[i].clone(), names[j].clone(), analysis));
                    }
                }
            }
        }
    }

    // Compute individual summaries
    let mut individual_results = Vec::new();
    for (i, bench) in group.benchmarks.iter().enumerate() {
        let per_iter: Vec<f64> = samples[i]
            .iter()
            .zip(iters_per_round.iter())
            .map(|(&elapsed, &iters)| elapsed as f64 / iters as f64)
            .collect();
        let summary = Summary::from_slice(&per_iter);
        individual_results.push(BenchmarkResult {
            name: bench.name.clone(),
            summary,
        });
    }

    ComparisonResult {
        group_name: group.name.clone(),
        benchmarks: individual_results,
        analyses,
        completed_rounds,
    }
}

/// Run a standalone benchmark (not compared).
fn run_standalone(
    bench: &mut crate::bench::Benchmark,
    gate: &mut ResourceGate,
    config: &GroupConfig,
) -> BenchmarkResult {
    gate.wait_for_clear();

    let iterations = estimate_iterations(&mut bench.func, config);
    let mut samples = Vec::with_capacity(config.rounds);

    let start = Instant::now();
    for _ in 0..config.rounds {
        if start.elapsed() >= config.max_time {
            break;
        }
        gate.wait_for_clear();

        let mut bencher = Bencher::new(iterations);
        bench.func.call(&mut bencher);
        samples.push(bencher.elapsed_ns as f64 / iterations as f64);
    }

    let summary = Summary::from_slice(&samples);
    BenchmarkResult {
        name: bench.name.clone(),
        summary,
    }
}

/// Estimate how many iterations fit in ~10ms.
fn estimate_iterations(func: &mut crate::bench::BenchFn, config: &GroupConfig) -> usize {
    let target_ns = 10_000_000u64; // 10ms
    let mut iters = 1;

    for _ in 0..5 {
        let mut bencher = Bencher::new(iters);
        func.call(&mut bencher);
        let elapsed = bencher.elapsed_ns.max(1_000); // Don't trust < 1µs
        let per_iter = elapsed / iters as u64;
        let per_iter = per_iter.max(1);
        let new_iters = (target_ns / per_iter) as usize;

        if new_iters <= 2 * iters {
            // Converged
            return new_iters.clamp(config.min_iterations, config.max_iterations);
        }
        iters = new_iters;
    }

    iters.clamp(config.min_iterations, config.max_iterations)
}

/// Generate a random permutation of 0..n.
fn random_permutation(n: usize, rng: &mut Xoshiro256SS) -> Vec<usize> {
    let mut perm: Vec<usize> = (0..n).collect();
    // Fisher-Yates shuffle
    for i in (1..n).rev() {
        let j = (rng.next_u64() as usize) % (i + 1);
        perm.swap(i, j);
    }
    perm
}

/// Cache firewall: reads a buffer to spoil CPU cache lines.
///
/// Uses `black_box` reads to force the CPU to evict cached benchmark data.
struct CacheFirewall {
    // Aligned to cache line size (64 bytes). Each u64 is 8 bytes,
    // so 8 u64s = 64 bytes = 1 cache line.
    data: Vec<u64>,
}

impl CacheFirewall {
    fn new(bytes: usize) -> Self {
        let n_u64s = bytes.div_ceil(8);
        let data = vec![0x5A45_4E42_454E_4348u64; n_u64s]; // "ZENBENCH"
        Self { data }
    }

    /// Read all data through `black_box` to force cache eviction.
    fn spoil(&self) {
        for chunk in self.data.chunks(8) {
            // Read one value per ~cache line
            std::hint::black_box(chunk[0]);
        }
    }
}

/// Simple timestamp without depending on chrono.
fn chrono_now() -> String {
    // Use system time for a basic ISO-8601 timestamp
    let now = std::time::SystemTime::now();
    let dur = now
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();

    // Basic UTC timestamp: seconds since epoch as ISO-8601-ish
    // Good enough for identification; not worth adding chrono dep
    let days = secs / 86400;
    let remaining = secs % 86400;
    let hours = remaining / 3600;
    let minutes = (remaining % 3600) / 60;
    let seconds = remaining % 60;

    // Days since 1970-01-01. Simple conversion (ignoring leap seconds).
    let mut y = 1970i64;
    let mut d = days as i64;
    loop {
        let year_days = if is_leap(y) { 366 } else { 365 };
        if d < year_days {
            break;
        }
        d -= year_days;
        y += 1;
    }

    let month_days = if is_leap(y) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut m = 1;
    for &md in &month_days {
        if d < md {
            break;
        }
        d -= md;
        m += 1;
    }

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y,
        m,
        d + 1,
        hours,
        minutes,
        seconds
    )
}

fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Default lock directory: temp dir with a zenbench subdirectory.
fn default_lock_dir() -> Option<PathBuf> {
    Some(std::env::temp_dir().join("zenbench"))
}

/// Cross-process advisory lock using fs4.
///
/// When multiple zenbench processes run simultaneously, they take turns
/// via this lock. The lock is released when this struct is dropped.
struct ProcessLock {
    _file: std::fs::File,
}

impl ProcessLock {
    fn acquire(dir: &std::path::Path) -> std::io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        let lock_path = dir.join("zenbench.lock");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .read(true)
            .open(&lock_path)?;

        // Write our PID so other processes can see who holds the lock
        use std::io::Write;
        let mut f = &file;
        let _ = write!(f, "{}", std::process::id());

        eprintln!("[zenbench] acquiring process lock...");
        // Blocking lock — waits until other zenbench processes finish
        fs4::fs_std::FileExt::lock_exclusive(&file)?;
        eprintln!("[zenbench] lock acquired.");

        Ok(Self { _file: file })
    }
}

impl Drop for ProcessLock {
    fn drop(&mut self) {
        let _ = fs4::fs_std::FileExt::unlock(&self._file);
    }
}
