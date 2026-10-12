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
    gate_config: GateConfig,
    /// Directory for the cross-process lock file.
    lock_dir: Option<PathBuf>,
    /// Suppress the terminal report (header, per-group, footer) when set.
    /// Used by `run_passes` to avoid printing every pass's individual
    /// report; only the final aggregated report is shown.
    quiet: bool,
}

/// `--no-busy-gate` (CLI) or `ZENBENCH_NO_BUSY_GATE=1` (env) disables the resource gate
/// and the rival-benchmark wait, overriding any harness-supplied [`GateConfig`].
///
/// For harnesses whose benchmarks spawn their own worker threads (e.g. a
/// codec's internal rayon pool), where the gate cannot tell the harness's own
/// load from a co-tenant's. The run is still reported; per-round
/// `gate_clean` is `None`. Check machine load yourself when using it.
pub(crate) fn gate_override() -> bool {
    std::env::args().any(|a| a == "--no-busy-gate")
        || std::env::var("ZENBENCH_NO_BUSY_GATE").is_ok_and(|v| !v.is_empty() && v != "0")
}

fn apply_gate_override(config: GateConfig) -> GateConfig {
    resolve_gate(config, gate_override())
}

fn resolve_gate(config: GateConfig, disable: bool) -> GateConfig {
    if disable {
        eprintln!("[zenbench] resource gate disabled (--no-busy-gate / ZENBENCH_NO_BUSY_GATE)");
        GateConfig::disabled()
    } else {
        config
    }
}

impl Engine {
    pub fn new(suite: Suite) -> Self {
        // Auto-detect CI and use appropriate gate config
        let gate_config = apply_gate_override(if platform::detect_ci().is_some() {
            GateConfig::ci()
        } else {
            GateConfig::default()
        });
        Self {
            suite,
            gate_config,
            lock_dir: default_lock_dir(),
            quiet: false,
        }
    }

    pub fn with_gate(suite: Suite, gate_config: GateConfig) -> Self {
        // with_gate uses the user's config as-is, unless overridden.
        Self {
            suite,
            gate_config: apply_gate_override(gate_config),
            lock_dir: default_lock_dir(),
            quiet: false,
        }
    }

    /// Set the directory for the cross-process lock file.
    /// Other zenbench processes using the same lock dir will wait their turn.
    #[allow(dead_code)] // Used by bin targets
    pub fn lock_dir(mut self, dir: PathBuf) -> Self {
        self.lock_dir = Some(dir);
        self
    }

    /// Suppress the terminal report (header, per-group, footer) when running.
    /// Useful when an outer driver (e.g. `run_passes`) prints its own report
    /// over the aggregated results.
    pub fn quiet(mut self, quiet: bool) -> Self {
        self.quiet = quiet;
        self
    }

