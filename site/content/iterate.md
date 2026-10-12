+++
title = "Explore a code change"
description = "Get useful edit feedback, keep a control, and choose what to confirm."
weight = 2
+++

Start here when you have a correct implementation and want to try one change.
Use a small group representing the real input sizes, distributions, cache reuse,
features and thread counts. Preserve the control while the candidate changes.

## Compare alternatives in one Rust executable

Register old and new implementations in the same
[native Suite group](@/getting-started.md). Replace `my_bench` and `sort` with
your target and group:

```sh
cargo bench --bench my_bench -- --group=sort
cargo bench --bench my_bench -- --group=sort --format=llm
```

Iterate is the default. Look at effect direction, spread, drift and resource
flags. If the apparent gain is small or unstable, diagnose the mechanism with
[assembly or profiling](@/profiling.md). A/A controls—running the same
implementation in both slots—help expose a difference caused by the experiment
itself. Keep inconvenient controls; do not automatically subtract their bias.

Change one mechanism at a time and rerun correctness over the actual domain,
including short inputs and tails. A gain for one hot kernel does not establish
an end-to-end application gain.

## Compare a source edit to a Git revision

Install the [current CLI](@/guides.md#install-the-version-these-guides-describe), then:

```sh
zenbench self-compare --bench my_bench --ref HEAD~1 --cargo-args="-- --group=sort"
```

This builds the old revision in a separate Git worktree and the current working
tree, including uncommitted edits. Both versions need a compatible zenbench
target using `zenbench::main!`. It runs baseline first and candidate second,
retains both packets, and compares them. It does not randomize the two builds'
execution order. The default exploratory comparison returns **2**, so it is not
a CI acceptance command. `--cargo-args` splits on whitespace; arguments containing
spaces need direct retained-executable runs.

## Keep two builds and rerun without compiling

Build each revision with the same profile, target, features and flags:

```sh
cargo bench --bench my_bench --no-run
```

Cargo prints the exact executable path. Copy that executable and required assets
into a fresh versioned directory outside Cargo's replaceable `target` tree.
Use `builds/before/my_bench` and `builds/after/my_bench` below as your retained
paths. On Windows keep the `.exe` and runtime dependencies.

Run both from the same working directory with identical inputs and placement:

```sh
ZENBENCH_RESULT_PATH="$PWD/before.json.gz" ./builds/before/my_bench --group=sort
ZENBENCH_RESULT_PATH="$PWD/after.json.gz" ./builds/after/my_bench --group=sort
zenbench data read before.json.gz
zenbench data read after.json.gz
```

Choose new result paths for each attempt. These commands require zenbench
benchmark executables; arbitrary programs use [external pair plans](@/external-programs.md).
For acceptance, run the retained suite executables with the
[fixed confirmation plan](@/ci-regression.md#compare-two-retained-suite-executables),
then use `zenbench data compare` on their complete packets. Exploratory packets
cannot pass that command.

In PowerShell, set the result environment variable before each launch:

```powershell
$env:ZENBENCH_RESULT_PATH = "$PWD/before.json.gz"
& ./builds/before/my_bench.exe --group=sort
$env:ZENBENCH_RESULT_PATH = "$PWD/after.json.gz"
& ./builds/after/my_bench.exe --group=sort
Remove-Item Env:ZENBENCH_RESULT_PATH
```

## Keep build identity with the executable

The harness saves an executable SHA-256. Its Git hash is queried at **runtime**
in the working directory, so an old binary can record today's HEAD. Build-time
source identity, clean/dirty state and a complete compiler/dependency manifest
are not captured automatically.

Keep a sidecar bound to each executable hash: source revision and patch/snapshot,
toolchain, target, profile, features, compiler flags, dependency/lockfile hashes,
allocator configuration and input hashes. Capture it before editing again.
[Storage guidance](@/results.md) explains what to archive and what to commit.

## Background convenience

```sh
zenbench jobs run my_bench --cargo-args="-- --group=sort"
zenbench jobs list
zenbench jobs status RUN_ID
zenbench jobs results latest
```

Replace `RUN_ID` with the listed ID. This older wrapper is for exploration;
it does not preserve confirmation's 0/1/2 decision codes. Its `--wait` timeout
also does not bound the initial Cargo child wait. Use foreground `cargo bench`
or retained executables for confirmation.

## Stop exploring when you have a concrete hypothesis

Keep the promising candidate, rejected attempts and controls. Freeze the inputs,
case inventory, build, margin and process/round counts, then
[confirm on fresh evidence](@/ci-regression.md). If you cannot explain an unstable
result, retain it as unresolved and investigate rather than selecting the fastest
attempt. The [full workflow](https://github.com/imazen/zenbench/blob/main/docs/benchmark-workflows.md)
covers provenance and inference limits.
