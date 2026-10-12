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

## Native hybrid Windows topology inventory

On 2026-10-11 the read-only PowerShell oracle ran on an Intel Core Ultra 7
270K Plus, Windows 11 Pro `10.0.26200`, with a native X64 process. It reported
24 physical cores, 24 logical processors, and one processor group. The Windows
core records exposed two efficiency classes with the following group-0 IDs:

| Efficiency class | Interpretation | Logical CPU IDs |
|---|---|---|
| 1 | Higher intrinsic performance | `0,1,10,11,12,13,22,23` |
| 0 | Lower intrinsic performance | `2-9,14-21` |

These are observations on this installation. CPU numbering does not identify
core class; resolve the map again rather than assuming contiguous P/E ranges.
The oracle reported one 36 MiB L3 cache shared by all 24 logical CPUs. Full
L1/L2 records and their sharing masks are retained alongside the core records.

[Raw native records](../benchmarks/windows_topology_2026-10-11.json) and
[source, command shape, and resource scope](../benchmarks/windows_topology_2026-10-11.meta.json)
preserve the query. `just windows-topology-remote <trusted-windows-alias>` runs
the oracle, with optional relay/jump/trusted-pin arguments for the existing SSH
configuration. The local wrapper reported:
`run-heavy: done rc=0 2s | peak-RSS 0.02GiB | min-avail 26796MiB | peak-load 0.32`.
This resource record covers the local launcher only.

The query changed no affinity and performed no benchmark workload. It is not
compiled Rust execution or a P/E process/Rayon confinement experiment. The
Rust backend still uses per-CPU CPUID on supported Windows x86/x64; ARM class
discovery and multiple-group placement remain unsupported. Separately, the
PowerShell oracle passed native Windows x64 and ARM64 CI at `4e393caa`, along
with the rest of that revision's CI suite.

## Native Windows process-mask controls

The native primitive oracle ran in separate diagnostic processes on the same
Core Ultra 7 270K Plus, selecting CPU 0 from class 1 and CPU 2 from class 0.
Each process created an ordinary managed thread and a managed pool worker
before restriction. Both workers initially had mask `0xFFFFFF`.

| Selected CPU | Process/worker restricted mask | Worker observed CPUs | Widening result |
|---|---|---|---|
| 0 (class 1) | `0x1` | `0,0` | Both rejected with error 87 |
| 2 (class 0) | `0x4` | `2,2` | Both rejected with error 87 |

`SetThreadAffinityMask` rejected each attempt to restore the full original mask
while the process was restricted. `GetProcessAffinityMask` and the scoped
thread-mask query matched the selected set. The oracle then restored the
process mask and both workers' original masks, verifying every readback before
returning. No expectations were reduced and no benchmark workload ran.

[Native observations](../benchmarks/windows_process_mask_2026-10-11.json) and
[source and command record](../benchmarks/windows_process_mask_2026-10-11.meta.json)
preserve both runs. Local launcher summaries were:

```text
run-heavy: done rc=0 2s | peak-RSS 0.02GiB | min-avail 26786MiB | peak-load 0.29
run-heavy: done rc=0 1s | peak-RSS 0.02GiB | min-avail 26790MiB | peak-load 0.48
```

These verify the Windows process/thread affinity primitives on a real hybrid
CPU with preexisting managed workers. They are not native compiled Rust/Rayon
execution; the Windows Rust backend uses the corresponding safe `affinity`
wrapper, whose crate tests run separately in CI. This experiment does not
establish cross-group placement or Windows ARM class discovery.

## Native Rust Windows repeatability and inherited boundaries

The final repeatability source is `95e4042c9e58bc53070b352d36a4a65f93293885`,
built with Rust 1.99.0, LLVM 23.1.1, and the GNU Windows cross linker. The
local build image was resolved with `docker image inspect zenbench-windows-build`
to `sha256:c6b67715ac784d2dcf9308cf555163f69954e03e84a8b8a4167962bb888a495c`.
These executables ran natively on the previously inventoried Core Ultra 7
270K Plus; this extends the earlier primitive-only observations.

Strict admission now retains an affinity-limited Windows Job Object. Native
Rust controls created Rayon workers before admission, attempted to widen both
process and worker masks, and verified actual masks throughout setup. Windows
returned success for some process widen requests while retaining the job mask;
the actual mask, rather than the return code, is the confinement oracle. A
separate isolated fixture verified an ordinary existing thread and an inherited
child process, plus scoped guard restoration.

Each timer series used five fresh processes for each of four placements, with
class order reversed on alternate repetitions. Each process retained 200
interleaved rounds for integer, cache, and two-worker Rayon workloads. Four
benchmark arms did identical work twice, 10% more work, and double work.
Separate worker warmup and 16 independent QPC batches preceded sampling.
All 40 successful processes retained independent execution schedules; all
20 default/mixed-set controls refused before measurement. All 120 paired
identical-work verdicts remained below the existing practical significance
gate, and all 240 known work-increment comparisons cleared it in the correct
direction. More work does not imply an equal percentage increase in latency,
especially for Rayon calls with scheduling overhead.

