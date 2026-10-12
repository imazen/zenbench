# CLI and API migration from 0.1.10

Use `zenbench guide` first. Benchmark binaries accept both `--name=value` and
`--name value`; Cargo forwards them after `--`. The primary CLI has nine entries:
`guide`, `cpu-info`, `exec`, `pair`, `nice`, `self-compare`, `data`, `fleet`, `jobs`.

## Commands and decisions

| Task | Preferred command | Meaning of success |
|---|---|---|
| Local alternatives in a Rust suite | `cargo bench --bench NAME -- --mode iterate` | Execution completed; exploratory |
| Frozen suite candidate | `cargo bench --bench NAME -- --mode confirm --workload-id ID` | Valid confirming decision; a single-arm population makes no relative claim |
| Named study cell | Add `--scope HOST/CLASS/WORKLOAD` to Confirm | Same protocol, explicit scope; no fleet-wide certification |
| Separate executables/environments | `zenbench pair PLAN.json` | Planned exploratory execution completed; no confirmation verdict |
| Compare saved suite evidence | `zenbench data compare BEFORE AFTER` | Every case establishes non-inferiority |
| Compare saved external A/B packets | `zenbench data pair-diff BEFORE AFTER` | Descriptive comparison completed |
| Compare fleet observations | `zenbench fleet diff BEFORE AFTER` | Compatible descriptive comparison completed |
| Background convenience | `zenbench jobs run NAME` | Legacy launch/status contract, not a performance decision |

For suite confirmation and `data compare`, exit 0 means accepted evidence,
1 means a confirmed regression, and 2 means invalid/inconclusive. For `pair`,
zero means execution only and failures/not-run return 2. Reading, exporting,
and descriptive diffing cannot promote exploratory measurements into confirmed
results. `nice` and `exec` return the launched command's status when it runs.

`self-compare` remains a source-build convenience. It runs the old source then
the current working tree and compares their suite packets. Default exploratory
packets return comparison exit 2. Its help explicitly describes that limitation.
`jobs run --wait` retains its older wait/exit behavior; use the benchmark directly
for confirmation. No additional general runner abstraction is introduced here.

The existing `--mode=evidence` and `BenchmarkMode::Evidence` remain supported
with their required scope. New callers can use Confirm with scope. Their recorded
mode identities are retained: do not switch spellings within one experiment or
assume historical packets with different plans are interchangeable.

## Existing scripts

All previous spellings remain accepted. Help presents the canonical paths:

| Flat command | Canonical path |
|---|---|
| `read`, `compare`, `pair-diff` | `data read`, `data compare`, `data pair-diff` |
| `archive`, `snapshot`, `reanalyze`, `baseline` | Same command under `data` |
| `fleet-run`, `fleet-read`, `fleet-diff`, `fleet-export` | `fleet run`, `fleet read`, `fleet diff`, `fleet export` |
| `capabilities`, `bundle`, `bundle-check` | Same command under `fleet` |
| `run`, `list`, `status`, `kill`, `results`, `clean` | Same command under `jobs` |

For example, `zenbench results latest` still works; new documentation calls it
`zenbench jobs results latest`. `data read` reads an evidence path, while
`jobs results` resolves background-job IDs such as `latest` and offers its existing
chart output. They share result types but have different input lookup contracts.
Embedded decode commands in historical evidence remain valid aliases.

## Library consumers

The published `BaselineComparison` retains its exact constructible fields and
`compare_against_baseline` return type. Its arithmetic counts are descriptive.
Use `baseline::assess_baseline` for a separate non-exhaustive `BaselineAssessment`
containing `confirmed`, `validity_issues`, `promotable` and the descriptive
`comparison`. CLI promotion uses this assessment. `ResourceGate` retains its
published unwind traits. CI checks the published API in addition to platform tests.

The cleanup retains the diagnostic probe and QoS APIs. Probing actual external
workers is an existing use case (`examples/cpu_probe.rs`), so removing those types
or adding another feature switch would trade a useful capability for migration
work. Probes report observations; they never certify placement or maximum clocks.
For Rayon, start with a verified process boundary, not `Suite::affinity` alone.

## Results and platform changes

Default Iterate changes from 200 maximum rounds/500 ms warmup to 30/100 ms per arm.
Confirm uses fixed independent processes. These settings are budgets, not runtime
speedup predictions. Raw evidence lives in ignored durable storage; export selected
small summaries for Git. See [storage](result-storage.md).

On asymmetric or unknown CPUs, inspect `cpu-info` and configure verified placement.
Linux requires a suitable cpuset; native Windows ARM64 now supports OS core/cache
classification and inherited Job confinement within one processor group. Windows
multiple groups and macOS hard P/E binding remain unsupported. Explicit
unrestricted exploration cannot authorize confirmation.

Old 0.1.10 and current builds use different default lock locations; upgrading one
participant does not make mixed versions coordinate automatically. Avoid concurrent
mixed-version benchmark runs. See [placement](process-affinity.md),
[workflow guidance](benchmark-workflows.md), and [methodology](../METHODOLOGY.md).