    /// Run all benchmarks and return results.
    pub fn run(mut self) -> SuiteResult {
        crate::report::startup_guidance();
        let plan = self.suite.plan.clone().resolve().unwrap_or_else(|e| {
            eprintln!("[zenbench] invalid benchmark plan: {e}; use --help or zenbench guide");
            std::process::exit(2);
        });
        let dry_run = std::env::args().any(|a| a == "--dry-run");
        let confirming = plan.mode != crate::BenchmarkMode::Iterate;
        if confirming && !dry_run && std::env::var("ZENBENCH_SUBPROCESS").as_deref() != Ok("1") {
            drop(self); // release registered inputs before starting the first child
            return crate::run_processes_planned(plan.processes, crate::Aggregation::Mean, plan);
        }
        if confirming {
            self.gate_config.strict = true;
            self.gate_config.max_wait_count = 0;
            self.gate_config.max_cpu_load = self.gate_config.max_cpu_load.min(0.05);
            self.gate_config.max_heavy_processes = 0;
            assert!(
                self.gate_config.enabled,
                "confirmation requires resource monitoring"
            );
            assert!(
                std::env::var_os("ZENBENCH_NO_SAVE").is_none(),
                "confirmation requires retained evidence"
            );
            #[cfg(feature = "criterion-compat")]
            assert!(
                self.suite.precomputed_comparisons.is_empty(),
                "confirmation requires registered groups, not immediate-mode measurements"
            );
        }
        for group in &mut self.suite.groups {
            if let Some(rounds) = plan.rounds {
                group.config.max_rounds = rounds;
            }
            if confirming {
                assert!(
                    group.config.max_rounds >= 2,
                    "confirmation requires at least two rounds per process"
                );
                group.config.min_rounds = group.config.max_rounds;
                group.config.auto_rounds = false;
            }
        }
        eprintln!(
            "[zenbench] mode={} inference={} workload={:?}",
            plan.mode,
            if confirming {
                "pending_process_confirmation"
            } else {
                "exploratory"
            },
            plan.workload_id
        );
        // Parse the environment once per process, before any benchmark can run.
        platform::configured_affinity().unwrap_or_else(|e| panic!("benchmark affinity: {e}"));
        let cpu_session =
            platform::CpuSession::start(self.suite.cpu_policy, self.suite.cpu_set.as_deref())
                .unwrap_or_else(|e| panic!("{e}"));
        eprintln!("[zenbench] {}", cpu_session.placement.llm());
        if dry_run {
            let groups: Vec<_> = self
                .suite
                .groups
                .iter()
                .filter(|g| !g.benchmarks.is_empty())
                .filter(|g| {
                    self.suite
                        .group_filter
                        .as_ref()
                        .is_none_or(|f| g.name.contains(f))
                })
                .map(|g| {
                    serde_json::json!({
                        "name": g.name,
                        "benchmarks": g.benchmarks.iter().map(|b| &b.name).collect::<Vec<_>>(),
                        "config": g.config,
                    })
                })
                .collect();
            if confirming && groups.is_empty() {
                eprintln!("[zenbench] empty confirmation selection");
                std::process::exit(2);
            }
            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                "plan": plan,
                "groups": groups,
                "cpu_placement": cpu_session.placement,
                "binary_sha256": crate::storage::binary_digest().expect("cannot hash executable"),
                "data_directory": crate::storage::data_dir(),
                "runtime_estimate": null,
                "note": "Registration has run; benchmark callbacks have not. Durations are budgets, not runtime predictions. Supervisor deadline applies to Confirm/Evidence child processes, not detached descendants.",
            })).expect("plan serializes"));
            std::process::exit(0);
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _admission = crate::coordination::Reservation::acquire("benchmark", plan.timeout)
            .expect("benchmark admission failed");
        // Acquire cross-process exclusive lock if configured. The lock
        // file carries the project / binary / current benchmark / ETA so
        // any waiting process can render an accurate "waiting on …"
        // message.
        let lock = self.lock_dir.as_ref().and_then(|dir| {
            let lock_path = dir.join("zenbench.lock");
            let initial_benchmark = self
                .suite
                .group_filter
                .clone()
                .or_else(|| self.suite.groups.first().map(|g| g.name.clone()))
                .unwrap_or_default();
            let cfg = crate::exclusive::AcquireConfig {
                path: Some(lock_path),
                benchmark: initial_benchmark,
                activity: format!("{} groups queued", self.suite.groups.len()),
                ..Default::default()
            };
            match crate::exclusive::Lock::acquire(cfg) {
                Ok(l) => Some(l),
                Err(e) => {
                    eprintln!("[zenbench] warning: could not acquire exclusive lock: {e}");
                    None
                }
            }
        });

        // Standalone demand warmup precedes calibration/timing metrics. This
        // cannot certify frequency or warm opaque pools on other threads.
        #[cfg(target_os = "macos")]
        {
            let elapsed = platform::warmup_cpu_for(std::time::Duration::from_millis(100));
            eprintln!(
                "[zenbench] separate CPU warmup: {}ms (frequency unverified)",
                elapsed.as_millis()
            );
        }
        let run_id = RunId::generate();
        let ci = platform::detect_ci().map(String::from);
        let git_hash = platform::git_commit_hash();
        let testbed = Some(platform::detect_testbed());
        let timer_res = platform::timer_resolution_ns();
        let loop_overhead_ns = measure_loop_overhead();

        // Run calibration workloads (opt-out: ZENBENCH_NO_CALIBRATE=1)
        let calibration = if std::env::var("ZENBENCH_NO_CALIBRATE").is_err() {
            let cal = crate::calibration::run_calibration();
            eprintln!(
                "[zenbench] calibration: int={:.2}ns/iter mem_bw={:.1}GiB/s mem_lat={:.1}ns",
                cal.integer_ns, cal.memory_bw_gibps, cal.memory_lat_ns,
            );
            Some(cal)
        } else {
            None
        };

        // Try to use hardware TSC timer for sub-ns precision
        #[cfg(feature = "precise-timing")]
        let tsc_ticks_per_ns: Option<f64> = match crate::timing::TscTimer::new() {
            Some(timer) => {
                let freq = timer.ticks_per_ns();
                eprintln!(
                    "[zenbench] timer resolution: {timer_res}ns, loop overhead: {:.2}ns/iter, \
                     TSC: {:.3} ticks/ns (invariant)",
                    loop_overhead_ns, freq,
                );
                Some(freq)
            }
            None => {
                eprintln!(
                    "[zenbench] timer resolution: {timer_res}ns, loop overhead: {:.2}ns/iter, \
                     TSC: unavailable (using Instant)",
                    loop_overhead_ns,
                );
                None
            }
        };
        #[cfg(not(feature = "precise-timing"))]
        let tsc_ticks_per_ns: Option<f64> = {
            eprintln!(
                "[zenbench] timer resolution: {timer_res}ns, loop overhead: {:.2}ns/iter",
                loop_overhead_ns,
            );
            None
        };
        let execution = crate::ExecutionRecord {
            protocol: "zenbench-process-v1".into(),
            plan: plan.clone(),
            binary_sha256: crate::storage::binary_digest()
                .expect("cannot hash benchmark executable"),
            process_id: std::process::id(),
            independent_processes: false,
            timer_backend: if tsc_ticks_per_ns.is_some() {
                "hardware_counter"
            } else {
                "instant"
            }
            .into(),
        };
        let start = Instant::now();

        // Durable compressed checkpoints are ignored by Git. Explicit snapshots
        // are the small, selected version-control export.
        let save_path = if std::env::var("ZENBENCH_NO_SAVE").is_ok() {
            None
        } else {
            Some(auto_save_path(&run_id))
        };
        if let Some(path) = &save_path {
            eprintln!("[zenbench] results → {}", path.display());
            let pending = SuiteResult {
                run_id: run_id.clone(),
                incomplete: true,
                ..Default::default()
            };
            pending
                .save(path)
                .unwrap_or_else(|e| panic!("cannot retain benchmark attempt: {e}"));
        }

        let mut comparisons: Vec<ComparisonResult> = Vec::new();
        // Include pre-computed results from criterion-compat immediate mode
        #[cfg(feature = "criterion-compat")]
        comparisons.extend(std::mem::take(&mut self.suite.precomputed_comparisons));
        for comparison in &comparisons {
            if let Some(placement) = &comparison.cpu_placement {
                assert!(
                    placement.process_compatible(&cpu_session.placement),
                    "precomputed comparison has different CPU placement"
                );
            }
        }
        let mut total_gate_waits = 0usize;
        let mut total_gate_wait_time = std::time::Duration::ZERO;

        // Print report header immediately (unless suppressed by trial driver)
        if !self.quiet {
            crate::report::print_header(&run_id, git_hash.as_deref(), ci.as_deref());
        }

        let group_filter = self.suite.group_filter.as_deref();

        // Count groups that will actually run, for ETA extrapolation.
        let total_groups: usize = self
            .suite
            .groups
            .iter()
            .filter(|g| !g.benchmarks.is_empty())
            .filter(|g| match group_filter {
                Some(f) => g.name == f || g.name.contains(f),
                None => true,
            })
            .count();
        let mut any_unreliable = self.lock_dir.is_some() && lock.is_none();
        let mut groups_done: usize = 0;

        // Run comparison groups (interleaved), streaming results to file
        for group in &mut self.suite.groups {
            if group.benchmarks.is_empty() {
                continue;
            }
            // Skip groups that don't match the filter
            if let Some(filter) = group_filter {
                if group.name != filter && !group.name.contains(filter) {
                    continue;
                }
            }
            // Tell the lock holder file what we're now running, so any
            // process waiting on the exclusive lock sees up-to-date info.
            if let Some(l) = lock.as_ref() {
                l.update_benchmark(&group.name);
            }
            // The sampler excludes the benchmark owner, including its threads.
            let mut gate = ResourceGate::new(self.gate_config.clone());
            let result = run_comparison_group(
                group,
                &mut gate,
                loop_overhead_ns,
                tsc_ticks_per_ns,
                timer_res,
                &cpu_session,
                self.suite.affinity_cores.as_deref(),
            );
            any_unreliable |= gate.is_unreliable();
            total_gate_waits += gate.total_waits();
            total_gate_wait_time += gate.total_wait_time();

            // Clear any status line and print this group's report immediately
            crate::report::clear_status();
            if !self.quiet {
                crate::report::print_group(&result, timer_res);
            }

            comparisons.push(result);
            groups_done += 1;

            // Refine ETA in the lock holder file: extrapolate from
            // elapsed time and remaining groups so peekers get a useful
            // "ETA in …" once we have a measurement to base it on.
            if let Some(l) = lock.as_ref() {
                if groups_done > 0 && groups_done < total_groups {
                    let elapsed = start.elapsed();
                    let avg = elapsed / (groups_done as u32);
                    let remaining = (total_groups - groups_done) as u32;
                    let eta = std::time::SystemTime::now() + avg * remaining;
                    l.update_eta(eta);
                }
            }

            // Checkpoint completed groups, including all raw rounds.
            if let Some(path) = &save_path {
                let partial = SuiteResult {
                    execution: Some(execution.clone()),
                    run_id: run_id.clone(),
                    timestamp: chrono_now(),
                    git_hash: git_hash.clone(),
                    ci_environment: ci.clone(),
                    comparisons: comparisons.clone(),
                    incomplete: true,
                    total_time: start.elapsed(),
                    unreliable: any_unreliable,
                    gate_waits: total_gate_waits,
                    gate_wait_time: total_gate_wait_time,
                    timer_resolution_ns: timer_res,
                    loop_overhead_ns,
                    testbed: testbed.clone(),
                    calibration: calibration.clone(),
                    cpu_placement: Some(cpu_session.placement.clone()),
                    ..Default::default()
                };
                partial
                    .save(path)
                    .unwrap_or_else(|e| panic!("cannot checkpoint benchmark attempt: {e}"));
            }
        }

        let total_time = start.elapsed();

        cpu_session.verify().unwrap_or_else(|e| panic!("{e}"));

        let result = SuiteResult {
            execution: Some(execution),
            run_id,
            timestamp: chrono_now(),
            git_hash,
            ci_environment: ci,
            comparisons,
            total_time,
            unreliable: any_unreliable,
            gate_waits: total_gate_waits,
            gate_wait_time: total_gate_wait_time,
            timer_resolution_ns: timer_res,
            loop_overhead_ns,
            testbed,
            calibration,
            cpu_placement: Some(cpu_session.placement.clone()),
            ..Default::default()
        };

        // Publish the immutable packet before removing our transient checkpoint.
        if let Some(path) = &save_path {
            let reference = crate::storage::persist(&result)
                .unwrap_or_else(|e| panic!("cannot persist benchmark evidence: {e}"));
            eprintln!("[zenbench] evidence → {}", reference.display());
            std::fs::remove_file(path).unwrap_or_else(|e| panic!("cannot retire checkpoint: {e}"));
        }

        if let Some(path) = crate::daemon::result_path_from_env() {
            result.save(path).expect("cannot save child result packet");
        }

        // Print footer (groups were already printed as they completed)
        if !self.quiet {
            crate::report::print_footer(
                result.total_time,
                result.gate_waits,
                result.gate_wait_time,
                result.unreliable,
            );
        }

        // Lock is released when _lock drops
        result
    }
}

