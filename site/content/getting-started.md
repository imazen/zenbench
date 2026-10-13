+++
title = "Your first Rust benchmark"
description = "Compare two implementations in one interleaved native Suite group."
weight = 1
+++

This example compares two ways to sort the same reversed integer input. Setup
and returned-output destruction are excluded so the question is the cost of
sorting an existing vector. Choose a timing boundary that matches your own use
case before adapting it.

## 1. Add a benchmark target

Add the zenbench 0.1.11 dependency and a benchmark target:

```toml
[[bench]]
name = "my_bench"
harness = false

[dev-dependencies]
zenbench = "0.1.11"
```

Add these entries to your existing `Cargo.toml`; keep your other dependencies and
targets. Rust 1.85 or newer is required. See [installation](@/guides.md#install-the-version-these-guides-describe)
for the optional CLI and revision pinning.

## 2. Check correctness and register both arms

Create `benches/my_bench.rs`:

```rust
use zenbench::prelude::*;

fn bench_sort(suite: &mut Suite) {
    // An untimed correctness control. Extend it to your actual input domain.
    let input: Vec<i32> = (0..1000).rev().collect();
    let mut expected = input.clone();
    expected.sort();
    let mut actual = input;
    actual.sort_unstable();
    assert_eq!(expected, actual);

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

Each round runs both arms in shuffled order. Creating a fresh vector for every
operation prevents the next operation from silently measuring already sorted data.
Returning the vector moves its destruction outside the `with_input` timer.
Any destruction you perform inside the measured closure stays included.

## 3. Inspect, then run

```sh
cargo bench --bench my_bench -- --help
cargo bench --bench my_bench -- --group=sort --dry-run
cargo bench --bench my_bench -- --group=sort
```

A dry run resolves registration, groups, placement, plan and storage without
running benchmark callbacks. If placement is refused, follow
[CPU placement](@/cpu-placement.md) before measuring.

Default Iterate uses one process, up to 30 rounds and 100 ms warmup per arm unless
your group overrides them. Budgets are not runtime predictions. A successful
exit means the benchmark completed; it does not establish acceptance.

## 4. Read the result

The report includes mean latency, spread and comparison intervals. Lower latency
is faster; a positive percent latency change means slower. Throughput is derived
from the work count you declared. An interval crossing zero leaves direction
unresolved; a narrow exploratory interval still needs a separate confirmation
before acceptance. See [example output](@/examples.md).

You have completed this guide when the outputs agree, both arms appear in the
`sort` group and the report represents your intended timing boundary. Next,
[explore an edit](@/iterate.md) or [freeze and confirm](@/ci-regression.md).

## Choose the right timing API

| Need | API | Included work |
|---|---|---|
| Time a complete repeated operation | `b.iter(|| work())` | Closure and returned-output destruction |
| Exclude per-operation setup and returned-output destruction | `b.with_input(|| make_data()).run(|data| work(data))` | Work closure, with a timer per operation |
| Exclude destruction of returned outputs | `b.iter_deferred_drop(|| work())` | Work and storing outputs; all outputs remain live until batch end |
| Time work already managed by Rayon or Tokio | `g.bench(...)` | Wait for all submitted work to finish inside the callback |
| Create independent workers yourself | `g.bench_parallel(...)` | Coordinator wall time through worker completion |
| Measure shared-state contention | `g.bench_contended(...)` | Shared worker execution and synchronization |

Deferred destruction changes live memory and cache behavior. Avoid using it as
an automatic replacement for `iter`. Configure external pools yourself and keep
the thread count fixed across arms. The [API reference](https://docs.rs/zenbench)
covers released signatures; the [current source](https://github.com/imazen/zenbench/blob/main/src/bench.rs)
and [measurement protocol](https://github.com/imazen/zenbench/blob/main/METHODOLOGY.md)
cover the implementation described here.
