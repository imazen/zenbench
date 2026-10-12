# Fleet deployment

`zenbench fleet capabilities` emits schema-1 JSON with the installed executable's
SHA-256, version, process architecture, reported machine architecture, CPU
classes, and placement result. A failed placement check is retained as an error;
a successful capability probe is not permission to measure. The reported machine
architecture can fall back to process architecture: `native_arch_verified:false`
means a worker must independently check native versus emulated execution.

Build once per target/toolchain/features combination. Package only explicit
executables, libraries, symbols and inputs:

```json
{"schema":1,"target":"x86_64-unknown-linux-gnu","plan":{"input":"data/input"},
 "files":{"bin/metric":{"source":"target/release/metric","executable":true},
          "data/input":{"source":"inputs/reference","executable":false}}}
```

```sh
zenbench fleet bundle bundle-spec.json deployed-bundle
zenbench fleet bundle-check deployed-bundle
```

Source paths resolve beside the specification. Destination paths are portable
relative paths; the manifest contains hashes, lengths and executable flags, not
source paths. Copy the directory using the fleet's existing content store.
Verify it on arrival. Preserve executable permissions when transporting it.
The bundle manifest's SHA-256 binds its target, logical plan and file identities.
Changing the staging directory does not change that identity. Unlisted dynamic
libraries, drivers and OS services remain part of the separately recorded testbed.
A failed bundle creation retains its partial directory and never overwrites one.

The scheduler owns worker selection, leases, retries and storage. A benchmark
identity must include the intended machine/testbed, CPU domain and planned
replicate. Retry attempts remain separate evidence. Do not collapse measurements
across machines or select the fastest retry. See the
[heap and fleet contract](heap-and-fleet-design.md) for the measurement semantics
and the implemented scope and remaining deployment validation.

## One observation per job

The bundle's `plan` for `fleet-run` is schema 1:

```json
{"schema":1,"program":"bin/metric","args":["--case","warm"],
 "env":{"RAYON_NUM_THREADS":"8"},"backend":"heaptrack",
 "operations":1,"timeout_seconds":300,"expected_correctness":{"score_hash":"EXPECTED"},
 "workload":{"metric":"NAME","input":"data/input","reuse":"warm-reference"}}
```

The driver performs exactly `ZENBENCH_OPERATIONS` operations and writes JSON to
`ZENBENCH_RESULT_FILE` after joining its work:

```json
{"schema":1,"operations":1,"correctness":{"score_hash":"EXPECTED"},"memory":[]}
```

```sh
zenbench fleet run deployed-bundle --output attempts/unique-attempt \
  --experiment study-1 --machine cpu-testbed-1 --domain selected-class \
  --replicate 0 --attempt unique-attempt --wait 300 --cancel cancel.request
zenbench fleet read attempts/unique-attempt
```

Output must be a new directory. The receipt binds bundle, runner, identities,
correctness, CPU placement and before/after resource observations. `events.jsonl`
gets a heartbeat during admission and execution. A cancel file stops waiting or
kills/reaps the child process group/Windows Job before releasing admission.
Unix workloads must remain in that group and join their children. Abnormal
supervisor death leaves an orphan reservation that fails closed.

`wall` records descriptive supervisor wall time, including launch and cleanup.
It is not an in-process latency estimate or confirmation result. `os_peak` uses
Linux GNU time's `ru_maxrss`, converted from KiB to integer bytes, or macOS
`/usr/bin/time -l`'s maximum resident set size in bytes. Windows reads the child
handle's `PeakWorkingSetSize` and `PeakPagefileUsage` before closing it, reporting
resident and private-commit high-water counters separately. Those are OS
counters, not heap, and not a simultaneous sum over a process tree. Windows
query failures retain null values with reasons. A missing Unix time tool refuses
the observation; it never substitutes sampled RSS.
`heaptrack` runs the Linux recorder and analyzer and retains their file hashes,
trace, summary and exact integer global-peak stack contributions. Allocation
contributions are all from the same global peak; independent call-site peaks
would not be additive. The transient allocation control caught heaptrack 1.5.0's
Massif export missing the spike, so that export is not used. See the upstream
[second-pass peak accounting](https://github.com/KDE/heaptrack/blob/v1.5.0/src/analyze/accumulatedtracedata.cpp)
and [stack export](https://github.com/KDE/heaptrack/blob/v1.5.0/src/analyze/print/heaptrack_print.cpp).

`driver` accepts an explicitly instrumented driver's `memory` records. Each has
`quantity`, integer-or-null `bytes`, `unavailable_reason`, `backend`,
`backend_version`, `coverage` and `scope`. A null value requires a reason; zero is
an actual zero. Driver claims are retained, not independently certified. Use a
separate latency binary without allocator instrumentation. The runner inherits
its worker environment plus explicit overrides; the receipt does not claim a
hermetic environment or dump inherited secrets.

Exit 0 means the observation completed and correctness matched. Exit 2 retains
busy, unsupported placement, cancellation, timeout, wrong-output, identity-change
or incomplete evidence. These outcomes are not performance regressions. Never
retry until a fast sample appears. Large traces stay in the fleet content store;
`packet.json` is a compact, readable receipt with hashes and decoding commands.

## Read, compare and commit small records

`fleet-read` verifies every retained artifact against the packet before decoding.
`fleet-diff LEFT RIGHT` requires complete observations with identical plans,
runner, machine and placement contracts. Binary bundle hashes may differ; the
output names both. It preserves integer byte deltas, including values above
floating-point exact-integer range. Unavailable counters remain null. This is a
descriptive comparison of individual observations, never statistical confirmation.
Use `zenbench pair` for latency comparisons with replicated timing evidence.

```sh
zenbench fleet diff attempts/before attempts/after
zenbench fleet export study.jsonl attempts/before attempts/after
```

The export creates a new JSONL file with one compact, readable record per attempt,
including failures, identities, plan, memory counters, packet hash and decoding
commands. Commit that file rather than thousands of raw trace files. Keep the
hash-addressed packets and traces in the fleet store; the summaries do not replace
raw evidence. Exports never overwrite an existing file.
