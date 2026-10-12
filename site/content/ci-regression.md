+++
title = "Confirm a change / CI"
description = "Freeze the candidate and plan, then accept only complete non-inferior evidence."
weight = 3
+++

Use Confirm after [exploration](@/iterate.md) has selected a correct candidate.
This guide requires registered native Suite groups from the current Git version.
Immediate-mode Criterion compatibility cannot supply this suite evidence.
External `pair` has a [separate block-level Confirm contract](@/external-confirmation.md).

## 1. Freeze the experiment

Decide the required cases, input bytes, features, thread counts, cache regime,
practical margin, confidence, process count and rounds before measuring. A workload
ID names the inputs/configuration; change it when those change, and keep it the
same across implementations of that workload.

Confirm defaults to five fresh processes, normally 30 fixed rounds per group,
a 1% margin and 0.95 family confidence. Explicit group settings apply. These are
default budgets, not guarantees of statistical power or elapsed time.

Inspect [CPU placement](@/cpu-placement.md). Each launch needs a compatible,
verified uniform class and retained clean resource observations. Do not switch
to `unrestricted` or disable monitoring to obtain a pass.

## 2. Inspect and save the control population

On the control build, replacing the example target, group and workload ID:

```sh
cargo bench --bench my_bench -- --group=sort --mode=confirm --workload-id=sort-inputs-v1 --dry-run
cargo bench --bench my_bench -- --group=sort --mode=confirm --workload-id=sort-inputs-v1 --save-baseline=before
```

Set `--margin=PERCENT`, `--confidence=FRACTION`, `--processes=N` or `--rounds=N`
on both launches if your predeclared plan differs. Confirmation requires at least
three processes and two rounds. A complete single-arm baseline population makes
no relative improvement claim.

## 3. Measure the frozen candidate

On the candidate build with the same cases, inputs and compatible settings:

```sh
cargo bench --bench my_bench -- --group=sort --mode=confirm --workload-id=sort-inputs-v1 --baseline=before
```

The interval for **every required case** must establish non-inferiority within
the declared margin. Missing or added cases, incompatible placement, incomplete
rounds, stale/flagged observations and invalid numeric results cannot pass.
Inspect the reported process-level decision; descriptive means and within-process
significance flags do not authorize promotion.

| Exit | Meaning | Next step |
|---|---|---|
| 0 | Every required comparison is valid and non-inferior | Accept within the declared margin and scope |
| 1 | At least one confirmed regression | Keep the baseline; fix or reject the candidate |
| 2 | Invalid or inconclusive evidence | Retain the attempt; fix its cause or predeclare a new experiment |

An inconclusive interval does not prove equivalence. Keep unsuccessful attempts;
do not repeatedly sample until one passes. A newly planned experiment must be
identified separately. [Methodology](@/methodology.md) explains the estimand and
independence assumptions.

## 4. Promote only when intended

If an accepted candidate should become the stored baseline, include the explicit
promotion flag in its planned run:

```sh
cargo bench --bench my_bench -- --group=sort --mode=confirm --workload-id=sort-inputs-v1 --baseline=before --update-on-pass
zenbench data baseline list
zenbench data baseline show before
```

Promotion requires non-inferiority for every matched case; it preserves the prior
reference. Avoid using low-level descriptive save APIs as a shortcut to acceptance.

## Compare two retained suite executables

For immutable zenbench binaries built with matched configurations, use fresh paths
and the same working directory and verified placement:

```sh
ZENBENCH_RESULT_PATH="$PWD/control-confirm.json.gz" ./builds/before/my_bench --group=sort --mode=confirm --workload-id=sort-inputs-v1
ZENBENCH_RESULT_PATH="$PWD/candidate-confirm.json.gz" ./builds/after/my_bench --group=sort --mode=confirm --workload-id=sort-inputs-v1
zenbench data compare control-confirm.json.gz candidate-confirm.json.gz
```

`data compare` reads the packets without executing code and returns the 0/1/2
contract above. It refuses shared child run identities. Historical controls are
independent populations; they are not contemporaneous randomized two-binary A/B
blocks. Keep that limitation in the claim. See [retaining builds](@/iterate.md#keep-two-builds-and-rerun-without-compiling).

## Carry evidence between CI jobs

Persist the named baseline references **and their complete raw objects**, not
only a CSV. Keep `ZENBENCH_DATA_DIR` in an artifact location that survives jobs;
use the same compatible machine/class and workload. Ordinary shared CI load can
make confirmation invalid, so treat that status as a measurement outcome rather
than a code regression. See [saving evidence](@/results.md).

Use direct foreground invocation. The `jobs run` wrapper does not preserve
confirmation decisions. The 300-second default confirmation deadline covers the
supervised children, including callbacks; Cargo compilation is outside it.

For a named hardware/workload cell, add `--scope=linux/performance/sort` to Confirm.
[Study cells](@/fleet.md) explains matrix coverage. The legacy Evidence spelling
uses the same protocol with mandatory scope.

[Full regression guide](https://github.com/imazen/zenbench/blob/main/REGRESSION-TESTING.md),
[mode contract](https://github.com/imazen/zenbench/blob/main/docs/benchmark-modes.md)
and [storage contract](https://github.com/imazen/zenbench/blob/main/docs/result-storage.md)
provide the complete compatibility and decision rules.
