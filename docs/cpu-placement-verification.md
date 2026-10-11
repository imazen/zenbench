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

## Native macOS QoS with a separate warmup

A later native probe on the same Apple M4 Pro tested both pthread QoS requests
using `just mac-qos-probe <trusted-ssh-alias>`. Each fresh worker saved its prior
requested QoS, applied interactive (33) or background (9) at relative priority
zero, and verified the readback. Both workers restored their prior default
class (21) and relative priority zero after the experiment.

Before any sample timer, each worker exercised the identical native PBKDF2
workload for at least 100 ms of measured thread CPU time. Twelve timed calls
then produced the following observations, recorded separately:

| Requested QoS | Warmup thread CPU time (ns) | Warmup wall time (ns) | Median sample (ns) |
|---|---:|---:|---:|
| Interactive | 100225292 | 100226750 | 297375 |
| Background | 101606333 | 101744209 | 1913146 |

These are single sequential observations, interactive then background. They
provide no repeated-run uncertainty, actual core IDs, or frequency measurements.
Requested QoS readback does not reveal scheduling overrides or certify P/E
placement. The native Python oracle exercises the same pthread ABI; it is not
an execution of the compiled Rust crate. Native Rust guard tests run in CI.

The [raw observations](../benchmarks/macos_qos_warmup_2026-10-11.json) and
[command, source, and resource record](../benchmarks/macos_qos_warmup_2026-10-11.meta.json)
preserve both classes and every sample. Native `/usr/bin/time -l` measured
22,396,928 bytes maximum resident set size. The interpreter ran niced with a
CPU-time limit and RSS monitoring, without a native hard memory cap. The local
SSH-launcher wrapper separately reported:
`run-heavy: done rc=0 0s | peak-RSS 0.01GiB | min-avail 25835MiB | peak-load 0.67`.
Its memory cap and peak RSS describe the local launcher only.

The Rust worker probe now performs its own separate 100 ms compute exercise
(default; configurable from 50 to 1000 ms), outside its integer/cache sample
timers, and records elapsed warmup and requested QoS. macOS engine startup and
Criterion compatibility also exercise the coordinator for 100 ms before
calibration. Opaque external pools must warm their actual workers separately.
All CPU evidence rows retain `cpu_frequency_verified=false`; these exercises
do not establish maximum frequency or replace fail-closed admission.

## Native asymmetric-cache inventory

A read-only sysfs query on an AMD Ryzen 9 9950X3D on 2026-10-11 reported online
CPUs `0-31` and the following last-level-cache domains:

| Reported L3 size | Shared logical CPU IDs |
|---|---|
| `32768K` (32 MiB) | `8-15,24-31` |
| `98304K` (96 MiB) | `0-7,16-23` |

These CPU assignments are observations on that installation. Resolve them
again before selecting CPUs; numbering is not a portable CCD identifier. The
query read `level`, `size`, and `shared_cpu_list` under
`/sys/devices/system/cpu/cpu*/cache/index*/` through an existing trusted SSH
connection, without a benchmark workload or compiled Rust execution.

The Linux Rust discovery path already distinguishes L3 `size` classes. Its
equal-core-capacity 96M/32M fixture verifies that a mixed process boundary is
refused, named performance selection cannot choose between equal-ranked cache
classes, and a confined explicit CPU set is admitted. This native inventory
confirms the hardware exposes unequal sizes; it does not measure confinement
or performance on the 9950X3D. `shared_cpu_list` was observed by the inventory,
but cache-domain identities are not currently separate Rust metadata fields.

Select each detected cache class in a fresh process and retain its CPU set and
cache-class label in results. Do not combine classes or infer the faster CCD
from cache capacity. Equal-size caches with different sharing/placement remain
a separate discovery limitation.