/// Run a comparison group with interleaved execution.
fn run_comparison_group(
    group: &mut BenchGroup,
    gate: &mut ResourceGate,
    loop_overhead_ns: f64,
    tsc_ticks_per_ns: Option<f64>,
    timer_resolution_ns: u64,
    cpu_session: &platform::CpuSession,
    suite_affinity: Option<&[usize]>,
) -> ComparisonResult {
    let environment =
        platform::configured_affinity().unwrap_or_else(|e| panic!("benchmark affinity: {e}"));
    let cores = group
        .affinity_cores
        .as_deref()
        .or(suite_affinity)
        .or(environment.as_deref());
    if let Some(cores) = cores {
        if cpu_session.placement.status == platform::PlacementStatus::EnforcedUniform
            && !cores
                .iter()
                .all(|cpu| cpu_session.placement.effective_cpus.contains(cpu))
        {
            panic!(
                "group '{}' affinity is outside the verified process CPU set",
                group.name
            );
        }
    }
    cpu_session.verify().unwrap_or_else(|e| panic!("{e}"));
    let _affinity = platform::GroupAffinityGuard::enter(cores)
        .unwrap_or_else(|e| panic!("benchmark group '{}' affinity: {e}", group.name));
    let mut group_placement = cpu_session.placement.clone();
    group_placement.thread_affinity_requested = cores.map(|cores| {
        let mut cores = cores.to_vec();
        cores.sort_unstable();
        cores.dedup();
        cores
    });
    group_placement.thread_affinity_effective = platform::current_thread_affinity().ok();
    let verify = || {
        cpu_session.verify().unwrap_or_else(|e| panic!("{e}"));
        group_placement
            .verify_thread()
            .unwrap_or_else(|e| panic!("{e}"));
    };
    let config = &group.config;
    let n_benchmarks = group.benchmarks.len();
    let group_start = Instant::now();

    gate.wait_for_no_benchmarks();
    gate.start_monitoring();

    // Phase 1: Warmup + iteration estimation
    crate::report::status(&format!("[zenbench] warming up '{}'...", group.name));

    // Explicit warmup phase: run each benchmark for warmup_time to fill
    // icache, branch predictors, and allocator free lists.
    if config.warmup_time > std::time::Duration::ZERO {
        for bench in group.benchmarks.iter_mut() {
            let warmup_start = Instant::now();
            while warmup_start.elapsed() < config.warmup_time {
                let mut b = Bencher::new(1);
                bench.func.call(&mut b);
            }
        }
    }

    // Estimate iteration count for each benchmark
    let mut estimates = Vec::with_capacity(n_benchmarks);
    let mut cold_starts = Vec::with_capacity(n_benchmarks);
    for bench in group.benchmarks.iter_mut() {
        let (est, cold_ns) = estimate_iterations(&mut bench.func, config, timer_resolution_ns);
        estimates.push(est);
        cold_starts.push(cold_ns);
    }
    // Use the LOWER MEDIAN estimate. The min is dragged down by the slowest
    // benchmark (e.g., 16-thread bench_parallel), but the upper median would
    // make slow benchmarks take minutes per sample. The lower median balances:
    // most benchmarks get reasonable precision, slow benchmarks get shorter
    // samples (acceptable — their per-iteration times are long enough).
    let mut sorted_estimates = estimates.clone();
    sorted_estimates.sort_unstable();
    let iterations_per_sample = if sorted_estimates.is_empty() {
        1
    } else {
        sorted_estimates[(sorted_estimates.len() - 1) / 2].max(1)
    };

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
    let eta_secs = sample_time_est.as_secs_f64() * config.max_rounds as f64 * n_benchmarks as f64;
    let eta_str = if eta_secs >= 60.0 {
        format!("{:.0}m{:.0}s", eta_secs / 60.0, eta_secs % 60.0)
    } else {
        format!("{:.0}s", eta_secs)
    };
    crate::report::status(&format!(
        "[zenbench] measuring '{}' (~{} iters/sample, est. {eta_str})...",
        group.name, iterations_per_sample,
    ));

    // Storage: samples[bench_idx] = vec of raw elapsed_ns per round
    let mut samples: Vec<Vec<u64>> = vec![Vec::with_capacity(config.max_rounds); n_benchmarks];
    // CPU time samples (parallel to wall time samples)
    let mut cpu_samples: Vec<Vec<u64>> = vec![Vec::with_capacity(config.max_rounds); n_benchmarks];
    let mut iters_per_round: Vec<usize> = Vec::with_capacity(config.max_rounds);

    // Allocation tracking: accumulate totals per benchmark
    #[cfg(feature = "alloc-profiling")]
    let mut alloc_totals: Vec<(u64, u64, u64, u64, u64, u64)> =
        vec![(0, 0, 0, 0, 0, 0); n_benchmarks]; // (allocs, deallocs, reallocs, bytes_alloc, bytes_dealloc, iterations)
    // Independent group/process schedules avoid repeating the same phase
    // relationship with periodic system activity. Retained rounds preserve
    // every realized permutation; bootstrap resampling stays deterministic.
    use std::hash::BuildHasher;
    let seed =
        std::collections::hash_map::RandomState::new().hash_one((std::process::id(), &group.name));
    let mut rng = Xoshiro256SS::seed(seed);

    // Cache firewall buffer
    let firewall = if config.cache_firewall {
        Some(CacheFirewall::new(config.cache_firewall_bytes))
    } else {
        None
    };

    let mut retained_samples = Vec::with_capacity(config.max_rounds);
    let mut completed_rounds = 0;
    let mut measurement_time = std::time::Duration::ZERO;

    for round in 0..config.max_rounds {
        // Hard wall-clock limit — includes gate waits. Safety net.
        if group_start.elapsed() >= config.max_wall_time {
            crate::report::clear_status(); // clear status line
            break;
        }
        // Check time limit against measurement time only (excludes gate waits).
        // Only enforce after min_rounds so slow benchmarks still get enough data.
        if round >= config.min_rounds && measurement_time >= config.max_time {
            break;
        }

        // Wall-clock limit
        let wall_remaining = config.max_wall_time.saturating_sub(group_start.elapsed());
        if round >= config.min_rounds && wall_remaining.is_zero() {
            break;
        }

        // Progress indicator — overwriting line showing round count
        if round > 0 && round % 10 == 0 {
            let elapsed = group_start.elapsed().as_secs_f64();
            crate::report::status(&format!(
                "[zenbench] '{}' round {}/{}  ({elapsed:.0}s)",
                group.name, round, config.max_rounds,
            ));
        }

        verify();

        // Record whether system is noisy (advisory, doesn't block).
        let gate_clean = gate.check_and_record_status();

        // Randomize benchmark order for this round
        let order = random_permutation(n_benchmarks, &mut rng);

        // Iteration count per round: two modes.
        //
        // Normal mode: ±20% anti-aliasing jitter (nanobench-inspired).
        // Linear sampling: sweep 0.2×–2.0× base (criterion-inspired)
        //   for OLS slope regression.
        let round_iters = if config.linear_sampling {
            // Sweep: round 0→0.2×, round 4→1.0×, round 9→2.0×, cycling every 10
            let phase = (round % 10) as f64;
            let factor = 0.2 + 1.8 * phase / 9.0;
            (iterations_per_sample as f64 * factor).max(1.0) as usize
        } else {
            let jitter = (rng.next_u64() % 41) as i64 - 20; // -20..+20
            ((iterations_per_sample as i64 + iterations_per_sample as i64 * jitter / 100).max(1))
                as usize
        };
        iters_per_round.push(round_iters);

        // Allocate bookkeeping before any benchmark timer starts. Keep the
        // declaration order and execution order distinct for paired replay.
        let mut retained = crate::results::RoundSample {
            gate_clean,
            gate_observation_age_ns: gate.observation_age_ns(),
            gate_observation: gate.last_observation(),
            iterations: round_iters,
            execution_order: order.clone(),
            elapsed_ns: vec![0; n_benchmarks],
            compensated_ns: vec![0; n_benchmarks],
        };
        let round_start = Instant::now();

        for &bench_idx in &order {
            // Cache firewall between benchmarks
            if let Some(fw) = &firewall {
                fw.spoil();
            }

            // Stack alignment jitter: shift stack by random offset before
            // each sample to defeat cache-line alignment bias.
            // The offset varies per benchmark per round.
            #[cfg(feature = "precise-timing")]
            let stack_offset = if config.stack_jitter {
                (rng.next_u64() as usize) % 4096
            } else {
                0
            };

            // Run the benchmark (with optional stack alignment jitter)
            let bench = &mut group.benchmarks[bench_idx];
            let mut bencher = Bencher::new_with_tsc(round_iters, tsc_ticks_per_ns);

            #[cfg(feature = "precise-timing")]
            {
                if stack_offset > 0 {
                    // Burn stack space via recursive trampoline, then call benchmark.
                    // The trampoline is safe — just recursive calls with padded frames.
                    let depth = (stack_offset & !0xF) / 64;
                    crate::timing::stack_jitter_call(&mut bench.func, &mut bencher, depth);
                } else {
                    bench.func.call(&mut bencher);
                }
            }

            #[cfg(not(feature = "precise-timing"))]
            bench.func.call(&mut bencher);

            // Subtract loop overhead (black_box + iteration control flow).
            // Clamp to 1ns to avoid negative/zero times.
            let overhead_total = (loop_overhead_ns * round_iters as f64) as u64;
            let compensated = bencher.elapsed_ns.saturating_sub(overhead_total).max(1);
            retained.elapsed_ns[bench_idx] = bencher.elapsed_ns;
            retained.compensated_ns[bench_idx] = compensated;
            samples[bench_idx].push(compensated);
            cpu_samples[bench_idx].push(bencher.cpu_ns);

            // Accumulate allocation stats
            #[cfg(feature = "alloc-profiling")]
            if let Some(delta) = bencher.alloc_delta {
                let t = &mut alloc_totals[bench_idx];
                t.0 += delta.allocs;
                t.1 += delta.deallocs;
                t.2 += delta.reallocs;
                t.3 += delta.bytes_allocated;
                t.4 += delta.bytes_deallocated;
                t.5 += round_iters as u64;
            }
        }

        measurement_time += round_start.elapsed();
        verify();
        retained_samples.push(retained);
        completed_rounds += 1;

        // Auto-rounds convergence check.
        //
        // For comparison groups: check paired differences against baseline.
        // A pair is "resolved" when EITHER:
        //   - Direction resolved: 95% CI on paired diff excludes zero
        //   - Equivalence established: CI width < target_precision × baseline mean
        //     (the largest plausible difference is smaller than we care about)
        //
        // For single-benchmark groups: individual precision check.
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
                // Single-benchmark group: individual precision
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

                    // (2) STABLE? For large differences (>10% of baseline),
                    // require the CI to be tight relative to the difference
                    // so the reported percentage is reproducible.
                    // For small differences (<10%), baseline-relative precision
                    // is enough — we don't need to pin down a 1% difference
                    // to ±0.1%.
                    let pct_diff = if base_mean.abs() > f64::EPSILON {
                        (diff_mean / base_mean).abs()
                    } else {
                        0.0
                    };
                    let stable = if direction_clear && pct_diff > 0.10 {
                        // Large effect: CI should be tight relative to the
                        // difference itself, so the reported % is reproducible.
                        // Don't also require baseline-relative precision —
                        // a 10× difference doesn't need 2% absolute precision.
                        let effect_precision = ci_half / diff_mean.abs();
                        effect_precision < 0.10
                    } else {
                        // Small or uncertain: need baseline-relative precision
                        // to determine if the difference is real or noise.
                        ci_pct_of_baseline < config.target_precision
                    };

                    resolved && stable
                })
            };

            if converged {
                crate::report::clear_status(); // clear status line
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

    let n_resamples = config.bootstrap_resamples;
    let noise_threshold = config.noise_threshold;

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

            if let Some(analysis) = PairedAnalysis::compute_with_config(
                &base_f64,
                &cand_f64,
                &iters_per_round,
                n_resamples,
                noise_threshold,
                timer_resolution_ns as f64,
            ) {
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
                    if let Some(analysis) = PairedAnalysis::compute_with_config(
                        &base_f64,
                        &cand_f64,
                        &iters_per_round,
                        n_resamples,
                        noise_threshold,
                        timer_resolution_ns as f64,
                    ) {
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

        // Bootstrap CI for this benchmark's mean
        let mean_ci = crate::stats::MeanCi::from_samples(&per_iter, n_resamples);

        // Compute allocation stats if profiling is active
        #[cfg(feature = "alloc-profiling")]
        let alloc_stats = {
            let t = &alloc_totals[i];
            if crate::alloc::is_active() && t.5 > 0 {
                Some(crate::alloc::AllocStats::from_totals(
                    t.0, t.1, t.2, t.3, t.4, t.5,
                ))
            } else {
                None
            }
        };

        // Slope regression: if linear_sampling is enabled, compute OLS slope
        let slope_ns = if config.linear_sampling {
            let xs: Vec<f64> = iters_per_round.iter().map(|&n| n as f64).collect();
            let ys: Vec<f64> = samples[i].iter().map(|&v| v as f64).collect();
            crate::stats::slope_estimate(&xs, &ys).map(|(slope, _r2)| slope)
        } else {
            None
        };

        // Timer ticks per sample: how many timer resolution units each sample spans.
        // Below ~50 means the measurement is quantization-limited.
        let timer_ticks = if timer_resolution_ns > 0 {
            (summary.mean * iterations_per_sample as f64) / timer_resolution_ns as f64
        } else {
            f64::INFINITY
        };

        individual_results.push(BenchmarkResult {
            name: bench.name.clone(),
            summary,
            cpu_summary,
            tags: bench.tags.clone(),
            subgroup: bench.subgroup.clone(),
            cold_start_ns: cold_starts[i] as f64,
            slope_ns,
            mean_ci,
            timer_ticks_per_sample: timer_ticks,
            #[cfg(feature = "alloc-profiling")]
            alloc_stats,
        });
    }

    ComparisonResult {
        config: Some(config.clone()),
        cpu_placement: Some(group_placement),
        group_name: group.name.clone(),
        benchmarks: individual_results,
        analyses,
        completed_rounds,
        samples: retained_samples,
        throughput: group.throughput.clone(),
        cache_firewall: config.cache_firewall,
        cache_firewall_bytes: config.cache_firewall_bytes,
        baseline_only,
        throughput_unit: group.throughput_unit.clone(),
        sort_by_speed: config.sort_by_speed,
        expect_sub_ns: config.expect_sub_ns,
        cold_start: config.cold_start,
        iterations_per_sample,
    }
}

/// Path for auto-saving results: `<target>/zenbench/results/zenbench-<run_id>.txt`
/// Checkpoints stay outside disposable Cargo build output.
fn auto_save_path(run_id: &RunId) -> std::path::PathBuf {
    let dir = crate::storage::prepare_dir()
        .expect("cannot prepare benchmark data directory")
        .join("attempts");
    std::fs::create_dir_all(&dir).expect("cannot create benchmark attempts directory");
    dir.join(format!("{run_id}.json.gz"))
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

/// Estimate iteration count for each sample.
///
/// Strategy: use the LARGER of two targets:
/// 1. **Timer precision floor**: enough iterations so each sample is ≥ 1000×
///    the timer resolution. This ensures the per-iteration time is well above
///    measurement noise. With TSC (~0.2ns), this is easy. With Instant (~25ns),
///    it requires more iterations.
/// 2. **User's sample_target_ns**: a time budget (default 1ms) that caps how
///    long each sample takes, limiting noise exposure from context switches.
///
/// On a quiet system, the precision floor dominates (short samples, many rounds).
/// On a noisy system, the time target caps sample duration (preventing unbounded
/// contamination).
///
/// Returns (iterations, cold_start_ns).
fn estimate_iterations(
    func: &mut crate::bench::BenchFn,
    config: &GroupConfig,
    timer_resolution_ns: u64,
) -> (usize, u64) {
    let mut iters = 1;
    let mut cold_start_ns = 0u64;

    for round in 0..5 {
        let mut bencher = Bencher::new(iters);
        func.call(&mut bencher);

        // Capture the very first single-iteration call as cold start
        if round == 0 && iters == 1 {
            cold_start_ns = bencher.elapsed_ns;
        }

        let elapsed = bencher.elapsed_ns.max(1_000); // Don't trust < 1µs
        let per_iter_ns = elapsed / iters as u64;
        let per_iter_ns = per_iter_ns.max(1);

        // Target 1: timer precision floor — sample should be ≥ 1000× timer resolution
        // so per-iteration time has ≥ 3 significant digits.
        let precision_target_ns = timer_resolution_ns.saturating_mul(1000).max(10_000); // at least 10µs
        let iters_for_precision = (precision_target_ns / per_iter_ns) as usize;

        // Target 2: user's sample_target_ns (default 1ms) — caps noise exposure
        let iters_for_target = (config.sample_target_ns / per_iter_ns) as usize;

        // Target 3: minimum sample duration floor — guards against the case
        // where one iteration is large enough that sample_target_ns barely
        // fits one of them. Without this, a single OS interrupt during a
        // ~1ms sample window swings the result 5–10%.
        //
        // Per-iter-aware: the floor only kicks in when sample_target_ns
        // would otherwise yield fewer than 20 iterations. Faster benches
        // that already accumulate ≥ 20 iters per sample_target are left
        // alone, so a 100ns-per-iter bench isn't dragged from 10,000 iters
        // (1 ms at default sample_target) up to 50,000 iters (5 ms at the
        // old unconditional 5 ms floor). The floor exists to protect slow
        // benchmarks from single-interrupt contamination, not to multiply
        // fast benchmarks' sample lengths 5× for no statistical gain.
        const MIN_SAMPLE_ITERS_THRESHOLD: usize = 20;
        let iters_for_min_sample = if iters_for_target >= MIN_SAMPLE_ITERS_THRESHOLD {
            0
        } else {
            (config.min_sample_ns / per_iter_ns) as usize
        };

        // Use the LARGER of the three. The 2x cap on sample_target keeps
        // fast benchmarks from running 10× the user's chosen sample length
        // just to satisfy a precision floor that's already over-satisfied.
        // The min_sample floor only applies to slow benchmarks (< 20 iters
        // per sample_target); see the per-iter-aware guard above.
        let new_iters = iters_for_precision
            .max(iters_for_target)
            .min(iters_for_target.saturating_mul(2).max(iters_for_precision))
            .max(iters_for_min_sample);

        if new_iters <= 2 * iters {
            // Converged
            return (
                new_iters
                    .max(1)
                    .clamp(config.min_iterations, config.max_iterations),
                cold_start_ns,
            );
        }
        iters = new_iters;
    }

    (
        iters
            .max(1)
            .clamp(config.min_iterations, config.max_iterations),
        cold_start_ns,
    )
}

/// Measure the per-iteration overhead of the benchmark loop.
///
/// Runs an empty loop (`for i in 0..N { black_box(i); }`) many times and
/// returns the minimum observed per-iteration cost in nanoseconds. This
/// overhead — loop control flow, `black_box` barrier, branch prediction —
/// is subtracted from all measurements so reported times reflect only the
/// user's code.
///
/// Uses minimum as the estimator: noise is additive (OS interrupts, cache
/// misses only add time), so the fastest run is closest to the true cost.
fn measure_loop_overhead() -> f64 {
    let n_samples = 200;
    let iters: usize = 10_000;
    let mut min_per_iter = f64::MAX;

    for _ in 0..n_samples {
        #[cfg(feature = "precise-timing")]
        crate::timing::compiler_fence();

        let start = Instant::now();
        for i in 0..iters {
            std::hint::black_box(i);
        }
        let elapsed_ns = start.elapsed().as_nanos() as f64;

        #[cfg(feature = "precise-timing")]
        crate::timing::compiler_fence();
        let per_iter = elapsed_ns / iters as f64;
        if per_iter < min_per_iter {
            min_per_iter = per_iter;
        }
    }

    // Clamp: overhead should be non-negative and sane (< 100ns on any platform)
    min_per_iter.clamp(0.0, 100.0)
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

#[cfg(test)]
mod shuffle_tests {
    use super::*;

    #[test]
    fn seeded_schedules_are_permutations_and_consume_fresh_randomness() {
        for n in [2, 3, 8] {
            let schedule = || {
                let mut rng = Xoshiro256SS::seed(42);
                (0..64)
                    .map(|_| random_permutation(n, &mut rng))
                    .collect::<Vec<_>>()
            };
            let orders = schedule();
            assert_eq!(
                orders,
                schedule(),
                "the fixed-seed control must be reproducible"
            );
            assert!(orders.windows(2).any(|pair| pair[0] != pair[1]));
            for mut order in orders {
                order.sort_unstable();
                assert_eq!(order, (0..n).collect::<Vec<_>>());
            }
        }
    }
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

/// Default lock directory: the per-user cache dir
/// ([`crate::paths::user_cache_dir`], e.g. `~/.cache/zenbench`), so every
/// zenbench process of a user rendezvouses there whatever its `TMPDIR`. The
/// lock file lives at `<dir>/zenbench.lock` and is managed by
/// [`crate::exclusive::Lock`].
fn default_lock_dir() -> Option<PathBuf> {
    Some(crate::paths::user_cache_dir())
}

#[cfg(test)]
mod gate_override_tests {
    use super::*;

    #[test]
    fn gate_override_disables_any_config() {
        assert!(!resolve_gate(GateConfig::strict(), true).enabled);
        assert!(resolve_gate(GateConfig::strict(), false).enabled);
    }
}
