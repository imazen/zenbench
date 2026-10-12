# Verified CPU placement

Zenbench checks CPU placement before calibration, warmup, or timed callbacks.
The default `auto` policy accepts known homogeneous hardware, or an OS process
boundary containing one known CPU class. Asymmetric or unknown topology without
that evidence aborts the run. A calling-thread mask, `ZENBENCH_PIN`, `taskset`,
and macOS Mach tags cannot satisfy that check.

Inspect the detected classes and the current admission result:

```sh
zenbench cpu-info
```

Choose a class before starting the benchmark program and its libraries:

```sh
zenbench exec --cpu-policy performance ./my-benchmark -- --format=llm
zenbench exec --cpu-policy efficiency ./my-benchmark -- --format=llm
```

Run those commands as separate processes to obtain separately labeled P/E
results. Persistent global pools then cannot retain the earlier class's workers.
Zenbench refuses to aggregate or baseline-compare different process sets,
classes, enforcement modes, or observed coordinator masks. `auto` versus an
explicit class request can compare when their actual placement agrees.

A class with unequal last-level cache capacities has separate entries, even if
its cores share a performance rank. Zenbench does not infer which cache class is
faster. Select explicit IDs and an `auto` policy:

```sh
zenbench exec --cpu-policy auto --cpu-set=0-3 ./my-benchmark
```

Bench binaries accept `--cpu-policy=auto|performance|efficiency|unrestricted` and
`--cpu-set=0,2-4`, with equivalent `ZENBENCH_CPU_POLICY` and `ZENBENCH_CPU_SET`
environment variables. CLI values override environment values. A crate's
registration function can set the suite defaults:

```rust
use zenbench::{Suite, platform::CpuPolicy};
fn configure(suite: &mut Suite) {
    suite.cpu_policy(CpuPolicy::Performance).cpu_set(&[0, 1]);
    suite.affinity(&[0]); // scoped coordinator/zenbench-worker default
}
```

Suite CPU settings override CLI/environment settings; explicit group affinity
requests override suite affinity, which overrides `ZENBENCH_PIN`. IDs must be
chosen from `cpu-info`, rather than assumed to identify P/E cores. Suite policy
checks run before measurement. Criterion-compatible immediate mode checks
CLI/environment policy before registration callbacks execute; later suite
settings cannot change those completed measurements. Launch-time placement is needed to cover
initialization and already-created external descendants.

The explicit opt-out is `--cpu-policy=unrestricted`, or
`ZENBENCH_CPU_POLICY=unrestricted`. It cannot be combined with a requested
process CPU set. Every LLM/CSV benchmark row then states
`UNVERIFIED_UNRESTRICTED` and `cpu_class_uniform=false`; this is permission to
measure without placement certification. Historical results remain
`LEGACY_UNKNOWN`. CPU evidence is also saved in JSON and displayed in terminal,
Markdown, and HTML reports. Reserved CPU evidence keys in user tags are
namespaced, and strings are escaped so embedded newlines cannot forge rows.

## Linux

