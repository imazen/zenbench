+++
title = "Migrating from criterion"
weight = 3
+++

## Step 1: Add zenbench alongside criterion

```toml
[dev-dependencies]
criterion = "0.8"                                          # keep
zenbench = { version = "0.1", features = ["criterion-compat"] }  # add
```

## Step 2: Change one import per file

```rust
// Before:
use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId, Throughput};

// After:
use zenbench::criterion_compat::*;
use zenbench::{criterion_group, criterion_main};
```

**Zero code changes** to your benchmark functions. Closures can borrow local data — no `move` or `Clone` needed.

## Step 3: Run both

Both criterion and zenbench bench files coexist. Run each independently:

```bash
cargo bench --bench my_criterion_bench    # still works
cargo bench --bench my_zenbench_bench     # new
```

## Step 4: Remove criterion when done

```toml
[dev-dependencies]
zenbench = "0.1"
```

## What works unchanged

Everything in criterion's common API:

- `criterion_group!` / `criterion_main!`
- `c.benchmark_group()` / `group.bench_function()` / `group.finish()`
- `c.bench_function()` / `c.bench_with_input()`
- `BenchmarkId::new()` / `BenchmarkId::from_parameter()`
- `Throughput::Bytes()` / `Throughput::Elements()`
- `b.iter()` / `b.iter_batched()` / `b.iter_batched_ref()`
- `group.sample_size()` / `group.measurement_time()` / `group.warm_up_time()`
- `black_box()`

## Incremental upgrades

Once migrated, add zenbench features one line at a time:

```rust
group.throughput_unit("pixels");       // custom unit: "Gpixels/s"
group.baseline("reference_impl");      // compare all vs this one
group.sort_by_speed();                 // fastest first in report
group.subgroup("SIMD variants");       // visual section headers
```

CLI flags work immediately:

```bash
cargo bench -- --save-baseline=main       # CI baselines
cargo bench -- --baseline=main            # regression gates
cargo bench -- --format=html              # browser report
```

## Switching to the native API

When you want interleaved execution, the diff is small:

```rust
// criterion-compat (sequential):        // native (interleaved):
let mut g = c.benchmark_group("sort");   suite.group("sort", |g| {
g.bench_function("std", |b| ...);           g.bench("std", |b| ...);
g.bench_function("unstable", |b| ...);      g.bench("unstable", |b| ...);
g.finish();                              });
```

`bench_function` → `bench`, drop `finish()`, wrap in `suite.group()`.

## Comparison table

| | criterion | zenbench compat | zenbench native |
|---|---|---|---|
| Code changes | — | **zero** | small rewrite |
| Interleaving | no | no | **yes** |
| CI regression | no | **yes** | **yes** |
| Baselines | no | **yes** | **yes** |
| Output formats | JSON/HTML | **JSON/CSV/LLM/MD/HTML** | same |
| Resource gating | no | **yes** | **yes** |
