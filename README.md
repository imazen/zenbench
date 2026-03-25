# zenbench

Interleaved microbenchmarking for Rust with paired statistics, CI regression testing, and hardware-adaptive measurement.

[![CI](https://img.shields.io/github/actions/workflow/status/imazen/zenbench/ci.yml?style=for-the-badge)](https://github.com/imazen/zenbench/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/zenbench?style=for-the-badge)](https://crates.io/crates/zenbench)
[![docs.rs](https://img.shields.io/docsrs/zenbench?style=for-the-badge)](https://docs.rs/zenbench)
[![License](https://img.shields.io/crates/l/zenbench?style=for-the-badge)](LICENSE-MIT)

## Why zenbench

Every round, all benchmarks in a group run in **shuffled order**. System state affects them equally. Paired tests on round-by-round differences detect changes that sequential harnesses miss.

## Quick start

```toml
[dev-dependencies]
zenbench = "0.1"

[[bench]]
name = "my_bench"
harness = false
```

```rust
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

## Output

```text
  sort  200 rounds × 3K calls
                     mean ±mad ns  95% CI vs base     items/s
  ├─ std_sort        258 ±5ns  [255–262]ns          3.87G
  ╰─ sort_unstable   246 ±4ns  [-5.5%–-4.2%]        4.06G
  sort_unstable  ██████████████████████████████████████████ 4.06 Gitems/s
  std_sort       ████████████████████████████████████████ 3.87 Gitems/s
```

Also: `--style=table` for bordered tables, `--format=json|csv|llm|md`.

## CI regression testing

```bash
cargo bench -- --save-baseline=main           # save after merge
cargo bench -- --baseline=main                # check PR (exit 1 on regression)
cargo bench -- --baseline=main --update-on-pass  # auto-ratchet
```

GitHub Actions workflow and full guide: [REGRESSION-TESTING.md](REGRESSION-TESTING.md)

## Migrating from criterion

Change one import per file — zero code changes:

```rust
// Before:
use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId, Throughput};
// After:
use zenbench::criterion_compat::*;
use zenbench::{criterion_group, criterion_main};
```

Requires `features = ["criterion-compat"]`. Full guide: [MIGRATION.md](MIGRATION.md)

## Features

**Measurement:**
interleaved execution · TSC hardware timer · stack alignment jitter · overhead compensation · deferred drop · slope regression · calibration workloads · allocation profiling

**Statistics:**
bootstrap 95% CI · Wilcoxon signed-rank · Cohen's d effect size · Spearman drift detection · noise threshold · per-benchmark CIs · auto-convergence

**CI/Workflow:**
baseline save/load · regression exit codes · `--update-on-pass` · benchmark process detection · hardware fingerprinting · cross-run variance inflation

**Output:**
tree display (default) · table display · JSON/CSV/LLM/Markdown · streaming per-group · adaptive column layout · bar charts

**API:**
`group()` with interleaving · `bench_fn()` shorthand · `with_input().run()` · `iter_deferred_drop()` · `bench_contended()` / `bench_parallel()` / `bench_scaling()` · `iter_async()` · criterion compat layer

**Platform:** Linux x64/ARM64 · Windows x64/ARM64 · macOS ARM64/Intel

## License

MIT OR Apache-2.0
