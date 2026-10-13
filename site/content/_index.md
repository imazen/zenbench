+++
title = "zenbench"
description = "Task guides for Rust benchmarks and command-line experiments."
template = "home.html"
sort_by = "weight"
+++

Zenbench compares Rust functions in shuffled, serial rounds. Begin with an
ordinary Cargo benchmark, then add comparisons and confirmation when needed.

## Add a benchmark target

```toml
[[bench]]
name = "my_bench"
harness = false

[dev-dependencies]
zenbench = "0.1.11"
```

Create `benches/my_bench.rs` using the [first benchmark tutorial](@/getting-started.md),
then run:

```sh
cargo bench --bench my_bench
```

Default **Iterate** runs are exploratory. Freeze your workload and candidate
before using **Confirm** to accept a change.

## Start with your task

- [Write your first Rust benchmark](@/getting-started.md)
- [Explore a code change](@/iterate.md)
- [Confirm a candidate or guard CI](@/ci-regression.md)
- [Compare programs or environment settings](@/external-programs.md)
- [Confirm an external program or environment change](@/external-confirmation.md)
- [Choose CPUs and work on a shared machine](@/cpu-placement.md)
- [Keep results and review a small diff](@/results.md)
- [Export charts for slides and reports](@/charts.md)
- [Investigate memory or CPU work](@/profiling.md)
- [Run named study cells and fleet observations](@/fleet.md)

[Choose a guide](@/guides.md) also covers installation, offline help and which
commands can make an acceptance decision. These guides describe zenbench **0.1.11**.

## Begin with the CLI

```sh
cargo install zenbench --version 0.1.11 --locked --bin zenbench
zenbench guide
zenbench pair --help
```

External pair plans retain randomized fresh-process A/B blocks, correctness
contracts and A/A controls. Use Iterate to explore, then
[external Confirm](@/external-confirmation.md) for a frozen block-level decision.
For native-suite acceptance, follow [confirmation](@/ci-regression.md).

## Read further

[Migrate Criterion benchmarks](@/migration.md) · [Understand the methodology](@/methodology.md) ·
[See report output](@/examples.md) · [API documentation](https://docs.rs/zenbench) ·
[Source and issues](https://github.com/imazen/zenbench)
