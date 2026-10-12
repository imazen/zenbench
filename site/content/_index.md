+++
title = "zenbench"
description = "Task guides for Rust benchmarks and command-line experiments."
template = "home.html"
sort_by = "weight"
+++

Zenbench compares Rust functions in shuffled, serial rounds. Use a short
**Iterate** run while editing, then freeze the workload and candidate before
**Confirm**. The CLI also compares prebuilt programs and startup environments,
keeps evidence, and records memory or fleet observations.

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
commands can make an acceptance decision. These guides describe the current
**unreleased Git version**; pin the reviewed revision for a reproducible study.

## Begin with a Rust comparison

```sh
cargo bench --bench my_bench -- --help
cargo bench --bench my_bench -- --group=sort
```

Create `my_bench` using the [first benchmark tutorial](@/getting-started.md).
Exit 0 in Iterate means execution succeeded. A comparison interval in an
exploratory report does not accept a candidate for CI.

## Begin with the CLI

```sh
cargo install --git https://github.com/imazen/zenbench --locked --bin zenbench
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
