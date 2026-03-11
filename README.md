# zenbench

Interleaved microbenchmarking with resource gating, paired statistics, and fire-and-forget mode.

## Why another benchmark harness?

Existing Rust benchmark harnesses (criterion, divan, tango) run benchmarks sequentially.
This means benchmark A runs on a hot CPU after warmup, while benchmark B runs on an even
hotter CPU with potentially degraded turbo boost. System load changes between runs corrupt results.

Zenbench fixes this by **interleaving**: in each measurement round, all benchmarks in a
comparison group run in randomized order. Since round N of benchmark A and round N of
benchmark B execute under near-identical system conditions, paired statistical tests have
far more power to detect real differences.

## Key features

- **Interleaved execution** — randomized round-robin eliminates thermal, turbo, and load bias
- **Resource gating** — waits for CPU load, RAM, temperature, and process contention to clear
- **Cross-process coordination** — file lock prevents concurrent benchmark processes from corrupting each other
- **Paired statistics** — Welford streaming, bootstrap CI, Cohen's d, Wilcoxon signed-rank test, drift detection
- **Robust metrics** — median, MAD (scaled), and mean/variance for both parametric and non-parametric analysis
- **Anti-aliasing jitter** — varies iteration count ±20% per round to prevent timer synchronization artifacts
- **Cache firewall** — spoils CPU cache between samples to reduce cache-state bias
- **Fire-and-forget** — spawn detached benchmark processes, query progress, auto-kill stale runs
- **CI-aware** — auto-detects GitHub Actions, GitLab CI, etc. and adjusts gate thresholds
- **Cross-platform** — Linux, macOS, Windows (x64 and ARM)
- **`#![forbid(unsafe_code)]`** — no unsafe anywhere

## Quick start

Add to `Cargo.toml`:

```toml
[dev-dependencies]
zenbench = "0.1"

[[bench]]
name = "my_bench"
harness = false
```

Using the `main!` macro (recommended for `cargo bench`):

```rust,ignore
// benches/my_bench.rs
use zenbench::black_box;

zenbench::main!(|suite| {
    suite.compare("sorting", |group| {
        group.bench("std_sort", |b| {
            b.with_input(|| (0..1000).rev().collect::<Vec<i32>>())
                .run(|mut v| { v.sort(); black_box(v) })
        });
        group.bench("sort_unstable", |b| {
            b.with_input(|| (0..1000).rev().collect::<Vec<i32>>())
                .run(|mut v| { v.sort_unstable(); black_box(v) })
        });
    });
});
```

Or using `zenbench::run()` directly:

```rust,no_run
# fn main() {
zenbench::run(|suite| {
    suite.compare("sorting", |group| {
        group.bench("std_sort", |b| {
            b.with_input(|| (0..1000).rev().collect::<Vec<i32>>())
                .run(|mut v| { v.sort(); v })
        });
        group.bench("unstable_sort", |b| {
            b.with_input(|| (0..1000).rev().collect::<Vec<i32>>())
                .run(|mut v| { v.sort_unstable(); v })
        });
    });
});
# }
```

```console
$ cargo bench --bench my_bench
```

## Resource gating

Before each measurement round, zenbench checks:
- CPU utilization (default: wait if >30%)
- Available RAM (default: wait if <512MB)
- CPU temperature (default: wait if >90°C)
- Heavy processes (default: wait if >3 processes using >10% CPU)

```rust
use zenbench::GateConfig;

zenbench::run_gated(
    GateConfig::default()
        .max_cpu_load(0.10)
        .min_available_ram_mb(2048)
        .max_cpu_temp_c(Some(80.0)),
    |suite| {
        // ...
    },
);
```

## Statistics

Zenbench provides multiple layers of statistical analysis:

- **Paired differences** — per-round diffs eliminate system-state confounds
- **IQR outlier filtering** — Tukey's 1.5×IQR fences remove measurement spikes
- **Bootstrap 95% CI** — 10,000 resamples for confidence interval on mean difference
- **Cohen's d** — standardized effect size for practical significance
- **Wilcoxon signed-rank test** — non-parametric p-value, valid for non-normal distributions
- **Spearman drift detection** — flags thermal throttling or systematic load changes
- **Multiple-comparison warning** — Bonferroni correction when groups have many benchmarks

## CLI

```console
$ zenbench list                    # List all benchmark runs
$ zenbench status <run-id>         # Check a specific run
$ zenbench kill <run-id>           # Kill a running benchmark
$ zenbench kill stale              # Kill runs from old git commits
$ zenbench results latest          # Show most recent results
$ zenbench results latest --json   # Machine-readable output
$ zenbench compare a.json b.json   # Compare two result files
$ zenbench clean --max-age-hours 48
```

## Design principles

Built on lessons from criterion, divan, tango, nanobench, and the Mytkowicz "Producing
Wrong Data" paper:

1. **Interleave, don't sequence.** Same-round pairing eliminates system-state confounds.
2. **Gate, don't hope.** Check system state before measuring, not after.
3. **Pair, don't pool.** Paired statistical tests have more power than independent tests.
4. **Detect drift.** Spearman correlation flags thermal throttling or load changes.
5. **Spoil caches.** Cache firewall prevents one benchmark from warming caches for the next.
6. **Jitter iterations.** Anti-aliasing prevents synchronization with periodic system events.
7. **Coordinate.** File lock prevents concurrent benchmark processes from fighting.

## License

MIT OR Apache-2.0
