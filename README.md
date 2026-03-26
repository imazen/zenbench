# zenbench

Interleaved microbenchmarking for Rust with paired statistics, CI regression testing, and hardware-adaptive measurement.

[![CI](https://img.shields.io/github/actions/workflow/status/imazen/zenbench/ci.yml?style=for-the-badge)](https://github.com/imazen/zenbench/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/zenbench?style=for-the-badge)](https://crates.io/crates/zenbench)
[![docs.rs](https://img.shields.io/docsrs/zenbench?style=for-the-badge)](https://docs.rs/zenbench)
[![License](https://img.shields.io/crates/l/zenbench?style=for-the-badge)](LICENSE-MIT)

**[Documentation](https://imazen.github.io/zenbench)** · **[Example HTML Report](https://imazen.github.io/zenbench/example-report.html)** · **[Tutorial](https://imazen.github.io/zenbench/getting-started/)**

```text
  compress_64k  200 rounds × 67 calls
                       mean ±mad µs  95% CI vs base          iB/s
  ├─ sequential
  │  ├─ level_1        16.2 ±0.5µs  [15.8–16.6]µs          3.78G
  │  ├─ level_6        15.1 ±0.5µs  [-4.7%–-3.5%]          4.05G
  │  ╰─ level_9        15.0 ±0.5µs  [-5.5%–-4.2%]          4.06G
  ╰─ patterns
     ├─ sequential     15.1 ±0.5µs  [-5.8%–-4.4%]          4.03G
     ╰─ mixed         401.0 ±8.1µs  [+2370%–+2385%]         156M

  level_9       ██████████████████████████████████████████████ 4.06 GiB/s
  level_6       ██████████████████████████████████████████████ 4.05 GiB/s
  sequential    █████████████████████████████████████████████ 4.03 GiB/s
  level_1       ███████████████████████████████████████████ 3.78 GiB/s
  mixed         ██ 156 MiB/s
```

## Why zenbench

Existing harnesses run benchmarks **sequentially**. Benchmark A runs on a hot CPU; benchmark B runs on an even hotter CPU with degraded turbo boost. System load changes between runs corrupt results.

Zenbench **interleaves**: each round, all benchmarks run in shuffled order. Round N of A and round N of B execute under identical conditions. Paired statistics on the round-by-round differences detect real changes — not thermal drift.

### vs criterion and divan

| Feature | criterion | divan | zenbench |
|---|:---:|:---:|:---:|
| **Execution model** | | | |
| Interleaved round-robin | ❌ | ❌ | ✅ |
| Auto-convergence (stop when precise) | ❌ | ❌ | ✅ |
| Resource gating (detect other benchmarks) | ❌ | ❌ | ✅ |
| **Statistics** | | | |
| Bootstrap confidence intervals | ✅ | ❌ | ✅ |
| Paired comparison test | Welch t | ❌ | Wilcoxon |
| Effect size metric | ❌ | ❌ | Cohen's d |
| Drift detection (thermal/load) | ❌ | ❌ | Spearman r |
| Noise threshold (suppress trivial diffs) | ✅ fixed 1% | ❌ | ✅ configurable |
| **Measurement** | | | |
| Hardware TSC timer (rdtsc/cntvct) | ❌ | ✅ opt-in | ✅ auto |
| Overhead compensation | slope regression | loop subtraction | loop subtraction |
| Stack alignment jitter | ✅ alloca (unsafe) | ❌ | ✅ safe trampoline |
| Deferred drop (exclude Drop from timing) | ❌ | ✅ MaybeUninit | ✅ Vec collect |
| Allocation profiling (GlobalAlloc) | ❌ | ✅ | ✅ |
| **CI / Workflow** | | | |
| Save/load baselines | ❌ | ❌ | ✅ `--baseline=` |
| Regression exit codes (0/1/2) | ❌ | ❌ | ✅ |
| Auto-update baseline on pass | ❌ | ❌ | ✅ `--update-on-pass` |
| Hardware fingerprint / testbed ID | ❌ | ❌ | ✅ |
| Cross-run variance inflation | ❌ | ❌ | ✅ pooled t-test |
| **Output** | | | |
| Terminal report | table | tree | tree (default) + table |
| Bar chart | ❌ | ❌ | ✅ sorted, throughput |
| JSON / CSV / Markdown | ✅ JSON | ❌ | ✅ JSON + CSV + LLM + MD |
| HTML plots (violin/PDF/regression) | ✅ plotters.rs | ❌ | ❌ |
| HTML report (self-contained, SVG) | ❌ | ❌ | ✅ `--format=html` |
| Streaming per-group | ❌ | ❌ | ✅ |
| Adaptive column layout | ❌ | ❌ | ✅ terminal-width aware |
| **API** | | | |
| Async benchmarks | ✅ to_async() | ❌ | ✅ iter_async() |
| Thread contention testing | ❌ | ✅ threads attr | ✅ bench_contended() |
| Thread scaling analysis | ❌ | ❌ | ✅ bench_scaling() |
| Drop-in criterion migration | — | ❌ | ✅ zero code changes |
| Attribute macros | ❌ | ✅ `#[divan::bench]` | ❌ |
| **Platform** | | | |
| Linux x86_64 / aarch64 | ✅ | ✅ | ✅ |
| Windows x86_64 / ARM64 | ✅ | ✅ | ✅ |
| macOS ARM64 / Intel | ✅ | ✅ | ✅ |

## Quick start

```toml
# Cargo.toml
[dev-dependencies]
zenbench = "0.1"

[[bench]]
name = "my_bench"
harness = false
```

```rust,no_run
use zenbench::prelude::*;

fn bench_sort(suite: &mut Suite) {
    suite.group("sort", |g| {
        g.throughput(Throughput::Elements(1000));

        g.bench("std_sort", |b| {
            b.with_input(|| (0..1000).rev().collect::<Vec<i32>>())
                .run(|mut v| { v.sort(); v })
        });

        g.bench("sort_unstable", |b| {
            b.with_input(|| (0..1000).rev().collect::<Vec<i32>>())
                .run(|mut v| { v.sort_unstable(); v })
        });
    });
}

zenbench::main!(bench_sort);
```

## CI regression testing

```bash
# After merging to main — save a baseline
cargo bench -- --save-baseline=main

# On PRs — check for regressions (exits 1 if > 5% slower)
cargo bench -- --baseline=main

# Auto-update baseline on clean runs
cargo bench -- --baseline=main --update-on-pass --max-regression=5
```

```text
  Baseline comparison
  ───────────────────
  compress::level_1     16.2µs →   16.4µs    +1.2%    unchanged
  compress::level_6     15.1µs →   15.3µs    +1.3%    unchanged
  compress::level_9     15.0µs →   15.6µs    +4.0%    unchanged
  compress::mixed      401.0µs →  412.3µs    +2.8%    unchanged
  decompress::zenflate  91.5µs →   92.7µs    +1.3%    unchanged

  Summary: 0 regressions, 0 improvements, 5 unchanged

[zenbench] PASS: no regressions exceed 5% threshold
```

Full CI guide with GitHub Actions workflows: [REGRESSION-TESTING.md](REGRESSION-TESTING.md)

## Thread scaling

```rust,ignore
suite.group("scaling", |g| {
    g.throughput(Throughput::Elements(10_000));
    g.bench_scaling("work", |b, _tid| {
        b.iter(|| expensive_computation())
    });
});
```

```text
  scaling  200 rounds × 77 calls
                    mean ±mad µs  95% CI vs base    items/s
  ├─ sqrt_1t        4.2 ±0.1µs  [4.2–4.3]µs       2.37G
  ├─ sqrt_2t        4.7 ±0.1µs  [+10.7%–+12.6%]   2.12G
  ├─ sqrt_4t        5.8 ±0.1µs  [+36.0%–+38.8%]   1.72G
  ├─ sqrt_8t        8.5 ±0.3µs  [+91.6%–+101%]    1.17G
  ╰─ sqrt_16t      14.2 ±0.3µs  [+232%–+245%]      703M

  sqrt_1t   ██████████████████████████████████████████████████ 2.37G
  sqrt_2t   █████████████████████████████████████████████ 2.12G
  sqrt_4t   ████████████████████████████████████ 1.72G
  sqrt_8t   █████████████████████████ 1.17G
  sqrt_16t  ███████████████ 703M
```

## Subgroups and organization

```rust,ignore
suite.group("dispatch", |g| {
    g.throughput(Throughput::Elements(100));
    g.throughput_unit("checks");

    g.subgroup("Generic (monomorphized)");
    g.bench("impl Stop (Stopper)", |b| b.iter(|| check_stopper()));
    g.bench("impl Stop (FnStop)", |b| b.iter(|| check_fn()));

    g.subgroup("Dynamic dispatch");
    g.bench("&dyn Stop", |b| b.iter(|| check_dyn()));
    g.bench("StopToken", |b| b.iter(|| check_token()));

    g.baseline("impl Stop (Stopper)");
    g.sort_by_speed();
});
```

```text
  dispatch  200 rounds × 10K calls
                                mean ±mad ns  95% CI vs base     checks/s
  ├─ Generic (monomorphized)
  │  ├─ impl Stop (FnStop)      19.7 ±0.3ns  [-49.1%–-47.2%]      5.08G
  │  ╰─ impl Stop (Stopper)     38.5 ±0.5ns  [37.9–39.1]ns        2.60G
  ╰─ Dynamic dispatch
     ├─ StopToken                97.2 ±1.2ns  [+148%–+154%]        1.03G
     ╰─ &dyn Stop              112.5 ±3.1ns  [+176%–+193%]         889M

  impl Stop (FnStop)   ██████████████████████████████████████████████ 5.08G
  impl Stop (Stopper)  █████████████████████████████ 2.60G
  StopToken            ████████████ 1.03G
  &dyn Stop            ██████████ 889M
```

## Migrating from criterion

Add zenbench alongside criterion — migrate one file at a time:

```toml
[dev-dependencies]
criterion = "0.8"                                          # keep
zenbench = { version = "0.1", features = ["criterion-compat"] }  # add
```

Change one import per file — **zero code changes** to benchmark functions:

```rust,ignore
// Before:
use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId, Throughput};

// After:
use zenbench::criterion_compat::*;
use zenbench::{criterion_group, criterion_main};
```

Closures can borrow local data — no `move` or `Clone` needed. Your existing `criterion_group!`, `criterion_main!`, `bench_function`, `bench_with_input`, `BenchmarkId`, `Throughput`, `group.sample_size()`, `group.measurement_time()`, and `group.finish()` all work unchanged.

Full upgrade ladder: [MIGRATION.md](MIGRATION.md)

## Output formats

```bash
cargo bench                           # tree display (default, stderr)
cargo bench -- --style=table          # bordered tables with min column
cargo bench -- --format=json          # structured JSON (stdout)
cargo bench -- --format=csv           # spreadsheet-friendly (stdout)
cargo bench -- --format=llm           # key=value for AI tools (stdout)
cargo bench -- --format=md            # markdown tables (stdout)
```

## API reference

```rust,ignore
use zenbench::prelude::*;

// Interleaved comparison group
suite.group("name", |g| {
    g.throughput(Throughput::Bytes(1024));
    g.subgroup("variant");
    g.bench("impl", |b| b.iter(|| work()));
    g.bench("with_setup", |b| {
        b.with_input(|| make_data()).run(|data| process(data))
    });
    g.bench("deferred_drop", |b| {
        b.iter_deferred_drop(|| Vec::<u8>::with_capacity(1024))
    });
});

// Single function shorthand
suite.bench_fn("fibonacci", || fib(20));

// Thread contention
g.bench_contended("mutex", 4, || Mutex::new(Map::new()), |b, m, tid| {
    b.iter(|| { m.lock().unwrap().insert(tid, 42); })
});

// Automatic thread scaling (probes 1..num_cpus)
g.bench_scaling("work", |b, _tid| b.iter(|| compute()));
```

## Configuration

```rust,ignore
group.config()
    .max_rounds(200)              // default 200
    .noise_threshold(0.02)        // ±2% significance gate
    .bootstrap_resamples(100_000) // CI precision (default 10K)
    .linear_sampling(true)        // slope regression for sub-100ns
    .cold_start(true)             // 1 iter + cache firewall
    .stack_jitter(true)           // random alignment (default on)
    .sort_by_speed(true);         // fastest first in report
```

## Platform support

Tested on all targets via GitHub Actions CI:

| Platform | Timer | Notes |
|---|---|---|
| Linux x86_64 | TSC (rdtsc) | Full support |
| Linux aarch64 | Counter (cntvct_el0) | Full support |
| Windows x86_64 | TSC (rdtsc) | Full support |
| Windows ARM64 | Instant (~300ns) | No hardware counter in user mode |
| macOS ARM64 | Counter (cntvct_el0) | Full support |
| macOS Intel | TSC (rdtsc) | Full support |

## License

MIT OR Apache-2.0
