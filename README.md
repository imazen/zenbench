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
- **Paired statistics** — Welford streaming, bootstrap CI, Cohen's d, drift detection
- **Cache firewall** — spoils CPU cache between samples to reduce cache-state bias
- **Fire-and-forget** — spawn detached benchmark processes, query progress, auto-kill stale runs
- **CI-aware** — auto-detects GitHub Actions, GitLab CI, etc. and adjusts gate thresholds
- **Cross-platform** — Linux, macOS, Windows (x64 and ARM)
- **`#![forbid(unsafe_code)]`** — no unsafe anywhere

## Quick start

```toml
[dev-dependencies]
zenbench = "0.1"

[[bench]]
name = "my_bench"
harness = false
```

```rust,no_run
// benches/my_bench.rs
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
- CPU utilization (default: wait if >15%)
- Available RAM (default: wait if <512MB)
- CPU temperature (default: wait if >85°C)
- Heavy processes (default: wait if any process >10% CPU)

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

Built on lessons from criterion, divan, tango, and the Mytkowicz "Producing Wrong Data"
paper:

1. **Interleave, don't sequence.** Same-round pairing eliminates system-state confounds.
2. **Gate, don't hope.** Check system state before measuring, not after.
3. **Pair, don't pool.** Paired statistical tests have more power than independent tests.
4. **Detect drift.** Spearman correlation flags thermal throttling or load changes.
5. **Spoil caches.** Cache firewall prevents one benchmark from warming caches for the next.
6. **Coordinate.** File lock prevents concurrent benchmark processes from fighting.

## License

MIT OR Apache-2.0
