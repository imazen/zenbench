use crate::bench::{BenchGroup, Bencher, GroupConfig, Suite};
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

        // Print results to stderr (includes inline footnotes for issues)
        result.print_report();

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
    // ETA: warmup gave us a rough per-sample time. Estimate total.
    // Each round = n_benchmarks samples. Time = rounds × n_benchmarks × sample_time.
    let warmup_elapsed = group_start.elapsed();
    let sample_time_est = if n_benchmarks > 0 && !warmup_elapsed.is_zero() {
        // Warmup ran each benchmark ~5 times for estimation.
        // Rough per-sample = warmup_time / (n_benchmarks * warmup_iterations)
        warmup_elapsed / (n_benchmarks as u32 * 5)
    } else {
        std::time::Duration::from_millis(10)
    };
    let eta_secs = sample_time_est.as_secs_f64() * config.rounds as f64 * n_benchmarks as f64;
    let eta_str = if eta_secs >= 60.0 {
        format!("{:.0}m{:.0}s", eta_secs / 60.0, eta_secs % 60.0)
    } else {
        format!("{:.0}s", eta_secs)
    };
    eprintln!(
        "[zenbench] measuring '{}' (~{} iters/sample, est. {eta_str})...",
        group.name, iterations_per_sample,
    );

    // Storage: samples[bench_idx] = vec of raw elapsed_ns per round
    let mut samples: Vec<Vec<u64>> = vec![Vec::with_capacity(config.rounds); n_benchmarks];
    // CPU time samples (parallel to wall time samples)
    let mut cpu_samples: Vec<Vec<u64>> = vec![Vec::with_capacity(config.rounds); n_benchmarks];
    let mut iters_per_round: Vec<usize> = Vec::with_capacity(config.rounds);
    let mut rng = Xoshiro256SS::seed(0xBE0C_0BAD_0000_0001);

    // Cache firewall buffer
    let firewall = if config.cache_firewall {
        Some(CacheFirewall::new(config.cache_firewall_bytes))
    } else {
        None
    };

    let mut completed_rounds = 0;
    let mut measurement_time = std::time::Duration::ZERO;

    for round in 0..config.rounds {
        // Check time limit against measurement time only (excludes gate waits).
        // Only enforce after min_rounds so slow benchmarks still get enough data.
        if round >= config.min_rounds && measurement_time >= config.max_time {
            break;
        }

        // Resource gate: cap the wait to remaining wall-clock budget so we
        // don't burn 30s on a gate when max_time is 10s.
        // After min_rounds, use 3x measurement budget as wall cap.
        // During min_rounds, allow up to 10x to ensure we get enough data.
        let wall_multiplier = if round < config.min_rounds { 10 } else { 3 };
        let wall_remaining = config
            .max_time
            .saturating_mul(wall_multiplier)
            .saturating_sub(group_start.elapsed());
        if round >= config.min_rounds && wall_remaining.is_zero() {
            break;
        }
        gate.wait_for_clear_with_deadline(Some(
            wall_remaining.max(std::time::Duration::from_secs(1)),
        ));

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

        let round_start = Instant::now();

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
            cpu_samples[bench_idx].push(bencher.cpu_ns);
        }

        measurement_time += round_start.elapsed();
        completed_rounds += 1;

        // Auto-rounds convergence check.
        //
        // For comparison groups: check paired differences against baseline.
        // A pair is "resolved" when EITHER:
        //   - Direction resolved: 95% CI on paired diff excludes zero
        //   - Equivalence established: CI width < target_precision × baseline mean
        //     (the largest plausible difference is smaller than we care about)
        //
        // For standalone (1 benchmark): individual precision check.
        //
        // Stop when ALL pairs are resolved.
        if config.auto_rounds
            && completed_rounds >= config.min_rounds.max(30)
            && completed_rounds % 10 == 0
        {
            let n = completed_rounds;
            let baseline_idx = group
                .baseline_name
                .as_ref()
                .and_then(|name| group.benchmarks.iter().position(|b| b.name == *name))
                .unwrap_or(0);

            // Convergence requires TWO things for each pair:
            //
            // 1. RESOLVED: either direction is clear (CI excludes zero)
            //    or equivalence is established (CI narrow, crosses zero).
            //    This answers "which is faster?"
            //
            // 2. STABLE: the effect size estimate is precise enough to be
            //    reproducible. CI half-width on the difference must be
            //    small relative to BOTH the baseline mean (so the reported
            //    percentage is stable) AND the difference itself when it's
            //    large (so a "42% faster" claim won't become "38%" next run).
            //
            // Without (2), we'd stop as soon as we know the direction,
            // but the magnitude would be noisy — humans remember "40% faster"
            // and compare across runs.

            let converged = if n_benchmarks < 2 {
                // Standalone: individual precision
                let (mean, std_dev) = streaming_mean_stddev(&samples[0], &iters_per_round);
                mean.abs() < f64::EPSILON
                    || (1.96 * std_dev / ((n as f64).sqrt() * mean.abs()) < config.target_precision)
            } else {
                // Comparison: check each baseline pair
                (0..n_benchmarks).all(|i| {
                    if i == baseline_idx {
                        return true;
                    }
                    // Streaming paired-difference stats
                    let mut diff_sum = 0.0_f64;
                    let mut diff_sum_sq = 0.0_f64;
                    for round in 0..n {
                        let base_per_iter =
                            samples[baseline_idx][round] as f64 / iters_per_round[round] as f64;
                        let cand_per_iter =
                            samples[i][round] as f64 / iters_per_round[round] as f64;
                        let diff = cand_per_iter - base_per_iter;
                        diff_sum += diff;
                        diff_sum_sq += diff * diff;
                    }
                    let diff_mean = diff_sum / n as f64;
                    let diff_var = (diff_sum_sq / n as f64) - (diff_mean * diff_mean);
                    let diff_stderr = diff_var.max(0.0).sqrt() / (n as f64).sqrt();
                    let ci_half = 1.96 * diff_stderr;

                    let (base_mean, _) =
                        streaming_mean_stddev(&samples[baseline_idx], &iters_per_round);

                    // (1) RESOLVED?
                    let direction_clear =
                        (diff_mean - ci_half > 0.0) || (diff_mean + ci_half < 0.0);

                    // CI width relative to baseline mean — controls precision
                    // of the reported percentage (e.g., "-42% ± 2%")
                    let ci_pct_of_baseline = if base_mean.abs() > f64::EPSILON {
                        2.0 * ci_half / base_mean.abs()
                    } else {
                        0.0
                    };

                    let equivalent = ci_pct_of_baseline < config.target_precision;

                    let resolved = direction_clear || equivalent;

                    // (2) STABLE? The effect size estimate won't shift much
                    // between runs. For large differences, we want the CI
                    // to be tight relative to the difference itself.
                    // For small/zero differences, baseline-relative is enough.
                    let stable = if diff_mean.abs() > f64::EPSILON && direction_clear {
                        // Relative precision of the difference itself
                        // e.g., diff = -42% ± 3% → ci_half/diff = 7% relative error
                        let effect_precision = ci_half / diff_mean.abs();
                        // Require < 10% relative error on the effect size,
                        // AND < target_precision of baseline (absolute floor)
                        effect_precision < 0.10 && ci_pct_of_baseline < config.target_precision
                    } else {
                        // No meaningful difference or equivalence case —
                        // baseline-relative precision is sufficient
                        ci_pct_of_baseline < config.target_precision
                    };

                    resolved && stable
                })
            };

            if converged {
                eprintln!(
                    "[zenbench] '{}' converged after {} rounds",
                    group.name, completed_rounds,
                );
                break;
            }
        }
    }

    // Phase 3: Compute paired statistics
    let mut analyses = Vec::new();
    let names: Vec<String> = group.benchmarks.iter().map(|b| b.name.clone()).collect();

    // Determine baseline index
    let baseline_idx = group
        .baseline_name
        .as_ref()
        .and_then(|name| names.iter().position(|n| n == name))
        .unwrap_or(0);

    // Auto-detect baseline_only: default to true when > 3 benchmarks
    let baseline_only = config.baseline_only.unwrap_or(n_benchmarks > 3);

    if n_benchmarks >= 2 {
        // Compare all benchmarks against the baseline
        let baseline_samples = &samples[baseline_idx];
        for i in 0..n_benchmarks {
            if i == baseline_idx {
                continue;
            }
            let candidate_samples = &samples[i];
            let base_f64: Vec<f64> = baseline_samples.iter().map(|&v| v as f64).collect();
            let cand_f64: Vec<f64> = candidate_samples.iter().map(|&v| v as f64).collect();

            if let Some(analysis) = PairedAnalysis::compute(&base_f64, &cand_f64, &iters_per_round)
            {
                analyses.push((names[baseline_idx].clone(), names[i].clone(), analysis));
            }
        }

        // Also compute non-baseline pairs (always stored for JSON; filtered in report)
        if !baseline_only && n_benchmarks > 2 {
            for i in 0..n_benchmarks {
                for j in (i + 1)..n_benchmarks {
                    if i == baseline_idx || j == baseline_idx {
                        continue; // already computed above
                    }
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
    let has_cpu_time = cpu_samples.iter().any(|s| s.iter().any(|&v| v > 0));
    let mut individual_results = Vec::new();
    for (i, bench) in group.benchmarks.iter().enumerate() {
        let per_iter: Vec<f64> = samples[i]
            .iter()
            .zip(iters_per_round.iter())
            .map(|(&elapsed, &iters)| elapsed as f64 / iters as f64)
            .collect();
        let summary = Summary::from_slice(&per_iter);

        let cpu_summary = if has_cpu_time {
            let cpu_per_iter: Vec<f64> = cpu_samples[i]
                .iter()
                .zip(iters_per_round.iter())
                .map(|(&elapsed, &iters)| elapsed as f64 / iters as f64)
                .collect();
            Some(Summary::from_slice(&cpu_per_iter))
        } else {
            None
        };

        individual_results.push(BenchmarkResult {
            name: bench.name.clone(),
            summary,
            cpu_summary,
            tags: bench.tags.clone(),
            subgroup: bench.subgroup.clone(),
        });
    }

    ComparisonResult {
        group_name: group.name.clone(),
        benchmarks: individual_results,
        analyses,
        completed_rounds,
        throughput: group.throughput.clone(),
        cache_firewall: config.cache_firewall,
        cache_firewall_bytes: config.cache_firewall_bytes,
        baseline_only,
        throughput_unit: group.throughput_unit.clone(),
        sort_by_speed: config.sort_by_speed,
        expect_sub_ns: config.expect_sub_ns,
        iterations_per_sample,
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
    let mut cpu_samples_vec = Vec::with_capacity(config.rounds);

    let start = Instant::now();
    let mut measurement_time = std::time::Duration::ZERO;
    for round in 0..config.rounds {
        if round >= config.min_rounds && measurement_time >= config.max_time {
            break;
        }
        let wall_multiplier = if round < config.min_rounds { 10 } else { 3 };
        let wall_remaining = config
            .max_time
            .saturating_mul(wall_multiplier)
            .saturating_sub(start.elapsed());
        if round >= config.min_rounds && wall_remaining.is_zero() {
            break;
        }
        gate.wait_for_clear_with_deadline(Some(
            wall_remaining.max(std::time::Duration::from_secs(1)),
        ));

        let sample_start = Instant::now();
        let mut bencher = Bencher::new(iterations);
        bench.func.call(&mut bencher);
        measurement_time += sample_start.elapsed();
        samples.push(bencher.elapsed_ns as f64 / iterations as f64);
        cpu_samples_vec.push(bencher.cpu_ns as f64 / iterations as f64);
    }

    let summary = Summary::from_slice(&samples);
    let cpu_summary = if cpu_samples_vec.iter().any(|&v| v > 0.0) {
        Some(Summary::from_slice(&cpu_samples_vec))
    } else {
        None
    };

    BenchmarkResult {
        name: bench.name.clone(),
        summary,
        cpu_summary,
        tags: bench.tags.clone(),
        subgroup: bench.subgroup.clone(),
    }
}

/// Compute streaming mean and stddev of per-iteration times.
fn streaming_mean_stddev(raw_samples: &[u64], iters_per_round: &[usize]) -> (f64, f64) {
    let n = raw_samples.len();
    if n == 0 {
        return (0.0, 0.0);
    }
    let mut sum = 0.0_f64;
    let mut sum_sq = 0.0_f64;
    for (j, &elapsed) in raw_samples.iter().enumerate() {
        let per_iter = elapsed as f64 / iters_per_round[j] as f64;
        sum += per_iter;
        sum_sq += per_iter * per_iter;
    }
    let mean = sum / n as f64;
    let variance = (sum_sq / n as f64) - (mean * mean);
    (mean, variance.max(0.0).sqrt())
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
