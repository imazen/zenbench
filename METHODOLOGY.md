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

**What Google Benchmark does that we don't**:
- Asymptotic complexity analysis (Big O fitting across input sizes)
- Manual timing mode (for GPU/custom hardware)
- Thread-aware benchmarking with synchronization barriers
- Custom counters (user-defined per-iteration metrics)
- Memory manager integration for allocation tracking
- PauseTiming/ResumeTiming within a benchmark

These are features worth considering for future development, particularly
complexity analysis and manual timing mode.

### Criterion.rs

Criterion uses bootstrap resampling for CI, modified Tukey's method for
outlier classification, and a noise threshold for filtering negligible
changes.

**What we took**: Bootstrap CI (10K resamples), Tukey's IQR filtering.

**Where we diverge**: Criterion keeps outliers in the analysis data and
just warns about them. We remove IQR outliers from paired differences
before computing CI. Our approach gives tighter intervals but could mask
genuinely bimodal performance (e.g., a function that's fast 90% of the
time and slow 10% due to a cold cache path). If your function has
legitimate bimodal behavior, our CI is optimistic. The raw data in JSON
output preserves all measurements including outliers.

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

## References

- Mytkowicz, Diwan, Hauswirth, Sweeney. [Producing Wrong Data Without Doing Anything Obviously Wrong](https://dl.acm.org/doi/10.1145/1508284.1508275). ASPLOS 2009.
- Curtsinger, Berger. [STABILIZER: Statistically Sound Performance Evaluation](https://people.cs.umass.edu/~emery/pubs/stabilizer-asplos13.pdf). ASPLOS 2013.
- Chen, Revels. [Robust Benchmarking in Noisy Environments](https://arxiv.org/abs/1608.04295). IEEE HPEC 2016.
- Kalibera, Jones. [Rigorous Benchmarking in Reasonable Time](https://kar.kent.ac.uk/33611/). ISMM 2013.
- Tratt. [What Metric to Use When Benchmarking?](https://tratt.net/laurie/blog/2022/what_metric_to_use_when_benchmarking.html). 2022.
- Tratt. [Minimum Times Tend to Mislead When Benchmarking](https://tratt.net/laurie/blog/2019/minimum_times_tend_to_mislead_when_benchmarking.html). 2019.
- [nanobench documentation](https://nanobench.ankerl.com/reference.html) (Ankerl).
- [Criterion.rs Analysis Process](https://bheisler.github.io/criterion.rs/book/analysis.html).
- [Google Benchmark User Guide](https://google.github.io/benchmark/user_guide.html).
