# CPU placement checks — 2026-10-11

Implementation: library `d151aac6`, launcher/example `8e2d6334`.
Native Linux, Intel Core Ultra 7 265K, 20 online logical CPUs. The resolving
command `zenbench cpu-info` reported P-class IDs 0–7 and E-class IDs 8–19 from
Linux PMU maps; these are observations for this CPU, not portable ID assignments.

The default admission check refused the unrestricted 0–19 process cpuset.
Fresh-process tests removed the CI opt-out and confirmed that pinning only the
caller does not permit timed callbacks under rejected process placement.
Invalid policy strings also refused execution. Explicit opt-outs produced
`UNVERIFIED_UNRESTRICTED` rows. Metadata tests rejected cross-class aggregation,
baseline comparison, and changed coordinator masks; missing historical evidence
remained `LEGACY_UNKNOWN`. Embedded newlines in user metadata stayed escaped on
one LLM result line.

## Existing external workers

Build and run the tiny proof example in separate single-class processes:

```sh
just cpu-policy-build
just cpu-policy-container ubuntu:26.04 0
just cpu-policy-container ubuntu:26.04 8
```

Container image digest:
`ubuntu@sha256:f144425ff09be612d6d9ad965196e9cdc23dae1f42110a8a11a3e9a8198759f7`.
The recipe applies a CPU limit and an explicit cpuset. The private Rayon pool
exists before engine startup and explicitly creates two workers. Each worker,
and an ordinary thread, attempts to widen affinity to all detected online IDs.
The OS filters those requests to the enforced process set.

| Process set | Private Rayon worker masks | Ordinary thread mask | Recorded class |
|---|---|---|---|
| `[0]` | `[[0], [0]]` | `[0]` | `performance_l330720K` |
| `[8]` | `[[8], [8]]` | `[8]` | `efficiency_l330720K` |

Both examples reported `cpu_placement=enforced_uniform`, `cpu_enforcement=cpuset`,
and `cpu_class_uniform=true`. Their coordinator masks matched the respective
process set. The tiny five-round benchmark only exercises admission and output;
it is not a performance comparison. The harness's metadata leaves opaque pool
thread counts unknown even though this example knows its own explicit count.

## Launcher negative and positive controls

A user scope accepting `AllowedCPUs=0-7` did not restrict the effective 0–19 set:
cpuset was not enabled in that user manager. This command exited 2 and never
printed the program marker:

```sh
zenbench exec --cpu-policy performance /usr/bin/printf -- PROGRAM_STARTED
```

An existing Docker cpuset satisfied admission and allowed the program marker:

```sh
just cpu-policy-launch-container ubuntu:26.04 0
```

The child printed verified process evidence with requested/effective set `[0]`
before `PROGRAM_STARTED`. The hidden launcher helper separately refused an
explicit `unrestricted` policy, preventing it from calling an opt-out verified.

## Other validation

The full native suite passed with Criterion compatibility and without default
features. Functional/statistical fixtures opt out through caller-visible
configuration; policy subprocess tests remove that opt-out. No expectations or
assertions were reduced. The existing gate test encountered foreign benchmark
activity on early attempts and passed unchanged on a quiet rerun.

Focused policy, serialization, affinity, Criterion compatibility, and chart
checks passed. Linux fixtures exercised unequal L3 cache sizes with equal core
capacities, requiring an explicit set rather than guessing the faster class.
macOS Intel/ARM, Windows x64, and unsupported WASM fallback compile checks
passed; these are compile checks, not runtime affinity measurements. Platform
runtime validation is recorded by the PR's CI jobs.

## Fixed-work worker observations

Sampler implementation `7d15cacf`; CLI `914111a9`. An optimized two-worker
private Rayon probe ran on the detected P/E IDs using:

```sh
just cpu-probe-example 0 8
```

Each worker retained its scoped mask `[0]` or `[8]`; every recorded Linux CPU
observation matched that worker's ID. Twelve equal-work samples per workload
produced different worker medians, plus a slow cache batch within the P worker.
This is a single diagnostic observation, without a confidence interval or an
attribution of the slow batch. It does not establish class identity from timing.

[Raw observations](../benchmarks/cpu_probe_2026-10-11.json) and
[source, compiler, command, and resource record](../benchmarks/cpu_probe_2026-10-11.meta.json)
preserve the measurement. The wrapper reported:
`run-heavy: done rc=0 9s | peak-RSS 0.51GiB | min-avail 23584MiB | peak-load 4.32`.

The separate `just cpu-probe-info` command measured two new diagnostic workers
while retaining the failed default admission for the mixed 0–19 process set.
Both APIs reported `certifies_cpu_class=false`. Fixture tests separately
distinguished persistent speed differences from isolated slow samples and
rejected mismatched work, duplicate worker IDs, and invalid times. The actual
Rayon integration test preserved masks and measured both pool workers.

## Native macOS Mach policy observation

On 2026-10-11, a native Apple M4 Pro running macOS 27.0 reported 12 logical CPUs
and two performance levels containing 8 and 4 CPUs. A direct native Mach probe
using the same ABI and policy calls as `src/platform/affinity.rs` returned
`KERN_NOT_SUPPORTED` (46) from `thread_policy_get(THREAD_AFFINITY_POLICY)`.
The runtime error string was `(os/kern) service not supported`.

The probe did not set a tag after failing to save the prior policy. The Rust
guard follows the same path and returns `AffinityError::Unsupported`. This
tests the native primitive, without claiming an execution of the full Rust
crate or a P/E scheduling experiment. No QoS alternative was tested.

The repeatable oracle is `examples/macos_affinity_probe.py`, invoked with
`just mac-affinity-probe <trusted-ssh-alias>`. It executes from standard input
without writing or building in a remote checkout. The
[raw result](../benchmarks/macos_mach_policy_2026-10-11.json) records the observed
OS, CPU, topology, API error, and explicit absence of placement certification.
