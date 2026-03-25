+++
title = "Getting Started"
weight = 1
+++

## Install

Add zenbench to your project:

```toml
[dev-dependencies]
zenbench = "0.1"

[[bench]]
name = "my_bench"
harness = false
```

## Write your first benchmark

Create `benches/my_bench.rs`:

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

## Run it

```bash
cargo bench
```

You'll see output like:

```text
  sort  200 rounds × 3K calls
                     mean ±mad ns  95% CI vs base     items/s
  ├─ std_sort        258 ±5ns  [255–262]ns          3.87G
  ╰─ sort_unstable   246 ±4ns  [-5.5%–-4.2%]        4.06G

  sort_unstable  ██████████████████████████████████████████ 4.06 Gitems/s
  std_sort       ████████████████████████████████████████ 3.87 Gitems/s
```

## What's happening

Each **round**, both benchmarks run in shuffled order. After 200 rounds, zenbench computes:

- **mean ±mad**: average time per call, ± the noise (Median Absolute Deviation)
- **95% CI vs base**: how much faster/slower than the baseline, with confidence bounds
- **throughput**: operations per second

The bar chart shows relative throughput — longest bar = fastest.

## Next: output formats

```bash
cargo bench -- --format=json    # structured JSON
cargo bench -- --format=csv     # spreadsheet
cargo bench -- --format=html    # browser report with SVG charts
cargo bench -- --style=table    # bordered table instead of tree
```

## Next: subgroups

Organize benchmarks visually:

```rust
suite.group("dispatch", |g| {
    g.subgroup("Generic");
    g.bench("monomorphized", |b| b.iter(|| fast_path()));

    g.subgroup("Dynamic");
    g.bench("vtable", |b| b.iter(|| dyn_path()));
});
```

## Next: thread scaling

```rust
suite.group("scaling", |g| {
    g.throughput(Throughput::Elements(10_000));
    g.bench_scaling("work", |b, _tid| {
        b.iter(|| compute())
    });
    // Automatically probes 1, 2, 3, ..., num_cpus threads
});
```

## API patterns at a glance

```rust
// Simple
b.iter(|| work())

// With setup (setup excluded from timing)
b.with_input(|| make_data()).run(|data| process(data))

// Deferred drop (Drop excluded from timing)
b.iter_deferred_drop(|| Vec::<u8>::with_capacity(1024))

// Thread contention
g.bench_contended("mutex", 4, || Mutex::new(Map::new()), |b, map, tid| {
    b.iter(|| { map.lock().unwrap().insert(tid, 42); })
});

// Single function shorthand
suite.bench_fn("fibonacci", || fib(20));
```