The launcher reuses a matching, already verified process set when available.
Otherwise it requests systemd's
[`AllowedCPUs`](https://www.freedesktop.org/software/systemd/man/latest/systemd.resource-control.html#AllowedCPUs=)
through `systemd-run --user --scope`, then the child verifies the actual
[`cpuset.cpus.effective`](https://docs.kernel.org/admin-guide/cgroup-v2.html#cpuset)
before it executes the program. A delegated/enabled cpuset controller is required.
A configured cpuset from another launcher, container runtime, or service also
works; library admission verifies the effective set.

A cpuset restricts every thread of the benchmark process, including existing
private Rayon pools, new threads, and new child processes. A worker cannot widen
its thread affinity beyond the effective cpuset. Existing child processes or
remote services are outside this boundary and need separate enrollment.

Accepting an `AllowedCPUs` property is insufficient. A 2026-10-11 user-scope
probe accepted CPU 0 but had no cpuset controller in the user manager. The caller
and a new worker could still widen affinity to CPUs 0 through 19. The implemented
child check rejects that situation before launching the program. Zenbench does
not change ancestor controller configuration or require administrator privileges
silently.

Class discovery uses Linux hybrid PMU CPU maps, reported CPU capacities, and
safe CPUID queries where available. It distinguishes reported L3 cache sizes.
Incomplete mappings remain unknown, though an enforced set entirely inside a
known mapped class can pass `auto`. P/E selection requires a complete ranking;
AMD power-efficiency identity is not treated as a performance ranking. Hardware
that hides its heterogeneity cannot be certified beyond the OS/CPU information
it exposes.

## Windows and macOS

Windows x86/x64 uses safe CPUID queries and a restrictive
[`SetProcessAffinityMask`](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-setprocessaffinitymask)
mask. Explicit P/E or CPU-set selection applies to the process for its lifetime;
it is not restored after a group. The mask is checked again outside timed rounds.
Admission also assigns the process to an affinity-limited
[Windows Job Object](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_basic_limit_information)
using the published safe `win32job` API. The job remains alive for the process;
its CPU boundary applies to existing threads and future child processes. A
library's attempt to change the process mask cannot widen this job boundary.
Windows may return success while retaining the restricted mask, so readback
of the actual mask is the confinement oracle. Job creation/assignment failure
aborts admission before calibration or callbacks.
Native Windows ARM64 discovers OS core efficiency/cache classes and uses the
same inherited Job boundary. Multiple processor groups remain unsupported and
fail closed by default. Start a fresh process to change classes; unavailable
classes cannot be selected from a previously narrowed mask.

`examples/windows_topology_probe.ps1` is a separate read-only native API oracle:
it reports `GetLogicalProcessorInformationEx(RelationAll)` core efficiency
classes, processor groups, and cache sizes/sharing masks. It does not change
affinity, certify placement, or supply topology to the Rust backend. Run it in
64-bit PowerShell; it reports its process architecture so emulation is visible.
The Windows x64/ARM CI jobs execute the oracle before Rust tests. These
[core](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-processor_relationship)
and [cache](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-cache_relationship)
records are the OS-level inputs needed to extend Windows discovery.

For repeatable remote observations, `just windows-topology-remote WINDOWS_ALIAS`
runs the script through existing SSH credentials. Optional `--relay HOST`,
`--network-jump HOST`, and `--known-hosts FILE` separate the credential host,
connection route, and an already trusted host pin. Host verification remains
enabled; private keys stay on the credential host. The helper sends a compressed
PowerShell command without writing remote files. An earlier stdin transport
using `Console.ReadToEnd` timed out; the command transport completed on native
Windows. This is a diagnostic transport, not a benchmark launcher.

`examples/windows_process_mask_probe.ps1` independently checks the process-mask
primitive with an already-created managed thread and managed pool worker. It
requires one processor group, checks rejected attempts to widen worker masks,
and restores the original process and worker masks before returning. Run it
with `--script examples/windows_process_mask_probe.ps1 --cpu-id ID` through the
same SSH helper; choose IDs from a fresh native topology query. The Windows CI
steps choose the first available CPU and require an outside CPU for the
negative control. This is an OS primitive oracle, not compiled zenbench/Rayon
execution or a benchmark workload.

macOS detects `hw.nperflevels` and performance-level counts. Its
[Mach affinity policy](https://developer.apple.com/library/archive/releasenotes/Performance/RN-AffinityAPI/)
is advisory and cannot bind P/E classes. Therefore an asymmetric macOS machine
requires the explicit unverified opt-out; it cannot produce certified class
results with this backend. Unsupported targets also require an opt-out.

## Placement, concurrency, and measurement validity

The process boundary determines where work runs. It does not reserve CPUs from
other processes, equalize boost clocks, or constrain an external library's thread
count. The launcher sets `RAYON_NUM_THREADS` to the selected CPU count as a
suggestion; [explicit Rayon pool sizes](https://docs.rs/rayon/latest/rayon/struct.ThreadPoolBuilder.html#method.num_threads)
can override it. Opaque pool thread counts remain `pool_thread_count=unknown`.
The resource gate remains relevant for unrelated load.

On macOS the engine and Criterion-compatible immediate path perform a separate
100 ms compute warmup before calibration and timing metrics. This warms the
coordinator, without running user callbacks or claiming to warm opaque pool
workers. Existing group warmup still exercises the benchmark's own work.
Demand can affect clocks, but maximum frequency is not guaranteed or measured;
CPU evidence states `cpu_frequency_verified=false`.

Zenbench rechecks an enforced boundary before and after measured rounds and
before returning results; observed changes abort the run. These checks assume
ordinary library workers do not change process policy or cgroup membership.
A privileged controller can change and restore a boundary between observations;
Zenbench does not claim protection against that. Benchmark code must keep any
intentional nested thread-affinity changes scoped and restored.

`examples/cpu_policy.rs` checks a pre-existing private Rayon pool and an ordinary
thread, including their attempts to widen affinity, then runs a tiny benchmark.
It requires an enforced single-class process set. No large sweep is involved.

Measured confinement and negative controls are recorded in
[CPU placement verification](cpu-placement-verification.md).

## Worker warmup observations

Fixed-work probes complement placement checks. A worker that remains slower
across repeated batches differs from a worker with isolated slow batches.
Neither observation alone identifies its cause: CPU class, SMT contention,
clock changes, or unrelated load can affect both. Similar times cannot prove
that workers obeyed a scheduling hint.

Inspect new diagnostic threads without running a benchmark:

```sh
zenbench cpu-info --probe --workers 2
```

The JSON keeps admission evidence separate from the probe. Each worker records
dependent integer and private 64 KiB pointer-chase batches, its observed affinity
before/after, and Linux's last CPU at batch boundaries. Summaries show medians,
MAD, within-worker max/min ratios, and between-worker median ratios. CPU IDs
are unavailable on other platforms, and migration between observations is
unknown. The report always states `certifies_cpu_class=false`; it has no
"passed" status and cannot weaken default CPU admission.

Those new threads do not inspect an existing pool. To observe the actual
workers of a private Rayon pool, execute the probe inside that pool:

```rust,no_run
use zenbench::platform::{CpuProbeConfig, CpuProbeReport, probe_cpu_thread};
let pool = rayon::ThreadPoolBuilder::new().num_threads(2).build().unwrap();
let workers = pool.broadcast(|_| probe_cpu_thread(CpuProbeConfig::default()))
    .into_iter().collect::<Result<Vec<_>, _>>().unwrap();
let report = CpuProbeReport::from_workers(workers).unwrap();
println!("{}", serde_json::to_string_pretty(&report).unwrap());
```

Every probe worker now performs its own separate compute warmup before samples:
`CpuProbeConfig::warmup_ms` defaults to 100 and accepts 50 through 1000 ms.
The report retains the actual warmup elapsed time. Reports with missing or
short warmup evidence cannot be combined as current warmed observations.

For native macOS QoS, request a preference inside each actual worker:

```rust,no_run
use zenbench::platform::{ThreadQos, set_current_thread_qos, probe_cpu_thread};
let guard = set_current_thread_qos(ThreadQos::UserInteractive).unwrap();
let observation = probe_cpu_thread(Default::default()).unwrap();
drop(guard); // restores the prior requested class and relative priority
```

Use `ThreadQos::Background` in a separate measurement to request background
scheduling. These call
[`pthread_set_qos_class_self_np`](https://github.com/apple-oss-distributions/libpthread/blob/main/include/pthread/qos.h)
at relative priority zero. Querying observes the requested QoS, not effective
overrides or P/E binding. The guard is bound to its thread and rejects a prior
unspecified/unrestorable class before mutation. Other targets return Unsupported.
The macOS-only FFI stays in the existing narrowly allowed backend.

The Rayon example accepts `--qos=interactive` or `--qos=background` before any
positional CPU IDs, and makes the request inside each pool worker. CPU-ID Mach
hints remain a separate mechanism and are unsupported on the tested M4 Pro.
These preference-only observations do not satisfy strict CPU admission.

`examples/cpu_probe.rs` runs that example. Optional positional CPU IDs let it
apply a separate scoped mask/hint to each worker; inspect `cpu-info` to choose
IDs. Build diagnostics with optimization to reduce debug overhead:

```sh
cargo run --release --example cpu_probe
# Two specific IDs selected from cpu-info:
cargo run --release --example cpu_probe -- 0 8
```

The sampler retains the caller's
affinity and QoS; apply any hints before invoking it. Configuration bounds
sample counts and work per batch, and every combined worker must use identical
configuration. Callers control simultaneity and which workers participate;
oversubscribing one CPU can create large ratios even with enforced placement.
Repeat probes between measurement windows to observe later scheduling changes.
The current diagnostic reports observations and does not set an automatic
timing threshold or infer a performance/efficiency class from timings.

### Native Windows repeatability controls

`examples/windows_repeatability.rs` executes integer, dependent-cache, and
private Rayon workloads in fresh processes. The pool exists before admission;
workers attempt to widen their masks after admission and record the resulting
masks. Each group includes two identical workloads, 10% additional work, and
twice the work. Work count is the controlled input; elapsed time need not scale
linearly, especially with thread-pool overhead or cache effects.

Build with `just windows-build-image` followed by
`just windows-build-example windows_repeatability`. These recipes use a local
container with a Windows GNU cross-linker. Invoke through the host's heavy-work
wrapper. No native CPU compiler flags are used.

`just windows-repeatability-upload <trusted-alias> [SSH options]` stages the
hash-named executable using SFTP. `just windows-repeatability-remote
<trusted-alias> 'default;0,2;performance;efficiency;0,1;2,3' 5 [SSH options]`
then verifies its SHA-256 before execution. Replace CPU IDs using your topology
inventory. The default and mixed-set entries are refusal controls on the
verified hybrid test machine; they are not portable CPU IDs. Each repetition
alternates configuration order. There is no shared-process aggregation across
classes.

The native PowerShell runner assigns itself to a Windows
[Job Object](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects)
before launching children, with a 2 GiB process/job committed-memory cap, idle
priority, and termination when its last job handle closes. Each child has a
90-second deadline. Peak RSS comes from
[GetProcessMemoryInfo](https://learn.microsoft.com/en-us/windows/win32/api/psapi/nf-psapi-getprocessmemoryinfo);
peak job committed memory is a separate measurement. The local Linux wrapper's
RSS describes the SSH launcher rather than the native Windows workload.

`just windows-repeatability-analyze <capture.log> <summary.json>
--topology benchmarks/windows_topology_2026-10-11.json` checks placement,
worker masks, complete rounds, randomized order, known slower controls, and
class selection against an independent Windows SDK inventory. The inventory
must describe the actual test machine. It reports process-to-process spread
and identical-work comparisons without deleting noisy runs. Engine outlier
counts remain visible.

These experiments explicitly disable resource admission, retain that fact in
round evidence, and warm actual pool workers separately for 100 ms. They do
not establish a quiet machine, maximum frequency, or accuracy for arbitrary
workloads. Per-run confidence intervals are not uncertainty intervals for
changes in hardware, process layout, or machine state between fresh runs.

Measurement-order shuffles receive an independent seed per group and process.
The retained execution-order vectors describe the realized schedule. Bootstrap
resampling keeps a deterministic seed so analyzing the same observations remains
repeatable.

The `windows_job_boundary` integration fixture runs in isolated native Windows
processes, including Windows ARM CI. It creates its Rayon and ordinary workers
before installing the affinity job, attempts process/thread escape, and checks
an inherited descendant process. This tests the OS boundary independently of
CPU class discovery. The separate `windows_arm_topology` fixture now validates
native ARM64 discovery and selected-class admission against the OS oracle.

The unchanged `gate_sees_load` regression can also run natively with
`just windows-gate-test-build` and `just windows-gate-test-remote HOST BINARY`.
It exercises the CPU monitor's first snapshot and the resource gate while
workers generate load, then drops those workers. The remote runner requires
the named test to execute and retains its status, native peak RSS, and job cap.
This is separate from the explicitly ungated repeatability protocol above.

`just windows-msrv-check` type-checks all Windows targets and the features that
claim Rust 1.85 support using that toolchain. CI also performs this Windows
check on Rust 1.85.0; the Linux MSRV check alone does not compile Windows-only
dependencies or backend code.

### Native Windows heap and fleet controls

From a revision-identified source tree on a Windows worker, run
`just windows-native-check` (or its PowerShell command from the justfile).
This checks topology, existing-thread/process-mask restoration, exact requested
heap accounting, inherited Job Object confinement (including existing Rayon
workers and descendants), and the fleet CLI. The final capability probe removes
the functional fixtures' unrestricted-policy override so its output exposes the
actual default placement verdict.

The runner uses one Cargo build job, disables compiler debug symbols and
incremental state, and sets idle priority and a 3 GiB inherited Job Object memory
cap. It requires 4 GiB available before starting, retains a local
transcript, and reports peak job commit bytes separately from heap measurements.
PowerShell's execution-policy override applies only to the command's process;
the recipe does not change the machine's policy. These are correctness controls,
not a repeatability study or evidence of production enrollment.

Windows ARM64 discovers every active core's Windows efficiency class and
last-level data/unified cache size through `GetLogicalProcessorInformationEx`.
It preserves intermediate performance ranks and unequal cache sizes. A missing
or inconsistent core/cache inventory, multiple processor groups, oversized
response or repeatedly changing query size refuses placement. Acquisition uses
a scoped FFI exception; parsing and allocation/retry logic use safe Rust.

Windows ARM64 CI compares the discovered classes against the independent native
PowerShell oracle, then starts fresh processes for automatic policy, both ranked
policies and every explicit class. Those controls create Rayon workers before
placement and assert the actual masks after attempts to widen them.
