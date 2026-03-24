# Methodology

How zenbench measures, what it can and can't tell you, and why.

## What we do

Zenbench interleaves benchmark execution: in each measurement round, every
benchmark in a comparison group runs once, in shuffled order. This means
round N of benchmark A and round N of benchmark B execute under nearly
identical system conditions — same thermal state, same background load,
same memory pressure. Paired statistical tests on the per-round differences
have far more power to detect real changes than unpaired tests on
sequentially-collected data.

This design is informed by several papers and tools. Here's what we took
from each, what we chose differently, and what we can't address.

## Influences and cross-reference

### Mytkowicz et al. — "Producing Wrong Data Without Doing Anything Obviously Wrong" (ASPLOS 2009)

The paper that started the conversation. Mytkowicz showed that changing
*linking order* or *environment variable size* — things nobody thinks
about — can flip benchmark conclusions. A program linked one way is 8%
faster; linked another way, 7% slower. Same source, same compiler, same
flags.

The root cause: caches and branch predictors are sensitive to code and
data alignment. A single binary is one sample from the space of possible
memory layouts.

**What we took**: Randomized execution order per round, so no benchmark
consistently benefits from running first or last.

**What we can't do**: We don't randomize code layout, link order, or
stack alignment. That requires runtime re-randomization (see STABILIZER
below). A zenbench run measures one binary's layout. If you need
layout-independent results, run the benchmark from multiple builds or use
STABILIZER.

### Curtsinger & Berger — "STABILIZER" (ASPLOS 2013)

STABILIZER re-randomizes code, stack, and heap layout at runtime, so
repeated runs sample different layouts. The Central Limit Theorem then
applies: the distribution of means converges to normal, enabling valid
confidence intervals.

Their key finding: LLVM's -O3 vs -O2 performance difference is
indistinguishable from random noise once layout bias is eliminated.
Without randomization, a single binary's lucky alignment can make -O3
look 8% faster.

**What we took**: The insight that 25+ runs are needed for stable
distributions. Our minimum of 30 rounds before convergence checks
reflects this.

**What we can't do**: Runtime re-randomization requires instrumenting
the binary. Zenbench measures the binary you give it, layout and all.
This is a real limitation — your results include layout effects. For
most comparative benchmarks (A vs B in the same binary), layout effects
affect both sides similarly, so paired differences cancel them out.
But absolute numbers ("this function takes 260ns") include layout luck.

### Chen & Revels — "Robust Benchmarking in Noisy Environments" (2016)

Chen and Revels argue that the **minimum** is the best estimator of true
function speed because measurement noise is strictly additive — system
interrupts, cache misses from background processes, and timer jitter
only add time, never subtract it. The minimum is the observation least
contaminated by noise.

They also show that empirical timing distributions are heavy-tailed, so
mean and median are both distorted. Their strategy is implemented in
Julia's BenchmarkTools package.

**What we took**: We show `min` as a separate column — the fastest
observed run, the floor. We also auto-scale iteration counts to keep
each sample above timer resolution, matching their approach.

**Where we diverge**: We show both min and mean because they answer
different questions:

- **min**: "How fast can this function run?" — best case, closest to
  true speed. Useful for small, deterministic functions.
- **mean**: "How fast does it usually run?" — includes real-world
  variance from memory allocation, cache behavior, etc. Useful for
  capacity planning and functions with genuine performance variance.

For paired comparisons, we use bootstrap CI on the paired differences,
not the minimum. The minimum of a *difference* isn't as clean a concept
as the minimum of a single measurement.

**Caveat on min**: Tratt (2019) warns that for large benchmarks with
genuine performance variance (garbage collection, thread scheduling,
I/O), the minimum is misleading — it's an unrepeatable best case, not
a representative measurement. Our `min` column is most useful for small,
deterministic functions. If `min` and `mean` diverge significantly,
the function has real variance and `mean` is the better reference.

### Kalibera & Jones — "Rigorous Benchmarking in Reasonable Time" (ISMM 2013)

Kalibera and Jones model benchmarks as having hierarchical sources of
variation: VM invocations, iterations within invocations, and runs
within iterations. They show how to optimally distribute measurement
effort across these levels to maximize precision per unit time.

Their survey found 71% of benchmarking papers fail to report any measure
of variation.

**What we took**: We always report uncertainty — the 95% CI on paired
differences, MAD for measurement spread, and footnotes for
quality issues.

**What we don't do**: We model a single level of variation (rounds
within one process invocation). We don't capture cross-invocation
variation from ASLR, JIT warmup, or OS state. For JIT-compiled
languages this matters; for ahead-of-time compiled Rust it matters less,
but ASLR still applies. If cross-invocation variance matters to you,
run zenbench multiple times and compare results.

### Tratt — "What Metric to Use When Benchmarking?" (2022)

Tratt's key point: there is no universally correct metric. Different
questions need different statistics.

| Question | Best metric |
|---|---|
| How fast *can* this run? | Minimum |
| How fast *does* it usually run? | Median or mean |
| How much total capacity do I need? | Mean |
| Is A faster than B? | CI on paired differences |

He also demonstrates that wall-clock time and CPU instructions can move
in opposite directions (multithreaded grep: fewer wall-clock seconds,
more instructions). The metric you choose determines the conclusion.

**What we took**: We report min and mean as separate columns, and the
comparison uses bootstrap CI. The user picks what matters to them.

### nanobench (Ankerl)

nanobench adds 0-20% random noise to iteration counts to prevent
aliasing with periodic system events — timer interrupts (~1ms),
scheduler quanta (~4ms), and similar. Without this, a benchmark that
takes exactly one timer quantum per iteration will systematically
measure the timer period, not the function.

**What we took**: ±20% iteration jitter per round, same rationale.

### Google Benchmark

Google Benchmark is the most widely used C++ microbenchmark library.
Several design comparisons are worth understanding.

**Interleaving**: Google Benchmark added random interleaving as an
opt-in flag (`--benchmark_enable_random_interleaving`), reporting ~40%
variance reduction. Their approach divides iterations into chunks and
interleaves chunks from different benchmarks. In zenbench, interleaving
is the core design — always on, at the round level, with full shuffle.

**Iteration count**: Both auto-scale iterations to fill a time target.
Google Benchmark targets `min_time` (default unspecified, typically
~0.5s); zenbench targets ~10ms per sample. Our shorter target means
more rounds for the same wall time, giving better paired statistics
at the cost of more timer overhead per sample.

