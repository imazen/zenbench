+++
title = "Migrate existing benchmarks"
description = "Keep familiar Criterion calls for exploration; use native groups for confirmation."
weight = 11
+++

Choose whether you need familiar Criterion calls for exploratory reporting or
registered native Suite groups for interleaving and confirmation. The current
compatibility layer executes measurements immediately and sequentially; it does
not supply the fixed-process evidence needed for baseline promotion.

## Keep Criterion-style calls

Enable compatibility on the Git dependency used by these guides:

```toml
[dev-dependencies]
zenbench = { git = "https://github.com/imazen/zenbench", branch = "main", features = ["criterion-compat"] }
```

Keep Criterion itself while comparing separate targets. In a zenbench target,
replace imports with:

```rust
use zenbench::criterion_compat::*;
use zenbench::{criterion_group, criterion_main};
```

Common calls include `benchmark_group`, `bench_function`, `bench_with_input`,
`BenchmarkId`, `Throughput`, `iter`, `iter_batched` and `iter_batched_ref`, plus
sample size, measurement-time and warmup configuration. Inspect the
[current compatibility source](https://github.com/imazen/zenbench/blob/main/src/criterion_compat.rs)
for the subset you use rather than assuming every Criterion API is covered.

Immediate `bench_function` and `bench_with_input` closures may borrow local data.
Batched **setup** closures still need `'static`; captured setup state can require
`move` and owned/cloned values. Timing boundaries and configuration semantics may
also differ. Run correctness checks and inspect your migrated report.

```sh
cargo bench --bench my_criterion_bench
cargo bench --bench my_zenbench_bench -- --format=html
```

The names are example targets you define separately with `harness = false`.
Compatibility timing is exploratory even when its report contains a narrow
interval. It cannot accept a Confirm candidate or promote a baseline.

## Move comparisons into native groups

For interleaving and the current confirmation protocol, register the arms in
`Suite::group` and use `zenbench::main!`:

```rust
use zenbench::prelude::*;

fn benches(suite: &mut Suite) {
    suite.group("sum", |g| {
        g.bench("iterator", |b| {
            b.iter(|| std::hint::black_box(0..1000u64).sum::<u64>())
        });
        g.bench("loop", |b| {
            b.iter(|| {
                let mut total = 0;
                for value in std::hint::black_box(0..1000u64) {
                    total += value;
                }
                total
            })
        });
    });
}

zenbench::main!(benches);
```

This is a registration example, not a measured speed claim. Confirm equal outputs
and use representative inputs before timing. Native `g.bench` closures are stored
for later interleaved execution and need owned `'static` captures when state is
captured. Replace `bench_function` with `bench` and wrap registration in
`suite.group(...)`; no `finish()` call is needed.

Follow [the first benchmark guide](@/getting-started.md) to inspect and run it,
then [confirm a frozen candidate](@/ci-regression.md).

## Update CLI scripts by task

The preferred command groups are `guide`, `cpu-info`, `exec`, `pair`, `nice`,
`self-compare`, `data`, `fleet` and `jobs`. Previous flat command names remain
accepted. Typical replacements:

| Older spelling | Preferred spelling |
|---|---|
| `zenbench read FILE` | `zenbench data read FILE` |
| `zenbench compare BEFORE AFTER` | `zenbench data compare BEFORE AFTER` |
| `zenbench snapshot FILE --output CSV` | `zenbench data snapshot FILE --output CSV` |
| `zenbench baseline list` | `zenbench data baseline list` |
| `zenbench fleet-run BUNDLE ...` | `zenbench fleet run BUNDLE ...` |
| `zenbench run NAME ...` | `zenbench jobs run NAME ...` |

Both `--name=value` and `--name value` work for benchmark options. Preserve recorded
mode/plan identity within an experiment. Read the
[complete migration contract](https://github.com/imazen/zenbench/blob/main/docs/cli-migration.md)
for aliases, decision exits and API compatibility.
