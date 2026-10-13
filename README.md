# zenbench [![CI](https://img.shields.io/github/actions/workflow/status/imazen/zenbench/ci.yml?style=flat-square&label=CI)](https://github.com/imazen/zenbench/actions/workflows/ci.yml) [![crates.io](https://img.shields.io/crates/v/zenbench?style=flat-square)](https://crates.io/crates/zenbench) [![lib.rs](https://img.shields.io/crates/v/zenbench?style=flat-square&label=lib.rs&color=blue)](https://lib.rs/crates/zenbench) [![docs.rs](https://img.shields.io/docsrs/zenbench?style=flat-square)](https://docs.rs/zenbench) [![MSRV](https://img.shields.io/badge/MSRV-1.85-blue?style=flat-square)](https://doc.rust-lang.org/cargo/reference/manifest.html#the-rust-version-field) [![license](https://img.shields.io/crates/l/zenbench?style=flat-square)](#license) [![codecov](https://img.shields.io/codecov/c/github/imazen/zenbench?style=flat-square)](https://codecov.io/gh/imazen/zenbench)

Interleaved microbenchmarking for Rust. Start with an ordinary Cargo benchmark;
add comparisons and confirmation as your question becomes more specific.

## Your first benchmark

Add this to `Cargo.toml`:

```toml
[[bench]]
name = "my_bench"
harness = false

[dev-dependencies]
zenbench = "0.1.11"
```

Create `benches/my_bench.rs`:

```rust,no_run
use zenbench::prelude::*;

fn bench_sort(suite: &mut Suite) {
    suite.bench_fn("sort", || {
        let mut values: Vec<i32> = (0..1000).rev().collect();
        values.sort_unstable();
        values
    });
}

zenbench::main!(bench_sort);
```

Run it:

```sh
cargo bench --bench my_bench
```

This measures creating, sorting, and dropping a 1,000-element vector. The return
value passes through `black_box` to keep the work observable. The default run is
exploratory: it helps you investigate performance while editing.

If the run reports a CPU placement refusal, follow the separate
[CPU placement guide](https://docs.rs/zenbench/latest/zenbench/platform/index.html#verified-cpu-placement).
Rust 1.85 or newer is required.

## Compare implementations

Put alternatives in one group. Zenbench runs every arm in shuffled order each
round and reports paired comparisons. Here the timing boundary excludes vector
creation and returned-output destruction, so the comparison measures sorting:

```rust,no_run
use zenbench::prelude::*;

fn bench_sort(suite: &mut Suite) {
    suite.group("sort", |g| {
        g.throughput(Throughput::Elements(1000));
        g.bench("sort", |b| {
            b.with_input(|| (0..1000).rev().collect::<Vec<i32>>())
                .run(|mut values| { values.sort(); values })
        });
        g.bench("sort_unstable", |b| {
            b.with_input(|| (0..1000).rev().collect::<Vec<i32>>())
                .run(|mut values| { values.sort_unstable(); values })
        });
    });
}

zenbench::main!(bench_sort);
```

Lower latency is faster. Negative percent changes mean the candidate is faster
than the baseline; an interval crossing zero leaves the direction unresolved.
Interleaving keeps comparisons close in time, but cannot guarantee matching
clocks, caches, or scheduling.

The [benchmarking guide](https://docs.rs/zenbench/latest/zenbench/struct.Suite.html#writing-benchmarks)
covers timing boundaries, throughput, thread scaling, output formats, and
configuration. It is included in the crate's docs.rs documentation.

## Confirm a change

Use the default exploratory run while editing. Before accepting a performance
change, freeze the candidate and workload, then use a fixed confirmation plan:

```sh
cargo bench --bench my_bench -- --mode=confirm --workload-id=sort-v1 --dry-run
```

[Confirmation and CI](https://github.com/imazen/zenbench/blob/main/docs/benchmark-modes.md)
explains fresh processes, saved baselines, practical margins, and acceptance
exits. Within-process comparisons alone cannot authorize baseline promotion.

## Read further when you need it

- [CPU pinning and worker pools](https://docs.rs/zenbench/latest/zenbench/platform/index.html#thread-pinning), then
  [verified process placement](https://docs.rs/zenbench/latest/zenbench/platform/index.html#verified-cpu-placement).
- [Retain results and export a small summary](https://github.com/imazen/zenbench/blob/main/docs/result-storage.md).
- [Migrate Criterion benchmarks](https://github.com/imazen/zenbench/blob/main/MIGRATION.md).
- [Install the optional CLI](https://github.com/imazen/zenbench/blob/main/docs/installation.md) to
  compare [external programs](https://github.com/imazen/zenbench/blob/main/docs/external-process.md),
  [profile heap use](https://github.com/imazen/zenbench/blob/main/docs/heap-profiling.md), or
  deploy [multi-machine studies](https://github.com/imazen/zenbench/blob/main/docs/fleet.md).
- [Measurement methods and limits](https://github.com/imazen/zenbench/blob/main/METHODOLOGY.md).

[Step-by-step tutorials](https://imazen.github.io/zenbench/getting-started/) ·
[API reference](https://docs.rs/zenbench) ·
[Example HTML report](https://imazen.github.io/zenbench/example-report.html)

## License

MIT OR Apache-2.0
