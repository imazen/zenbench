# Cooperative scheduling with `zenbench nice`

Use `zenbench nice -- cargo build --release` for background work on a machine
that also runs benchmarks. Updated zenbench benchmark binaries and `nice` share
a FIFO queue. One admitted job owns the reservation until it finishes. Benchmark
admission precedes CPU warmup and calibration; multi-process experiments retain
one reservation across their children. Nested commands inherit a reservation
only when its live owner is an ancestor, preventing self-deadlock.

This is voluntary scheduling, not a machine-wide CPU isolation guarantee. Every
participating launcher must use the same directory. Unwrapped work still runs,
and benchmark resource monitoring still observes its CPU and memory pressure.
Queued process names are excluded from the rival-benchmark scan; their CPU use
is **not** subtracted from resource observations. Old zenbench versions do not
participate in this queue.

## One user or the whole machine

By default, admission lives in the user's cache directory. For several accounts,
have an administrator create a group-writable directory on a local filesystem,
then configure **every** participating account and service with:

```sh
export ZENBENCH_COORDINATION_DIR=/var/lib/zenbench/coordination
zenbench nice --jobs 8 --memory-mib 8192 -- cargo build --release
```

The accounts need permission to create, read, lock and remove each other's queue
files (including an appropriate shared group and umask). The directory is a
cooperation and trust boundary. Do not place it on a network filesystem. An
explicit invalid directory is an error, never a fallback to a separate queue.
Zenbench does not install a privileged daemon or change another process's priority.

## Limits and fairness

`--wait` bounds queue plus resource admission (default 300 seconds). `--timeout`
bounds the command runtime (default 3600 seconds). New jobs cannot overtake an
already queued job. Jobs are non-preemptive: a long build can delay a benchmark.
Choose a shorter timeout or split large independent jobs when that matters.
Timeout does not give permission to overlap an existing reservation.

Before starting work, `nice` checks for at least 4 GiB available RAM and at most
80% global CPU utilization. These are background-job admission limits, **not**
benchmark cleanliness criteria. Confirm retains its existing stricter checks.

On Linux, `nice` requires a systemd user scope, uses CPU nice level 19 and idle
I/O scheduling, and sets `MemoryMax` plus `MemorySwapMax=0`. The default ceiling
is the smaller of 24 GiB and half physical RAM. Failure to create that scope is
an error. The launcher checks scope completion before releasing its reservation.
On macOS it uses nice level 19; on Windows an inherited Job sets BelowNormal
priority. Those platforms currently have no hard memory ceiling in this command;
requesting `--memory-mib` is rejected there rather than silently ignored.

`--jobs` sets `CARGO_BUILD_JOBS`, `RAYON_NUM_THREADS` and `OMP_NUM_THREADS` for the
child. By default it leaves four available logical CPUs out, with a minimum of
one worker. These variables are cooperative hints; arbitrary programs may ignore
them. They are not a CPU affinity mask or a concurrency enforcement mechanism.

## Child lifetime, failures and recovery

Commands must run in the foreground and join their workers. Unix supervision
uses process groups; programs that detach into another session are outside that
contract. Linux `nice` additionally tracks its systemd scope. Windows supervision
uses a Job with assignment before execution. The launcher terminates remaining
owned group/scope work before admitting the next task. Never use this launcher
as a daemon/service installer.

A crashed owner can leave children alive. A running reservation whose lock is
no longer held therefore blocks new work with an explicit orphan diagnostic.
It is never stolen on age or timeout. Verify the original job and all descendants
have stopped before removing the specific JSON ticket named in the diagnostic.
Do not delete the queue directory or its control lock while clients use it.
A dead waiter that never started a task can be pruned automatically.

Successful launch returns the command's exit code. Admission, supervision or
cleanup errors return 2. Neither success nor scheduling proves a benchmark
conclusion: placement verification, observations and statistical validity still
apply. Use `nice` for background work; a benchmark launched beneath it inherits
its lower priority and must report that execution environment.
