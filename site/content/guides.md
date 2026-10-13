+++
title = "Choose a guide"
description = "Start with the question you need your benchmark to answer."
weight = 0
+++

## What do you need to do?

| Your question | Start here | You finish with |
|---|---|---|
| How do I benchmark a Rust function? | [Your first Rust benchmark](@/getting-started.md) | A native comparison group with a declared timing boundary |
| Does this edit look promising? | [Explore a code change](@/iterate.md) | Exploratory results and a candidate worth confirming |
| Can this candidate replace the baseline? | [Confirm a change / CI](@/ci-regression.md) | A fixed experiment and an explicit acceptance decision |
| How do I compare programs or startup environment variables? | [Compare external programs](@/external-programs.md) | Fresh serial A/B processes, correctness checks and A/A controls |
| Can I accept a separate program or environment change? | [Confirm an external comparison](@/external-confirmation.md) | Fixed blocks, a timing error bound and a checked A/A decision |
| Why does placement fail, or how do I measure Rayon? | [CPU placement and shared machines](@/cpu-placement.md) | One declared CPU class and an appropriate pool size |
| How do I keep and compare results? | [Save and review evidence](@/results.md) | Full raw archives and one selected Git summary |
| How do I make charts for slides? | [Charts for slides and reports](@/charts.md) | SVG exports, fixed light/dark themes and the right statistical caption |
| Why did allocations, heap use or instructions change? | [Profile memory and CPU work](@/profiling.md) | Separate diagnostic artifacts tied to the measured build |
| How do I repeat a study on multiple machines? | [Study cells and fleet observations](@/fleet.md) | Named cells, verified bundles and retained attempts |
| Can I keep my Criterion benches? | [Migrate existing benchmarks](@/migration.md) | Exploratory compatibility or registered native groups |

## Install the version these guides describe

The [first benchmark](@/getting-started.md) starts with `Cargo.toml` and
`cargo bench`. These guides describe zenbench 0.1.11. Install the optional CLI
for retained-result tools, CPU launch control, or external-program comparisons:

```sh
cargo install zenbench --version 0.1.11 --locked --bin zenbench
zenbench guide
```

[Installation details](https://github.com/imazen/zenbench/blob/main/docs/installation.md)
include source builds and revision pinning. CPU pinning also has a
[guide in docs.rs](https://docs.rs/zenbench/latest/zenbench/platform/index.html#thread-pinning).

## Know which command owns the arguments

```sh
# A Rust benchmark: Cargo forwards flags after --.
cargo bench --bench my_bench -- --help

# A retained zenbench benchmark executable: flags go directly to it.
./builds/candidate/my_bench --help

# The installed CLI: each task has its own help.
zenbench pair --help
zenbench data --help
```

`my_bench`, input paths, build paths and IDs throughout these guides are examples
for you to replace. Shell commands use a POSIX shell unless marked PowerShell.
Arguments in a pair JSON plan are literal strings on every supported platform.

## Explore first, decide on fresh evidence

**Iterate** is the default: one process, normally up to 30 rounds and 100 ms
warmup per arm. Use it while editing. Exit 0 means execution succeeded.

**Confirm** runs a fixed plan in fresh processes, normally five. Freeze the
candidate, workload, practical margin and sampling budget before measuring.
Use it to accept a change or promote a baseline. Adding `--scope` identifies one
study cell; the retained `--mode=evidence` spelling uses the same protocol and
requires scope.

External `pair` defaults to Iterate; its successful exit means execution completed.
External Confirm requires its own frozen JSON policy, timing error bound and A/A
control. Saved suite comparison,
external pair comparison and fleet observation comparison have different contracts:

| Command | What it does | Meaning of exit 0 |
|---|---|---|
| `zenbench data compare BEFORE AFTER` | Reads two suite packets | Every required case confirms non-inferiority |
| `zenbench data pair-diff BEFORE AFTER` | Reads two external pair packets | Descriptive comparison completed |
| `zenbench fleet diff BEFORE AFTER` | Reads two compatible fleet attempts | Descriptive comparison completed |

All three read retained data; they do not execute the programs again. For the
reasoning behind these distinctions, read [methodology](@/methodology.md) and the
[full command contract](https://github.com/imazen/zenbench/blob/main/docs/cli-migration.md).

## Offline help by task

| Command | Use it for |
|---|---|
| `zenbench guide` | The edit → freeze → confirm workflow |
| `zenbench guide --topic pair` | External plan examples and extractor contracts |
| `zenbench guide --topic methodology` | Measurement assumptions and busy systems |
| `zenbench guide --topic simd` | Dispatch, correctness, assembly and profiling |
| `zenbench guide --topic fleet` | Heap quantities and deployment |
| `zenbench cpu-info` / `zenbench exec --help` | Inspecting and selecting placement |
| `zenbench nice --help` | Cooperating background work |
| `zenbench self-compare --help` | Building a source revision and the current tree |
| `zenbench data --help` | Reading, comparing, exporting and reanalyzing evidence |
| `zenbench fleet --help` | Bundles and individual observations |
| `zenbench jobs --help` | Exploratory background jobs |