**Warmup**: Google Benchmark has an explicit warmup phase (disabled by
default, configurable via `MinWarmUpTime`). Zenbench runs warmup
implicitly during iteration estimation — the first few runs of each
benchmark during calibration serve as warmup.

**Statistics**: Google Benchmark reports mean, median, stddev, and CV
across repetitions. It does not compute bootstrap CI, paired tests,
effect sizes, or drift detection. Comparison between benchmarks is
done via external tooling (e.g., `compare.py`), not built in.

**DoNotOptimize vs black_box**: Google Benchmark's `DoNotOptimize`
forces a value into a register/memory and acts as a memory barrier.
Rust's `std::hint::black_box` is the equivalent — both prevent the
compiler from optimizing away benchmark code.

**What we do differently**:

| Feature | Google Benchmark | Zenbench |
|---|---|---|
| Interleaving | Opt-in, chunk-based | Always on, round-level |
| Paired statistics | No | Bootstrap CI, Wilcoxon, Cohen's d |
| Drift detection | No | Spearman correlation |
| Resource gating | No | CPU, RAM, temp, process checks |
| Auto convergence | No | CI-based adaptive stopping |
| Cache firewall | No | Opt-in L2 spoiling |
| Outlier handling | None built-in | IQR filtering on paired diffs |
| Output | Console/JSON/CSV | Console + JSON + Markdown + CSV |

**What Google Benchmark does that we don't yet**:
- Asymptotic complexity analysis (Big O fitting across input sizes)
- Manual timing mode (for GPU/custom hardware)
- Custom counters (user-defined per-iteration metrics)
- Memory manager integration for allocation tracking
- PauseTiming/ResumeTiming within a benchmark

Thread-aware benchmarking is implemented via `bench_contended()` and
`bench_parallel()`. Complexity analysis and manual timing are the most
valuable remaining gaps.

### Criterion.rs

Criterion (0.8.x) is the most widely used Rust benchmarking library.
Its statistics are the most sophisticated of the established tools, but
its execution model has fundamental limitations that interleaving
addresses.

**Statistics:**

- **Bootstrap CI**: 100K resamples using `oorandom::Rand64` (seeded from
  system time, non-reproducible). Percentile method — takes 2.5th and
  97.5th percentiles of the bootstrap distribution. CIs are computed on
  mean, median, stddev, MAD, and slope (in Linear sampling mode).
- **Slope regression**: In Linear mode, iteration counts are
  `[d, 2d, 3d, ..., 100d]`, enabling through-origin OLS regression
  `time = slope × iterations`. The slope separates per-iteration time
  from constant overhead (function call, timer, black_box). This is
  criterion's strongest statistical feature — it answers "what does each
  iteration cost?" rather than "what does each iteration plus overhead
  cost?"
- **Comparison**: Two-stage gate. First, Welch's t-test via mixed
  bootstrap (pool both samples under H0, bootstrap the t-statistic,
  compute p-value vs significance_level=0.05). Second, a ±1% noise
  threshold — both gates must pass for a "regression/improvement"
  verdict. This dual-gate design prevents both false positives (noise
  threshold) and false negatives (statistical test).
- **Two-sample bootstrap**: For relative change, uses a sqrt(N) chunking
  optimization — one resample of A paired with sqrt(N) resamples of B
  per chunk. Reduces total draws but introduces within-chunk correlation.
- **Outlier classification**: Tukey's fences with 5 categories (low
  severe, low mild, not an outlier, high mild, high severe). Mild at
  1.5×IQR, severe at 3×IQR. **Outliers are reported but NOT removed** —
  all statistics run on the full sample. This is more conservative than
  our approach but means a single extreme outlier can dominate the mean.