Fresh-process range below is `100 × (largest median − smallest median) /
median(process medians)`, using every process. It is descriptive variability,
not a confidence interval. Each cell contains integer / cache / Rayon ranges.

| Placement | TSC ranges (%) | QPC-only ranges (%) |
|---|---:|---:|
| Full P class | 1.429 / 1.030 / 2.904 | 1.552 / 5.400 / 2.441 |
| Full E class | 0.046 / 0.410 / 1.166 | 0.134 / 0.552 / 4.599 |
| P pair `[0,1]` | 0.170 / 0.967 / 0.732 | 0.117 / 1.530 / 2.969 |
| E pair `[2,3]` | 0.180 / 0.539 / 0.187 | 0.114 / 0.371 / 0.911 |

Raw identical-work 95% intervals excluded zero in 9 of 60 TSC comparisons
and 5 of 60 QPC comparisons, despite none clearing the existing ±1% practical
threshold. These controls support the practical verdicts at this workload
scale; they do not establish nominal 95% coverage for sub-percent effects.
Per-process intervals do not capture the full fresh-process variability.

On the fixed pairs, integer medians differed from the independent QPC reference
by −0.110% to +0.111% with TSC, and −0.104% to +0.037% with QPC-only timing.
Full-P results differed by as much as +1.769%; no cause was established.
For this tested conference path, retain explicit CPU sets, separate P/E results,
and fresh-process repeats of the actual workload. A shared core class does not
certify identical instantaneous speed.

The repeatability series explicitly disabled resource admission. An additional
execution of the unchanged `gate_sees_cpu_load` regression passed natively: the
first monitor snapshot observed generated CPU load, and the resource gate
refused the busy system. This is a regression control, not proof that the
repeatability runs were quiet. The runner read back its inherited 2 GiB native
process/job memory caps and idle priority before launching work. Native maximum
working sets were 18,812,928 bytes (TSC), 18,386,944 bytes (QPC), and 21,970,944
bytes (load-gate fixture). Local launcher resource records were:

```text
TSC: run-heavy: done rc=0 115s | peak-RSS 0.02GiB | min-avail 26859MiB | peak-load 0.83
QPC: run-heavy: done rc=0 115s | peak-RSS 0.02GiB | min-avail 26010MiB | peak-load 3.12
Gate: run-heavy: done rc=0 7s | peak-RSS 0.02GiB | min-avail 26852MiB | peak-load 0.88
```

These wrapper RSS values cover the local SSH launcher; the native working sets
above were read through GetProcessMemoryInfo.

[Full TSC rounds and source pointer](../benchmarks/windows_repeatability_job_tsc_2026-10-11.pointer.md),
[TSC aggregate summary](../benchmarks/windows_repeatability_job_tsc_2026-10-11.summary.json),
[full QPC rounds and source pointer](../benchmarks/windows_repeatability_job_qpc_2026-10-11.pointer.md),
and [QPC aggregate summary](../benchmarks/windows_repeatability_job_qpc_2026-10-11.summary.json)
preserve all results, including refusals, outliers, and raw execution orders.
Large raw packets have content-addressed primary and mirrored storage; the
pointers record hashes and resolving locations.

Windows ARM class discovery, multiple processor groups, macOS hard P/E binding,
and equal-size cache-sharing domain identity remain unsupported. Maximum CPU
frequency, quietness, and opaque pool size remain unverified. The Windows ARM
Job fixture tests the OS boundary independently of missing core-class discovery.

[Native boundary and load-gate fixture packets](../benchmarks/windows_native_boundary_gate_2026-10-11.json) retain their executable/source hashes, test output, native RSS, and local resource records.

## Native Snapdragon Windows ARM64 controls (2026-10-12)

At `6a798e09`, the native ARM64 build passed requested-heap phase/failure
controls, fleet success/failure/cancellation and named OS memory counters,
and inherited Job Object confinement for existing Rayon workers and descendants.
The independent Windows oracle reports performance classes 0 on CPUs 0–3 and
1 on CPUs 4–11, with three 12 MiB shared L2 domains. These labels are OS reports,
not measured speed ratios.

The capability probe exposed missing ARM64 topology discovery: zenbench reported
unknown and refused default placement. Passing the functional controls does not
certify class selection. Native latency/repeatability was not measured.

[Compact evidence and exact invocation](../benchmarks/winarm_native_controls_2026-10-12.json)
retain the source/binary identities and heap observations. The control used one
build job at idle priority under a 3 GiB process-tree Job Object cap; Windows
reported peak job commit of 2,675,781,632 bytes, a build-envelope counter rather
than heap or RSS. A 2 GiB cap stopped compilation of the Windows API dependency,
including when debug symbols and incremental state were disabled.
