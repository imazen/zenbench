# zenbench

[![CI](https://github.com/imazen/zenbench/actions/workflows/ci.yml/badge.svg)](https://github.com/imazen/zenbench/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/zenbench?style=for-the-badge)](https://crates.io/crates/zenbench)
[![docs.rs](https://img.shields.io/docsrs/zenbench?style=for-the-badge)](https://docs.rs/zenbench)
[![License](https://img.shields.io/crates/l/zenbench?style=for-the-badge)](LICENSE-MIT)

Interleaved microbenchmarking with paired statistics, resource gating, and AI-friendly output.

## Why another benchmark harness?

Existing Rust benchmark harnesses run benchmarks sequentially. Benchmark A
runs on a hot CPU after warmup, while benchmark B runs on an even hotter CPU
with degraded turbo boost. System load changes between runs corrupt results.

Zenbench fixes this by **interleaving**: each measurement round, all benchmarks
in a comparison group run in randomized order. Round N of benchmark A and round
N of benchmark B execute under near-identical system conditions, so paired
statistical tests detect differences that sequential harnesses miss.

## Quick start

```toml
[dev-dependencies]
zenbench = "0.1"

[[bench]]
name = "my_bench"
harness = false
```

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

```console
$ cargo bench --bench my_bench
```

## Output

```text
  sort_1000 ───────────────────────────────────────────────────
  60 rounds × 22K calls, baseline-only (4 benchmarks)
  ┌─────────────────┬───────┬───────┬──────────────────────────┬───────────────┐
  │ benchmark       │   min │  mean │           95% CI vs base │    throughput │
  ├─────────────────┼───────┼───────┼──────────────────────────┼───────────────┤
  │ std_sort        │ 251ns │ 260ns │ [ 258ns   260ns   261ns] │ 3.85 Gitems/s │
  │ sort_unstable   │ 247ns │ 256ns │ [ -1.7%   -1.3%   -1.0%] │ 3.90 Gitems/s │
  └─────────────────┴───────┴───────┴──────────────────────────┴───────────────┘

  sort_unstable  ████████████████████████████████████████████████ 3.90 Gitems/s
  std_sort       █████████████████████████████████████████████████ 3.85 Gitems/s
```

Columns:
- **min** — fastest observed run (the real floor; noise only adds time)
- **mean** — typical performance
- **mad** — median absolute deviation (robust to outliers, shown when no throughput)
- **95% CI vs base** — bootstrap confidence interval: `[lo  mid  hi]` in ns for baseline, % for comparisons
- **throughput** — when `Throughput` is set

Green = faster (CI excludes zero). Red = slower. Dim = uncertain (CI crosses zero).
Footnotes `[1]` flag issues: drift, high CV, CI crossing zero, tiny effect size.

## How measurement works

Three layers: **rounds**, **samples**, and **calls**.

A **call** is one invocation of your function. A **sample** is a timed
batch of N calls (N auto-scaled to ~10ms). A **round** is one sample from
every benchmark in a group, in shuffled order.

1. **Warmup** — estimate calls per sample
2. **Gate check** — wait for quiet system (CPU, RAM, temp)
3. **Measure** — up to 200 rounds, shuffled per round
4. **Converge** — stop early when paired-difference CI is stable
5. **Analyze** — bootstrap CI, Wilcoxon test, drift detection

Auto-rounds convergence stops measurement when the 95% CI on paired
differences excludes zero AND the effect size is stable (reproducible
across runs). No hardcoded percentage thresholds.

## Configuration

```rust,ignore
suite.compare("my_group", |group| {
    group.baseline("reference_impl");
    group.throughput(Throughput::Elements(1000));
    group.throughput_unit("items");

    group.config()
        .max_rounds(200)          // ceiling (auto-rounds may stop earlier)
        .max_time(Duration::from_secs(10))  // measurement time limit
        .max_wall_time(Duration::from_secs(120)) // hard wall-clock limit
        .cache_firewall(true)     // spoil L2 between benchmarks
        .cold_start(true)         // 1 call/sample, cache firewall on
        .sort_by_speed(true)      // table sorted fastest-first
        .baseline_only(true)      // only compare vs baseline (auto for >3)
        .target_precision(0.02);  // convergence threshold (2%)

    group.subgroup("fast path");
    group.bench("hot", |b| { /* ... */ });
    group.subgroup("slow path");
    group.bench("cold", |b| { /* ... */ });
});
```

## Threading

Three APIs for three patterns:

```rust,ignore
// 1. Contended shared state
group.bench_contended("mutex_map", 8,
    || Mutex::new(HashMap::new()),
    |b, shared, tid| {
        b.iter(|| { shared.lock().unwrap().insert(tid, 42); })
    },
);

// 2. Independent parallel scaling
group.bench_parallel("work_4t", 4, |b, _tid| {
    b.iter(|| expensive_computation())
});

// 3. Automatic scaling analysis (probes 1..physical_cores)
group.bench_scaling("work", |b, _tid| {
    b.iter(|| expensive_computation())
});
```

For **rayon/tokio** code, use regular `bench()` — wall-clock timing
captures all threads. Don't mix `bench_parallel`/`bench_contended`
with existing thread pools.

## Output formats

```console
$ cargo bench -- --format=llm     # key-value lines (greppable)
$ cargo bench -- --format=csv     # CSV with all stats
$ cargo bench -- --format=md      # Markdown tables
$ cargo bench -- --format=json    # Full JSON
```

Or via environment variable: `ZENBENCH_FORMAT=llm cargo bench`

Results auto-save to `/tmp/zenbench/zenbench-{id}.txt` in LLM format.
The path is printed before measurement starts — tools can re-read
results without re-running. Disable with `ZENBENCH_NO_SAVE=1`.

## Resource gating

Before each round, zenbench checks system health:
- CPU utilization (default: wait if >30%)
- Available RAM (default: wait if <512MB)
- CPU temperature (default: wait if >90°C)
- Heavy processes (default: wait if >3 at >10% CPU)

Not checked: disk I/O, network, frequency scaling, VM/container noise.

```rust,ignore
zenbench::run_gated(
    GateConfig::default()
        .max_cpu_load(0.10)
        .min_available_ram_mb(2048),
    |suite| { /* ... */ },
);
```

## Statistics

All comparison statistics are CI-based, not p-value thresholds:

- **Bootstrap 95% CI** — 10K resamples, percentile-based (asymmetric)
- **Significance** = CI excludes zero (no hardcoded % cutoff)
- **Cohen's d** — standardized effect size
- **Wilcoxon signed-rank** — non-parametric test
- **Spearman drift** — detects thermal throttling or load changes
- **MAD** — median absolute deviation (robust to outlier spikes)
- **IQR filtering** — Tukey's fences on paired differences

See [METHODOLOGY.md](METHODOLOGY.md) for the full research cross-reference
with Mytkowicz, STABILIZER, Chen & Revels, Kalibera & Jones, and others.

## Migrating from Criterion

### Cargo.toml

```diff
 [dev-dependencies]
-criterion = "0.5"
+zenbench = "0.1"

 [[bench]]
 name = "my_bench"
 harness = false
```

### Simple benchmark

```diff
-use criterion::{black_box, criterion_group, criterion_main, Criterion};
+use zenbench::black_box;

-fn bench_fib(c: &mut Criterion) {
-    c.bench_function("fib 20", |b| {
-        b.iter(|| fibonacci(black_box(20)))
-    });
-}
-
-criterion_group!(benches, bench_fib);
-criterion_main!(benches);
+zenbench::main!(|suite| {
+    suite.compare("fibonacci", |group| {
+        group.bench("fib 20", |b| {
+            b.iter(|| fibonacci(black_box(20)))
+        });
+    });
+});
```

`b.iter()` works the same way. `black_box` works the same way.

### Comparing functions

```diff
-fn bench_sorts(c: &mut Criterion) {
-    let mut group = c.benchmark_group("sorting");
-    group.bench_function("std_sort", |b| {
-        b.iter(|| { /* ... */ })
-    });
-    group.bench_function("sort_unstable", |b| {
-        b.iter(|| { /* ... */ })
-    });
-    group.finish();
-}
+suite.compare("sorting", |group| {
+    group.bench("std_sort", |b| {
+        b.iter(|| { /* ... */ })
+    });
+    group.bench("sort_unstable", |b| {
+        b.iter(|| { /* ... */ })
+    });
+});
```

No `finish()` needed — the closure handles scope.
Benchmarks are automatically interleaved and compared.

### Throughput

```diff
-group.throughput(Throughput::Elements(size as u64));
-group.bench_with_input(BenchmarkId::new("sum", size), &input, |b, i| {
-    b.iter(|| i.iter().sum::<u64>())
-});
+group.throughput(Throughput::Elements(size));
+group.bench("sum", |b| {
+    b.with_input(|| input.clone())
+        .run(|data| data.iter().sum::<u64>())
+});
```

`with_input().run()` separates setup from measurement — setup time
and drop cost are excluded from timing. In criterion, you'd use
`iter_batched` for this.

### What changes

| Criterion | Zenbench | Why |
|---|---|---|
| Sequential execution | Interleaved (shuffled per round) | Eliminates thermal/load bias |
| `criterion_group!` + `criterion_main!` | `zenbench::main!` | Single macro |
| `bench_function` / `bench_with_input` | `bench` + `with_input().run()` | Cleaner separation |
| `BenchmarkId` | Just a string name | Simpler |
| `group.finish()` | Automatic (closure scope) | Less boilerplate |
| Linear regression on samples | Bootstrap CI on paired diffs | More powerful for comparisons |
| `--baseline` flag | `group.baseline("name")` | In code, not CLI |
| `sample_size(N)` | `max_rounds(N)` + auto-convergence | Stops early when precise |
| Fixed sample count | Adaptive convergence | Respects your time |
| Noise threshold (1%) | CI-based (no hardcoded %) | Statistically valid |
| `measurement_time` | `max_time` + `max_wall_time` | Separate measurement vs wall |
| HTML reports | Terminal + LLM + CSV + Markdown + JSON | No gnuplot dependency |

### What you lose

- HTML reports with plots (zenbench has terminal tables and bar charts)
- `iter_batched_ref` (zenbench's `with_input().run()` always clones)
- `Criterion::default().configure_from_args()` (use `--format=X` or env vars)
- Async benchmark support (not yet implemented)
- Custom measurement types (wall-clock only for now)

### What you gain

- Interleaved execution — fairer comparisons
- Paired statistics — CI on the actual difference, not separate means
- Resource gating — waits for quiet system
- Auto-convergence — stops when results are precise, not after a fixed count
- Threading APIs — `bench_contended`, `bench_parallel`, `bench_scaling`
- Cold-start mode — `config().cold_start(true)`
- LLM-friendly output — `--format=llm` for AI-assisted workflows
- Auto-save — results in temp file, no re-running

## License

MIT OR Apache-2.0

## AI-Generated Code Notice

Developed with Claude (Anthropic). Not all code has been manually reviewed.
Review critical paths before production use.