- **Effect size**: None. Reports percentage change with CI, but no
  standardized metric (Cohen's d or similar).
- **Drift detection**: None.
- **Distribution fitting**: None. All inference is nonparametric via
  bootstrap. KDE (Gaussian kernel, Silverman bandwidth) exists for
  plotting only.

**Execution model:**

- 3-second warmup (doubling iterations).
- Linear or Flat sampling mode, auto-selected. Linear runs 100 samples
  with iteration counts `[d, 2d, ..., 100d]` for OLS. Flat uses the
  same iteration count for all samples (loses the slope estimate).
- 5-second measurement time (default). Fixed sample count (100).
- No interleaving. Samples are collected sequentially in one batch.
- Stack alignment jitter (0.8.x): `alloca` shifts the stack by
  `i % page_size` per sample, varying cache line alignment.
- Quick mode: doubling strategy with stdev convergence check. Produces
  only 2 data points — fast but statistically weak.

**What we took**: Bootstrap CI (our 10K vs their 100K), Tukey's IQR
method, the general approach of computing CIs on differences rather than
individual benchmarks.

**Where we diverge**:

| Aspect | Criterion | Zenbench |
|---|---|---|
| Bootstrap resamples | 100K, non-reproducible | 10K, deterministic seed |
| CI method | Percentile | Percentile (same) |
| Outlier handling | Classify but keep | IQR-remove from paired diffs |
| Comparison test | Welch t-test (parametric) | Wilcoxon signed-rank (non-parametric) |
| Practical significance | ±1% noise threshold | None (CI-only) |
| Effect size | None | Cohen's d |
| Drift detection | None | Spearman rank correlation |
| Slope regression | OLS through origin (Linear mode) | None |
| Interleaving | None | Always-on shuffle |
| Resource gating | None | CPU/RAM/temp/process |
| Convergence | Fixed 100 samples | Adaptive (CI width + stability) |
| Stack jitter | alloca per sample | ±20% iteration jitter |

**Key differences explained**:

- *Outlier removal vs preservation*: Criterion keeps outliers so its
  statistics reflect everything that happened. We remove IQR outliers
  from paired differences before computing CI. Our approach gives tighter
  intervals but could mask genuinely bimodal performance (a function
  that's fast 90% of the time and slow 10% due to a cold cache path).
  The raw data in JSON output preserves all measurements including
  outliers.
- *No interleaving is criterion's biggest weakness*: If system load
  changes during measurement, earlier samples differ systematically from
  later ones. Linear mode is especially vulnerable — later samples run
  more iterations and are weighted differently in the regression.
- *Slope regression is criterion's biggest strength*: See the "Gaps"
  section below for why this matters and how we might address it.

### Divan

Divan (0.1.x) prioritizes ergonomics and low overhead over statistical
depth. It's the fastest benchmarking framework to set up and the
lightest at runtime.

**Statistics:**

Divan's statistics are minimal by design. The `StatsSet` struct has four
fields: `fastest` (min), `slowest` (max), `median`, and `mean`. That is
the entire statistical model. There is:

- No confidence intervals (no bootstrap, no t-distribution, nothing)
- No variance or standard deviation
- No outlier detection or removal
- No effect size measurement
- No regression detection between runs
- No historical comparison or baseline tracking
- No p-values or hypothesis testing of any kind
- No machine-readable output (no CSV, JSON, or HTML)

This is a deliberate tradeoff: divan runs fast, reports clean numbers,
and gets out of your way. If you need to know *whether a difference is
real*, divan can't tell you.

**Execution model:**

Adaptive tuning via timer precision. Starts at `sample_size = 1` and
doubles until each sample is ≥100× the timer's precision floor. Then
collects 100 samples at that size. No explicit warmup — the tuning
doublings serve that purpose (early small-batch runs are discarded).

**Where divan excels:**

- **Overhead compensation**: Explicitly measures and subtracts per-iteration
  loop overhead and allocation-tracking overhead. This is more principled
  than criterion's slope regression for isolating the benchmark from the
  harness.
- **DCE prevention**: Uses `asm!("")` fences (not just `compiler_fence`)
  which LLVM cannot reason through at all. Stronger than `black_box`
  alone.
- **Deferred drop**: Outputs are written to `MaybeUninit` slots during
  the timed loop; `Drop` runs only after timing ends. Prevents drop
  cost from polluting measurements.
- **Allocation profiling**: Built-in `AllocProfiler` wrapping
  `GlobalAlloc` that counts allocs/deallocs/reallocs per benchmark.
- **TSC timer**: Optional `rdtsc`/`rdtscp` with proper serialization
  barriers and automatic frequency calibration.
- **Multi-threaded benchmarks**: First-class via
  `#[divan::bench(threads = [1,2,4,8])]` with barrier-synchronized start.
- **Picosecond precision**: Internal `FineDuration` uses u128 picoseconds.

**What we could learn from divan:**

1. Overhead compensation — measuring and subtracting loop/timer overhead
   rather than assuming it's negligible.
2. Stronger DCE fences — `asm!("")` blocks prevent more optimizations
   than `black_box` alone (though this requires `unsafe` or nightly).
3. Deferred drop — excluding `Drop::drop` from timing for types with
   expensive destructors.
4. TSC support — hardware cycle counters for sub-nanosecond precision.

### tango-bench

tango-bench (0.7.x) is the only other Rust framework built around
paired measurement, but takes a radically different architectural
approach to achieve it.

**The dylib trick:**

tango loads two versions of the same benchmark into a single process
simultaneously. You compile your benchmark, save the binary with
`cargo-export`, modify your code, then run `cargo bench -- compare
<saved-binary>`. The runner loads the old binary as a shared library via
`libloading` (with ELF/PE binary patching for PIE/IAT issues). Both
versions coexist in the same address space, sharing thermal state,
frequency, scheduler timeslice, and cache hierarchy.

This same-process approach eliminates the dominant noise source: inter-run
system state variation. Both functions experience identical environmental
conditions within each sample, not just similar conditions.

**Interleaving:**

Deterministic ABAB alternation — on every sample, the function pointers
are swapped. This is not randomized (ours is). Periodic system effects
with period 2 could systematically bias results, though the authors
presumably chose deterministic alternation for reproducibility.

Three sampler strategies for iteration count per sample: Flat (constant),
Linear (sweep 1..N), and Random (default — random count each sample).
The Random sampler decorrelates cache/alignment effects from measurement
order, similar to our ±20% jitter.

**Statistics:**

Deliberately minimal. Z-test on paired differences with threshold
z ≥ 2.6 (~99% significance) AND |diff/baseline| > 0.5% practical
threshold. No bootstrap, no CIs, no effect size, no non-parametric
tests. Welford's algorithm for streaming mean/variance. Optional Tukey
IQR outlier removal (`--filter-outliers`).

**Anti-bias techniques:**

- Cache firewall (`--cache-firewall N`): reads N KiB of cache lines
  between samples to force eviction.
- Stack randomization (`--randomize-stack N`): `alloca`-based random
  stack offset per sample.
- System time bias detection: warns if kernel time > 5% of total CPU
  time (via `getrusage`).
- Warmup: `iterations/10` warmup before each sample.

**Comparison with zenbench:**

| Aspect | tango-bench | Zenbench |
|---|---|---|
| Pairing mechanism | Same-process dylib | Separate builds |
| Interleaving | Deterministic ABAB | Randomized shuffle |
| Statistics | Z-test only | Bootstrap CI + Wilcoxon + Cohen's d + Spearman |
| Confidence intervals | None | 95% bootstrap |
| Effect size | 0.5% practical threshold | Cohen's d |
| Outlier handling | IQR (opt-in) | IQR (always, on paired diffs) |
| Cache firewall | Opt-in, user-specified size | Opt-in, 2 MiB default |
| Stack randomization | alloca per sample | None |
| Iteration jitter | Random sampler (default) | ±20% per round |
| Resource gating | System time bias warning | CPU/RAM/temp/process |
| Cross-process coordination | None | File lock (fs4) |
| Machine-readable output | None | JSON/CSV/LLM/Markdown |
| Setup complexity | cargo-export + linker flags | Just `cargo bench` |

**tango's unique advantage**: Same-process comparison is strictly more
powerful than interleaving for noise reduction. Both functions share the
exact same system state, not just similar state from the same time
window. But the dylib approach requires special build steps and has
platform-specific fragility (PIE patching on Linux, IAT patching on
Windows).

**tango's key weakness**: Minimal statistics. You get "significant/not
significant" but no confidence interval on the magnitude. No way to
distinguish "1% ± 0.2%" from "1% ± 5%".

## What we compute

### Per-benchmark
- **min**: Fastest observed per-iteration time (the floor)
- **mean**: Arithmetic mean of per-iteration times across rounds
- **median**: Middle value (robust to outliers)
- **MAD**: Median absolute deviation, scaled by 1.4826 to estimate σ
  for normal distributions. Robust to outliers — one 10x spike barely
  moves it, unlike stddev which would be destroyed
- **stddev/variance**: Classical spread metrics (in JSON, not displayed
  — MAD is shown instead)

### Per-comparison (paired)
- **Bootstrap 95% CI**: 10K resamples of the paired round-by-round
  differences. Percentile-based (2.5th, 50th, 97.5th), not parametric.
  Captures asymmetry from right-skewed benchmark distributions.
- **pct_change**: Sample mean of paired differences as % of baseline
- **Cohen's d**: Standardized effect size (how many pooled-σ apart)
- **Wilcoxon signed-rank p-value**: Non-parametric significance test
- **Spearman drift correlation**: Detects thermal throttling or load
  changes over time
- **IQR outlier filtering**: Tukey's 1.5×IQR fences on paired diffs,
  applied before CI computation

### Display logic
- **Significance**: Based on whether the 95% CI excludes zero, not on
  p-value thresholds or arbitrary percentage cutoffs
- **Color**: Green = CI entirely below zero (faster), Red = CI entirely
  above zero (slower), Dim = CI crosses zero (uncertain)
- **Footnotes**: Fire for CI crossing zero, tiny effect size (d < 0.2),
  drift (Spearman r > 0.5), high CV (>20%), and sub-ns with near-zero
  variance (likely optimized away)

## What we can't check

The resource gate monitors CPU load, free RAM, CPU temperature, and
heavy processes before each measurement round. It cannot detect:

- **Disk I/O pressure**: A backup or indexing job thrashing the disk
  won't show up in CPU metrics
- **Network activity**: Background downloads or NFS traffic
- **CPU frequency scaling**: The governor may downclock mid-measurement
  due to thermal or power limits. We detect *thermal drift* via Spearman
  correlation, but not instantaneous frequency changes.
- **VM/container scheduling**: Hypervisor preemption or cgroup throttling
- **NUMA effects**: Memory placement on multi-socket systems
- **SMT interference**: Work on a sibling hyperthread sharing L1/L2

## Known limitations

1. **Single-binary layout**: Results include code/data alignment effects
   specific to this build. STABILIZER-level randomization isn't feasible
   without runtime instrumentation.

2. **Single-process variation**: We don't capture cross-invocation
   variance from ASLR, kernel state, or process startup effects.

3. **IQR filtering removes outliers**: Bimodal performance
   (fast path vs slow path) will have the slow-path measurements
   filtered. The CI reflects the fast-path distribution. Check the JSON
   output if you suspect bimodal behavior.

4. **Hot-loop bias**: When calls/sample > 1, the CPU's branch predictor
   and instruction cache warm up. Results reflect best-case pipeline
   state, not cold-call performance. The methodology line in output
   shows the call count so you know what you're getting.

5. **No instruction-level measurement**: We measure wall-clock time, not
   CPU instructions. As Tratt showed, these can diverge for parallel or
   I/O-heavy code.

## Future work: pause/resume for I/O isolation

The `with_input().run()` API excludes setup and teardown from timing,
but there's no way to exclude work *within* the timed region. If your
benchmark reads a file, processes it, and writes output, you currently
time all three. Google Benchmark's `PauseTiming()`/`ResumeTiming()` lets
you bracket the part you care about.

The problem: timer calls have their own overhead (~20-50ns each). For a
microsecond-scale benchmark, two pause/resume calls per iteration add
measurable bias. Google Benchmark documents this caveat.

A possible design for zenbench:

```rust,ignore
b.iter(|| {
    let data = load_file();       // not timed
    b.resume();
    let result = process(&data);  // timed
    b.pause();
    write_output(&result);        // not timed
    result
});
```

This requires `Bencher` to carry timer state and accumulate only the
resumed intervals. The overhead of two `Instant::now()` calls per
pause/resume is unavoidable — document it and let users decide.

An alternative: `with_input` already handles the common case (expensive
setup, cheap teardown). For the "I/O sandwich" pattern, users can
restructure:

```rust,ignore
b.with_input(|| load_file())
    .run(|data| process(&data))
// output is dropped outside timing
```

This doesn't cover mid-benchmark I/O exclusion but handles most cases
without timer overhead.

## Cold-start measurement

### What we capture now

During warmup, the very first single-iteration call for each benchmark
is recorded as `cold_start_ns` in the JSON output. This is the coldest
measurement we can get without process isolation — caches are cold,
branch predictors haven't learned the function's patterns, TLBs aren't
populated.

It's not a *perfect* cold start: the binary is already loaded, page
tables are set up, and earlier benchmarks in the group may have warmed
shared caches. But it's the coldest data point available for free.

### When cold start matters

- **CLI tools**: First invocation performance is what users feel
- **Serverless**: Lambda cold starts dominate tail latency
- **Request handlers**: First request after deploy
- **Infrequent code paths**: Error handlers, config parsers

For these use cases, the hot-loop mean is misleading — nobody runs
your CLI tool 10,000 times in a tight loop.

### Future: dedicated cold-start mode

True cold-start measurement would require:

1. **Process isolation**: Run each sample in a fresh subprocess so
   nothing carries over — no warm caches, no trained branch predictors,
   no populated TLBs. High overhead (process spawn per sample) but
   the only way to get a real cold start.

2. **Cache firewall per sample**: Less extreme than process isolation.
   Spoil L2/L3 between samples within the same process. Combined with
   `iterations = 1`, this gives near-cold-start conditions without
   subprocess overhead. Already partially supported via
   `config().cache_firewall(true)`.

3. **Separate reporting**: Cold-start results should be reported
   alongside hot-loop results, not mixed. A table might show:
   ```
   │ benchmark │ cold │  min │  mean │
   │ parse_cfg │ 45µs │ 12µs │ 14µs  │  ← cold is 3x hot
   ```

The `cold_start_ns` field in JSON output is the first step. A proper
`config().cold_start(true)` mode that runs 1 iteration per sample with
cache firewall and reports separately is future work.

## Multithreaded benchmarking

Three patterns, three APIs.

### Pattern 1: Contended shared state — `bench_contended()`

"How fast is this `Mutex<HashMap>` with 8 threads hammering it?"

```rust,ignore
group.bench_contended("mutex_map", 8,
    || Mutex::new(HashMap::new()),       // setup: fresh state each sample
    |b, shared, tid| {
        b.iter(|| { shared.lock().unwrap().insert(tid, 42); })
    },
);
```

All threads barrier-synchronize before starting. Wall-clock time from
barrier release to all threads completing. Thread creation/joining is
excluded. Setup runs fresh each sample so lock state doesn't carry over.

### Pattern 2: Independent parallel scaling — `bench_parallel()`

"Does this work scale linearly with threads, or am I hitting
memory bandwidth / cache contention / SMT limits?"

```rust,ignore
for threads in [1, 2, 4, 8] {
    group.bench_parallel(format!("{threads}t"), threads, |b, _tid| {
        b.iter(|| expensive_pure_computation())
    });
}
```

Same as `bench_contended` but no shared state. Each thread works
independently. If 4 threads aren't 4x the throughput of 1 thread,
you're hitting a scaling wall.

### Pattern 3: Existing thread pools (rayon, tokio) — `bench()`

"How fast is `par_sort()` / parallel pipeline / async handler?"

```rust,ignore
group.bench("par_sort", |b| {
    b.with_input(|| (0..10_000).rev().collect::<Vec<i32>>())
        .run(|mut v| { v.par_sort(); v })
});
```

Just use regular `bench()`. Wall-clock timing already captures all
threads' work. The thread pool persists across samples — this is
realistic (your production rayon pool is warm too).

**Do not** use `bench_parallel` or `bench_contended` for rayon/tokio
code. Those APIs spawn their own threads, which compete with the
existing pool for cores. The result is artificial contention that
doesn't reflect production.

### Rayon-specific guidance

- **Thread pool lifetime**: Rayon's global pool initializes on first
  use and persists for the process. This means the first sample pays
  pool startup cost; subsequent samples reuse warm threads. This is
  realistic — most production code uses a long-lived pool.

- **Pool size**: Rayon defaults to `num_cpus` threads. If you're
  benchmarking within a comparison group, all benchmarks share the
  same pool size. To compare different pool sizes, configure rayon's
  `ThreadPoolBuilder` in each benchmark's setup.

- **cpu-time feature**: Thread-local CPU time only measures the
  calling thread. For rayon benchmarks this severely undercounts
  actual CPU usage. Use wall-clock time (the default) for parallel
  workloads.

- **Interaction with resource gating**: Rayon's threads show up as
  CPU load in the gate check. The gate doesn't know they're part of
  the benchmark, not background noise. For heavily parallel benchmarks,
  consider `GateConfig::disabled()` or raising `max_cpu_load`.

### Design notes

- **Barrier-synchronized start**: Both `bench_contended` and
  `bench_parallel` use `Barrier::new(threads + 1)` — the +1 is the
  timing thread. All worker threads wait at the barrier, then the main
  thread starts the timer and waits at the second barrier for completion.

- **Interleaving works**: Threaded benchmarks are just `BenchFn`
  closures. They run in their slot during the round shuffle like any
  other benchmark. Threads are created and destroyed per sample.

- **Thread creation overhead**: Each sample spawns and joins `N`
  threads. This is excluded from timing (outside the barriers) but
  adds wall-clock overhead between samples. For benchmarks where thread
  creation cost matters, use rayon's persistent pool instead.

### Future: automatic scaling analysis — `bench_scaling()`

The `bench_parallel` loop (`for threads in [1, 2, 4] { ... }`) is
boilerplate. Zenbench should handle this automatically:

```rust,ignore
group.bench_scaling("sqrt_work", |b, _tid| {
    b.iter(|| expensive_computation())
});
// Automatically probes 1, 2, 4, ..., physical_cores, logical_cores
```

What it should report:

```
│ threads │     throughput │ scaling │ efficiency │
│       1 │ 2.49 Gitems/s │   1.00x │       100% │
│       2 │ 4.80 Gitems/s │   1.93x │        96% │  ← near-linear
│       4 │ 7.10 Gitems/s │   2.85x │        71% │  ← diminishing
│       8 │ 7.20 Gitems/s │   2.89x │        36% │  ← wasting cores
│      16 │ 6.90 Gitems/s │   2.77x │        17% │  ← SMT hurts
```

Where:
- **scaling** = throughput_N / throughput_1
- **efficiency** = scaling / N × 100% (perfect = 100%)
- Detect physical vs logical cores (SMT/HT) via `sysinfo`
- Flag the sweet spot: best throughput-per-core
- Flag when adding threads *hurts* (SMT contention)
- Consider "CPU time waste" tolerance: 71% efficiency means 29%
  of CPU time is coordination overhead, not useful work

Design considerations:
- Thread counts should include powers of 2 up to logical cores,
  plus the physical core count if it's not a power of 2
- Each thread count is a separate benchmark in the comparison group
  so they get interleaved and compared properly
- The single-thread run is the baseline for scaling metrics
- Throughput must be set on the group for scaling to be meaningful
- The gate needs thread-count awareness so it doesn't flag the
  benchmark's own threads as "heavy processes"

### Other future work

- **Resource gating thread awareness**: The gate should know a
  benchmark's thread count so it doesn't flag the benchmark's own
  threads as "heavy processes."
- **Process-level CPU time**: Aggregate CPU time across all threads
  for efficiency analysis (CPU-seconds per wall-second).

## Gaps and future statistical work

A systematic comparison against criterion, divan, and tango-bench
reveals concrete methodology gaps in zenbench. Ordered by impact.

### Statistical gaps

#### ✅ Overhead compensation — DONE

`measure_loop_overhead()` runs 200 samples of 10K iterations of an
empty `for i in 0..N { black_box(i); }` loop, takes the minimum as
the per-iteration harness cost. Subtracted from all measurements.
Stored in `SuiteResult.loop_overhead_ns` for transparency.

#### ✅ Practical significance gate — DONE

`GroupConfig::noise_threshold` (default 1%). Significance requires the
95% CI to fall entirely outside ±threshold of baseline. Prevents
"statistically significant but unmeasurably small" reports.

#### ✅ Per-benchmark confidence intervals — DONE

`MeanCi` struct: bootstrap 95% CI on each benchmark's mean, not just
paired differences. Stored in `BenchmarkResult.mean_ci`. Reported in
JSON, LLM, CSV output.

#### ✅ Configurable bootstrap resamples — DONE

`GroupConfig::bootstrap_resamples` (default 10K, minimum 100). Passed
through to both `PairedAnalysis` and per-benchmark `MeanCi`.

#### 1. Slope regression (MEDIUM — deferred)

**What criterion does**: Linear sampling with OLS regression separates
per-iteration cost from constant overhead. Most impactful for sub-100ns
benchmarks.

**Our mitigation**: Overhead compensation subtracts the loop+black_box
cost directly. Precision-driven iteration estimation ensures enough
iterations to amortize timer overhead. TSC timer reduces per-call cost.
These three measures together address the same root problem that slope
regression solves — the remaining gap is for benchmarks where the
function call overhead itself (not loop/black_box) is significant
relative to the measured time. For most practical benchmarks (> 10ns),
the difference is negligible.

**Status**: Not planned for 0.1. Overhead compensation + TSC + precision
iteration estimation provide equivalent accuracy for > 10ns benchmarks.
Slope regression would improve sub-10ns measurements.

### Methodology gaps

#### ✅ TSC / hardware timer — DONE

`precise-timing` feature (default on). `rdtsc`/`rdtscp` on x86_64 with
`lfence` serialization. `cntvct_el0` on aarch64 with `isb` barriers.
Auto-calibration against `Instant` (convergence < 0.1%). Automatic
fallback when TSC is non-invariant.

#### ✅ Stack alignment jitter — DONE

Safe recursive trampoline: `stack_jitter_call(func, bencher, depth)`.
Each level adds a 64-byte `black_box`ed stack frame. Random 0..4096
byte offset per sample per benchmark. On by default with
`precise-timing`. Zero unsafe code.

#### ✅ Deferred drop — DONE

`Bencher::iter_deferred_drop()`: collects outputs in pre-allocated
`Vec<O>` during the timed loop, drops after timing ends. `black_box`
on the slice prevents LLVM from eliding writes.

#### ✅ asm fences — DONE

`asm!("")` barriers around all timing windows. Stronger than
`compiler_fence(SeqCst)` — LLVM cannot reason through inline assembly.
Applied in `iter()`, `iter_deferred_drop()`, `InputBencher::run()`,
and `measure_loop_overhead()`.

#### ✅ Allocation profiling — DONE

`AllocProfiler` wrapping any `GlobalAlloc`. Thread-local `Cell<u64>`
counters. Reports `allocs/iter`, `bytes/iter`, `reallocs/iter` in
LLM, CSV, JSON output.

#### 2. Explicit warmup phase (LOW)

Rust is AOT-compiled (no JIT). Iteration estimation already runs each
benchmark 5+ times, warming caches and branch predictors. `with_input()`
separates setup from measurement. A time-based warmup phase would help
benchmarks with large working sets (filesystem caches, database
connections) but these are uncommon in microbenchmarking.

**Status**: Low priority. `warmup_time` in `GroupConfig` is the planned
API if needed.

#### 3. Precision-driven iteration estimation — DONE (post-original-doc)

Added after the initial gap analysis. Scales iteration count to
`max(1000 × timer_resolution / per_iter_time, sample_target_ns / per_iter_time)`.
Produces shortest possible samples that are still precise. Default
`sample_target_ns` = 1ms caps noise exposure.

### What we already do that others don't

For completeness — areas where zenbench leads:

- **Auto-convergence**: Unique. No competitor adapts measurement
  duration to statistical certainty. Saves time on clean systems,
  spends more on noisy ones.
- **Resource gating**: Unique. CPU load, temperature, RAM, heavy
  process monitoring before each round.
- **Drift detection**: Unique. Spearman rank correlation catches
  thermal throttling that sequential frameworks miss entirely.
- **Non-parametric testing**: Wilcoxon signed-rank is more robust than
  criterion's Welch t-test or tango's Z-test. No normality assumption.
- **Cohen's d**: Only framework with a standardized effect size metric.
- **Process coordination**: File lock prevents concurrent benchmarks
  from corrupting each other's measurements.
- **Interleaving + rich statistics**: tango interleaves but has minimal
  stats. Criterion has rich stats but no interleaving. Zenbench has both.

## Baseline persistence and CI regression testing

### The problem

CI performance regression testing requires comparing the current
commit's performance against a known-good reference. This means:

1. Storing benchmark results durably (not in `/tmp/`)
2. Identifying results by commit, branch, or named tag
3. Comparing new results against the stored baseline
4. Alerting when performance degrades beyond a threshold
5. Updating the baseline when changes are intentional

zenbench already has the statistical machinery (PairedAnalysis, bootstrap
CI, significance detection) and serialization (SuiteResult JSON). What's
missing is the storage, identity, and workflow layer.

### Design: named baselines

**Storage location**: `.zenbench/baselines/` in the project directory.
Each baseline is a `SuiteResult` JSON file named by its identifier:

```
.zenbench/baselines/
  main.json           # latest results from main branch
  v0.3.0.json         # release baseline
  prod.json           # named tag for production reference
```

These are regular files that can be committed to git (for reproducibility
and review) or gitignored (for large suites).

**CLI commands**:

```bash
# Save current results as a named baseline
cargo bench -- --save-baseline main

# Compare against a saved baseline
cargo bench -- --baseline main

# Update baseline only if no regressions
cargo bench -- --baseline main --update-on-pass

# List saved baselines
zenbench baseline list

# Delete a baseline
zenbench baseline delete old-baseline
```

**Behavior of `--baseline`**:

When `--baseline main` is specified, zenbench:
1. Runs the full benchmark suite (interleaved, converging)
2. Loads `.zenbench/baselines/main.json`
3. For each benchmark present in both, computes PairedAnalysis between
   the baseline's per-round means and the new run's per-round means
4. Reports using the same CI / significance / Cohen's d framework
5. Exits with nonzero status if any significant regressions are detected

**Threshold configuration**:

```toml
# .zenbench/config.toml (future)
[thresholds]
noise_threshold = 0.01        # ±1% noise gate (suppress tiny changes)
max_regression_pct = 5.0      # fail CI if any benchmark regresses >5%
significance_level = 0.95     # CI confidence level
```

Thresholds can also be set per-group in code:

```rust
group.config().noise_threshold(0.02);     // 2% for this group
group.config().max_regression_pct(10.0);  // 10% for noisy benchmarks
```

### Comparison against saved baselines vs. interleaved comparison

Saved baseline comparison is fundamentally weaker than interleaved
comparison. The baseline and candidate ran at different times, on
potentially different hardware, under different system conditions. The
paired-difference statistical framework assumes the two measurements
experienced similar conditions — which is true within an interleaved
run but NOT across runs.

**Mitigation strategies:**

1. **Wider CIs for cross-run comparisons**: Apply an inflation factor
   to the CI based on estimated cross-run variance. Store the baseline's
   internal variance alongside its means — use it to widen the
   comparison CI appropriately.
2. **Require more evidence**: Use a stricter significance threshold
   (99% instead of 95%) or larger noise threshold for cross-run
   comparisons.
3. **Calibration workloads**: See "Cross-machine comparability" below.
4. **Prefer self-compare for PRs**: Use `self-compare` (interleaved,
   same machine, same run) for PR checks. Use saved baselines only for
   tracking trends on main.

### CI workflow recommendations

**PR checks** (blocking, high confidence):

```yaml
# Build both versions, interleave on same runner
- run: cargo bench -- self-compare --ref origin/main --format=json
  # Exits nonzero if significant regressions detected
```

This is the gold standard — interleaved, paired, same hardware, same
thermal state. The only overhead is building twice.

**Main branch tracking** (non-blocking, trend analysis):

```yaml
# Run benchmarks, save as baseline tagged by commit
- run: cargo bench -- --save-baseline "commit-$(git rev-parse --short HEAD)"
  # Also update the "main" baseline for PR comparisons
- run: cp .zenbench/baselines/commit-*.json .zenbench/baselines/main.json
```

Over time this builds a time series suitable for change point detection
(see below).

**Release gates** (blocking, named baseline):

```yaml
- run: cargo bench -- --baseline release-v0.3.0 --max-regression-pct 3
```

### Implementation status — DONE

All core baseline features are implemented:

1. ✅ `--save-baseline=<name>`: Saves to `.zenbench/baselines/<name>.json`
2. ✅ `--baseline=<name>`: Load, compare, exit 0/1/2
3. ✅ `--update-on-pass`: Auto-ratchets baseline on clean runs
4. ✅ `--max-regression=<pct>`: Configurable threshold (default 5%)
5. ✅ `zenbench baseline list/show/delete`: CLI management
6. ✅ Works with both `main!` and `criterion_main!` macros

See REGRESSION-TESTING.md for complete user stories and CI workflows.

### Cross-run statistical adjustment

When comparing against a saved baseline (not interleaved), the paired
assumptions break down. The two runs experienced different system
states, so the difference distribution has higher variance than an
interleaved comparison would measure.

**Approach: variance inflation**

Store the baseline's per-benchmark variance (from its original
interleaved run). When comparing cross-run:

1. Compute the new run's per-benchmark variance
2. Estimate cross-run variance as `max(var_baseline, var_new)` plus
   a configurable additive term (the "cross-run noise floor")
3. Widen the CI by the ratio `sqrt(cross_run_var / interleaved_var)`
4. Apply stricter significance threshold (99% CI instead of 95%)

This is conservative — it reduces false positives from hardware
fluctuations at the cost of missing small regressions. For catching
small regressions, self-compare (interleaved) is the right tool.

## Cross-machine comparability

### The fundamental problem

CI runners are not stable hardware. GitHub Actions runners use shared
VMs with variable CPU allocation, noisy neighbors, and no guarantees
about CPU model or frequency. Even self-hosted runners may be
heterogeneous. A benchmark that takes 100ns on one runner might take
130ns on another, and neither number is wrong.

This means absolute times from different machines cannot be directly
compared. But CI regression testing requires *some* form of
cross-machine comparison, at least for trend tracking.

### Approach 1: Relative benchmarking (PRIMARY — already implemented)

The strongest approach: don't compare across machines. Run both old
and new code on the same machine in the same CI job. The ratio
(new/old) cancels out hardware differences.

zenbench's `self-compare` already implements this via git worktrees.
For PR regression testing, this is the recommended approach.

**Limitation**: Requires building two versions, roughly doubling CI
time. Cannot track absolute performance trends over time (only
relative to the comparison point).

### Approach 2: Calibration workloads (MEDIUM priority)

Run a small set of known reference benchmarks alongside real
benchmarks. Normalize real benchmark times by dividing by the
corresponding calibration time from the same run.

**Design:**

```rust
// Built-in calibration suite
suite.calibrate(|cal| {
    cal.integer_throughput();    // tight loop: adds, multiplies, branches
    cal.memory_bandwidth();     // sequential memcpy, 1 MiB
    cal.memory_latency();       // pointer-chasing, 4 MiB (L3-bound)
    cal.branch_heavy();         // unpredictable branches
});
```

The calibration runs before real benchmarks (excluded from gate waits
since it's measuring the machine, not user code). Results are stored
alongside the SuiteResult:

```json
{
  "calibration": {
    "integer_throughput_ns": 0.42,
    "memory_bandwidth_gbps": 18.5,
    "memory_latency_ns": 12.3,
    "branch_heavy_ns": 8.7,
    "cpu_model": "AMD EPYC 7763",
    "logical_cores": 4
  }
}
```

**Normalization**: For each real benchmark, divide its mean time by
the most relevant calibration metric. CPU-bound benchmarks normalize
by `integer_throughput`; memory-bound by `memory_bandwidth`. The user
tags each benchmark group with its dominant bottleneck, or zenbench
auto-detects based on IPC heuristics (future work).

**Pros**: Simple, fast (~100ms for calibration), gives approximate
hardware-normalized scores. Useful for trend tracking.

**Cons**: Different benchmarks scale differently across hardware. A
single calibration factor is approximate. A CPU-bound calibration
won't normalize a memory-bound benchmark correctly. Multiple
calibration workloads help but don't eliminate the problem.

### Approach 3: Testbed separation (MEDIUM priority)

Tag each result with a hardware fingerprint: CPU model, cache sizes,
core count, OS. Compare only within the same testbed. When hardware
changes, start a new baseline.

**Design:**

```json
{
  "testbed": {
    "cpu_model": "AMD EPYC 7763",
    "cpu_family": 25,
    "cache_l1d_kb": 32,
    "cache_l2_kb": 512,
    "cache_l3_kb": 32768,
    "cores_physical": 2,
    "cores_logical": 4,
    "os": "linux",
    "arch": "x86_64"
  }
}
```

Baseline comparisons refuse to compare across testbeds unless
`--cross-testbed` is explicitly passed (which widens CIs per the
variance inflation approach above).

This is Bencher's approach. It's simple, correct, and conservative.
The cost is losing historical comparison across hardware transitions.

### Approach 4: Change point detection (FUTURE)

For long-running time series (nightly benchmarks on main), use change
point detection (E-Divisive algorithm) instead of point-to-point
threshold comparison. CPD analyzes the entire history and detects
*persistent shifts* in the distribution.

**Why this matters for cross-machine**: When hardware changes, CPD
produces a single change point. Threshold-based detection would fire
on every benchmark. CPD handles it gracefully — one notification
about the hardware transition, then it re-baselines automatically.

**Design**: Store benchmark results as a time series (one point per
commit on main). Run CPD as a post-processing step. Alert when a
change point is detected that doesn't correspond to a known hardware
transition.

**Implementation**: Apache Otava's windowed t-test approach (not full
E-Divisive permutation test) is the practical choice. The windowed
variant is O(1) per new data point and needs ~30 historical points to
be reliable.

This is complementary to self-compare: self-compare catches
regressions at PR time (blocking), CPD catches gradual drifts on main
(advisory).

### Approach 5: Instruction counting (OPTIONAL)

Use Valgrind's Cachegrind/Callgrind to count executed instructions
instead of measuring wall time. Instruction counts are nearly
hardware-independent (< 0.001% variance across runs).

**Integration**: This would be a separate mode, not a replacement for
wall-clock benchmarking. Something like:

```bash
cargo bench -- --mode=cachegrind --bench my_bench
```

**Caveats**:
- **Not all instructions cost the same.** Code that replaces 1000
  scalar ops with 125 AVX2 ops looks *slower* by instruction count
  despite being faster in wall time.
- **Cache simulation is outdated.** Cachegrind's model doesn't match
  modern CPUs (no prefetching, no OoO, simplified associativity).
- **~20-50× slowdown.** Only practical for a subset of benchmarks.
- **Linux only.** Valgrind doesn't run on Windows or macOS ARM.
- **Not perfectly deterministic.** CodSpeed discovered that different
  CPU models cause glibc's malloc to detect different CPU features,
  changing code paths and producing different instruction counts for
  the same binary.
- **Cannot measure threading.** Valgrind serializes threads.

This is useful as a *complementary signal* alongside wall-clock
measurements, not as a replacement. iai-callgrind already does this
well — integration or interop with iai-callgrind may be more practical
than reimplementing.

### Approach comparison

| Approach | Cross-machine? | Accuracy | CI time | Complexity |
|---|---|---|---|---|
| Relative (self-compare) | N/A (same machine) | Excellent | 2× build | Already built |
| Calibration workloads | Approximate | Good for similar HW | +100ms | Medium |
| Testbed separation | No (avoids the problem) | Exact within testbed | None | Low |
| Change point detection | Handles transitions | Good for trends | Post-processing | High |
| Instruction counting | Yes | Misses SIMD/cache | 20-50× | Medium-High |

### Remaining implementation priorities

**Phase 1 — DONE:**
- ✅ Named baseline save/load, exit codes, `--update-on-pass`
- ✅ Noise threshold gate
- ✅ Overhead compensation, TSC timer, asm fences, deferred drop
- ✅ Stack alignment jitter, per-benchmark CIs, configurable resamples
- ✅ Allocation profiling, precision-driven iteration estimation

**Phase 2 — DONE:**
- ✅ Cross-run variance inflation (pooled t-test gating)
- ✅ Hardware fingerprint (Testbed struct in SuiteResult)
- ✅ Testbed comparison guards (warn on CPU/platform change)
- ✅ Calibration workloads (integer, memory BW, memory latency)
- ✅ Slope regression (OLS, linear sampling mode)
- ✅ Warmup phase (GroupConfig::warmup_time now functional)
- ✅ Async support (iter_async with tokio block_on)
- ✅ Criterion compat config forwarding (sample_size, measurement_time, etc.)

**Phase 3 — Time series and change point detection (FUTURE)**:

Design (not yet implemented):

**Storage**: Append each run's per-benchmark means to
`.zenbench/history/<group>__<bench>.csv`:
```csv
timestamp,git_hash,mean_ns,variance,n
2026-03-24T12:00:00Z,abc1234,245.3,12.5,200
2026-03-24T13:00:00Z,def5678,312.4,15.2,200
```

**Algorithm**: Windowed t-test (Apache Otava approach):
- Window size: 30 data points (configurable)
- For each new data point: split history at that point, compare
  the 30-point window before vs the 30-point window after
- Welch's t-test on the two windows; if t > 3.0 AND the shift
  persists for 3+ consecutive commits → flag as change point
- O(1) per new data point via running mean/variance

**CLI**: `zenbench history analyze`
- Scans all CSV files in `.zenbench/history/`
- Reports change points with commit ranges and magnitude
- Advisory only — doesn't block CI

**Integration**:
- `postprocess_result()` auto-appends to history CSV after each run
- `zenbench history analyze` is the query tool
- Complements `--baseline` (blocking point comparison) with
  time-series trend detection (advisory drift detection)

**Estimated scope**: ~400 lines (CSV append, windowed t-test, CLI)

**Phase 4 — Instruction counting integration (FUTURE, OPTIONAL)**:
- `--mode=cachegrind` for hardware-independent metrics
- Interop with iai-callgrind output format

## References

- Mytkowicz, Diwan, Hauswirth, Sweeney. [Producing Wrong Data Without Doing Anything Obviously Wrong](https://dl.acm.org/doi/10.1145/1508284.1508275). ASPLOS 2009.
- Curtsinger, Berger. [STABILIZER: Statistically Sound Performance Evaluation](https://people.cs.umass.edu/~emery/pubs/stabilizer-asplos13.pdf). ASPLOS 2013.
- Chen, Revels. [Robust Benchmarking in Noisy Environments](https://arxiv.org/abs/1608.04295). IEEE HPEC 2016.
- Kalibera, Jones. [Rigorous Benchmarking in Reasonable Time](https://kar.kent.ac.uk/33611/). ISMM 2013.
- Tratt. [What Metric to Use When Benchmarking?](https://tratt.net/laurie/blog/2022/what_metric_to_use_when_benchmarking.html). 2022.
- Tratt. [Minimum Times Tend to Mislead When Benchmarking](https://tratt.net/laurie/blog/2019/minimum_times_tend_to_mislead_when_benchmarking.html). 2019.
- [nanobench documentation](https://nanobench.ankerl.com/reference.html) (Ankerl).
- [Criterion.rs Analysis Process](https://bheisler.github.io/criterion.rs/book/analysis.html).
- [Divan announcement and design philosophy](https://nikolaivazquez.com/blog/divan/).
- [tango-bench: paired benchmarking via dylib loading](https://github.com/bazhenov/tango).
- [Google Benchmark User Guide](https://google.github.io/benchmark/user_guide.html).
- [Apache Otava: Change Point Detection for Performance Regressions](https://otava.apache.org/docs/math/). 8 years of optimization from MongoDB/DataStax.
- [CodSpeed: Why glibc is faster on some GitHub Actions Runners](https://codspeed.io/blog/unrelated-benchmark-regression). Cross-machine instruction count pitfalls.
- [iai-callgrind: Hardware-agnostic Rust benchmarking via Cachegrind](https://github.com/iai-callgrind/iai-callgrind).
- [Bencher: Continuous Benchmarking](https://bencher.dev/docs/explanation/continuous-benchmarking/).
- [Aakinshin: Performance Stability of GitHub Actions](https://aakinshin.net/posts/github-actions-perf-stability/). Measured up to 3× variance.
- [rustc-perf: Instruction counts as primary metric](https://kobzol.github.io/rust/rustc/2023/09/23/rustc-runtime-benchmarks.html). SQLite also uses this approach.
