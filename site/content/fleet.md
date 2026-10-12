+++
title = "Study cells and fleet observations"
description = "Keep hardware/workload cells separate and retain every deployment attempt."
weight = 10
+++

Start here when a result must cover several machines, CPU classes, workloads or
configurations. Define the matrix and correctness controls first. One successful
cell does not certify the rest of the study; do not average unlike hardware into
one baseline by accident.

## Confirm one native-suite study cell

Run the [fixed confirmation workflow](@/ci-regression.md) for each required cell,
adding its scope on the control and candidate:

```sh
cargo bench --bench my_bench -- --group=sort --mode=confirm --workload-id=sort-inputs-v1 --scope=linux/performance/sort --save-baseline=cell-before
cargo bench --bench my_bench -- --group=sort --mode=confirm --workload-id=sort-inputs-v1 --scope=linux/performance/sort --baseline=cell-before
```

Replace the scope with your hardware/class/workload identity. Preserve the recorded
mode and plan in each experiment. `--mode=evidence` remains a compatibility spelling
with mandatory scope and the same protocol. It does not automatically schedule
machines, upload results or certify publication readiness.

Keep the per-cell interval, practical margin, failed attempts, build manifest and
input identities. The suite comparison family correction is not an automatic
study-wide multiplicity policy. Define coverage, weights and cross-cell claims
before combining results.

## Probe a fleet worker and package explicit files

```sh
zenbench fleet capabilities
zenbench cpu-info
```

A reported architecture is not always verified native execution. If
`native_arch_verified` is false, independently check the host and executable
architecture; emulation changes the measurement. A capability probe records what
the runner can observe—it is not admission to measure.

Build once for each target/toolchain/features combination. A bundle specification
lists the executable, libraries, symbols, inputs and the driver's declared workload:

```sh
zenbench fleet bundle bundle-spec.json new-bundle
zenbench fleet bundle-check new-bundle
```

Use fresh directories. Destination paths are portable relative paths and each
listed file is hashed. Transport the bundle through the existing scheduler/content
store and verify it on arrival, preserving executable permissions. Undeclared
libraries, drivers and OS services remain testbed context. The
[complete fleet guide](https://github.com/imazen/zenbench/blob/main/docs/fleet.md)
contains the bundle and driver JSON schemas; define those before running these
commands.

## Record one observation per job

The driver must perform the fixed `ZENBENCH_OPERATIONS` count, join all work, and
write its schema-1 correctness/memory record to `ZENBENCH_RESULT_FILE`. After
preparing that driver contract and bundle:

```sh
zenbench fleet run new-bundle --output attempts/unique-01 \
  --experiment study-1 --machine cpu-testbed-1 --domain selected-class \
  --replicate 0 --attempt unique-01 --wait 300
zenbench fleet read attempts/unique-01
```

Replace the study, machine, domain and attempt IDs with your identities. The
receipt binds the bundle, runner, placement, correctness and resource observations.
Output must be a new directory. Keep busy, cancelled, timed-out and wrong-output
attempts; a retry is not a new preplanned replicate.

| Backend | Quantity |
|---|---|
| `wall` | Descriptive supervisor duration, including launch and cleanup |
| `os_peak` | Platform-named resident/private-commit high-water counters |
| `heaptrack` | Linux whole-child intercepted heap traces and global-peak stack contributions |
| `driver` | Explicit instrumented memory records with quantity, coverage and scope |

Select the backend in the bundle plan. Keep a separate uninstrumented latency
binary and [dedicated memory binary](@/profiling.md). Never sum independent worker
peaks, divide a peak by operations or relabel resident memory as heap. Unavailable
counters retain null with a reason.

## Review and export selected observations

```sh
zenbench fleet diff attempts/before attempts/after
zenbench fleet export study.jsonl attempts/before attempts/after
```

`fleet read` verifies retained hashes. `fleet diff` requires compatible complete
observation contracts and reports descriptive deltas, not statistical confirmation.
Exit 0 from a fleet run means the observation completed and correctness matched;
exit 2 means inspect the retained receipt. Neither is a native timing acceptance
verdict. Use [native confirmation](@/ci-regression.md) or a separately planned
[external Confirm experiment](@/external-confirmation.md) for timing acceptance.

The export creates a new compact JSONL file including failed attempts, identities,
packet hashes and decode commands. Commit selected study records and pointers;
keep packets and traces in the content store.

Zenfleet owns deployment, routing, leases and storage. Coordinate local background
work with `zenbench nice` and a shared admission directory. The
[benchmark fleet integration](https://github.com/imazen/zenmetrics/blob/master/docs/BENCHMARK_FLEET.md)
and [heap/fleet contract](https://github.com/imazen/zenbench/blob/main/docs/heap-and-fleet-design.md)
describe scheduler responsibilities and measurement limits. You have completed a
study only when every declared cell has valid retained evidence or a disclosed
unresolved outcome.
