+++
title = "Methodology"
weight = 4
+++

## How interleaving works

Traditional harnesses run benchmark A for N iterations, then benchmark B for N iterations. System state (thermal, load, cache) differs between them.

Zenbench runs **one sample** of each benchmark per round, in shuffled order:

```text
Round 1: [B, A]     → A sample 1, B sample 1
Round 2: [A, B]     → A sample 2, B sample 2
Round 3: [B, A]     → A sample 3, B sample 3
...
Round 200: [A, B]   → A sample 200, B sample 200
```

Now `A[i] - B[i]` is a **paired difference** measured under identical conditions. The bootstrap CI on these paired differences is far more sensitive than comparing independently-collected means.

## Terminology

- **Call**: one invocation of your benchmark function
- **Sample**: many calls timed together (e.g., 3000 calls in 1ms)
- **Round**: one sample per benchmark in the group, shuffled order
- **Group**: benchmarks compared against each other (interleaved)

## Statistical pipeline

For each pair of benchmarks (baseline vs candidate):

1. **IQR outlier removal** — Tukey's fences (1.5×IQR) on paired differences. Removes context-switch spikes.
2. **Bootstrap 95% CI** — 10,000 resamples of the paired difference mean. Non-parametric — no normality assumption.
3. **Significance** — CI must fall entirely outside ±noise_threshold (default 1%) to be "significant."
4. **Cohen's d** — standardized effect size: `d = mean_diff / pooled_sd`. Tells you *how big* the difference is, not just whether it exists.
5. **Wilcoxon signed-rank** — non-parametric significance test. Doesn't assume normal distributions.
6. **Spearman drift** — rank correlation of round index vs difference. Detects thermal throttling (r > 0.5) or warmup effects (r < -0.5).

## Hardware-adaptive measurement

- **TSC timer**: Uses `rdtsc`/`rdtscp` on x86_64, `cntvct_el0` on aarch64. Sub-nanosecond precision. Auto-calibrated against `Instant` at startup.
- **Stack alignment jitter**: Random 0–4096 byte stack offset per sample via safe recursive trampoline. Defeats cache-line alignment bias (Mytkowicz 2009).
- **Overhead compensation**: Measures the empty benchmark loop cost (200 samples × 10K iterations, minimum). Subtracted from all measurements.
- **Precision-driven iteration count**: Scales iterations to `max(1000 × timer_precision, sample_target_ns)`. Short samples dodge context switches; many rounds enable statistical filtering.

## Resource gating

Before each round, zenbench checks for other benchmark processes (criterion, divan, zenbench). If found, it waits up to 30s — concurrent benchmarks would corrupt both sets of measurements.

General system noise (CPU load, heavy processes) is **not** gated — the IQR filter and bootstrap CIs handle it. This means benchmarks complete quickly even on busy development machines.

## Auto-convergence

Instead of running a fixed number of rounds, zenbench checks every 10 rounds:

1. **Resolved**: CI excludes zero (direction clear) OR CI width < target precision (equivalent)
2. **Stable**: effect size estimate is precise enough to reproduce

When both conditions are met for all pairs, measurement stops. This saves time on clean systems and spends more time on noisy ones.

## References

- Mytkowicz et al. — *Producing Wrong Data Without Doing Anything Obviously Wrong* (ASPLOS 2009)
- Curtsinger & Berger — *STABILIZER* (ASPLOS 2013)
- Chen & Revels — *Robust Benchmarking in Noisy Environments* (IEEE HPEC 2016)
- Kalibera & Jones — *Rigorous Benchmarking in Reasonable Time* (ISMM 2013)
- Tratt — *What Metric to Use When Benchmarking?* (2022)
