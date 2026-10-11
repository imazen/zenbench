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
Multiple processor groups and Windows ARM class discovery are unsupported and
fail closed by default. Start a fresh process to change classes; unavailable
classes cannot be selected from a previously narrowed mask.

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

`examples/cpu_probe.rs` runs that example. The sampler retains the caller's
affinity and QoS; apply any hints before invoking it. Configuration bounds
sample counts and work per batch, and every combined worker must use identical
configuration. Callers control simultaneity and which workers participate;
oversubscribing one CPU can create large ratios even with enforced placement.
Repeat probes between measurement windows to observe later scheduling changes.
The current diagnostic reports observations and does not set an automatic
timing threshold or infer a performance/efficiency class from timings.
