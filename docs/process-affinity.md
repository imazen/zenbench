# Process-level CPU placement

Zenbench currently pins the calling thread and zenbench-created workers for each
comparison group. `ZENBENCH_PIN` supplies the default group mask. It does not
establish a process-wide restriction or reconfigure external thread pools.

The proposed process-level policy belongs in the launcher, before benchmark
registration and library initialization. Each benchmark child gets a fixed CPU
set for its lifetime. Group affinity can narrow that set; requests outside it
must fail. Core-class comparisons should use separate children and separately
labeled results, so persistent global pools cannot retain an earlier group's
configuration.

## Linux

A [cpuset cgroup](https://docs.kernel.org/admin-guide/cgroup-v2.html#cpuset)
restricts task placement independently of pool ownership. A domain cgroup covers
all threads of its processes, and newly created child processes inherit their
parent's membership. A worker's affinity request cannot make CPUs outside the
effective cpuset available. Existing worker processes must be enrolled explicitly;
moving the benchmark process does not move its already-existing descendants.

The launcher needs access to an enabled cpuset controller, through delegation or
a system-managed service. It must verify `cpuset.cpus.effective` against the
requested set before executing benchmarks. The requested configuration alone is
not proof of enforcement. Hotplug or parent restrictions can change the effective
set; changed placement must invalidate affected results.

For example, systemd's
[`AllowedCPUs`](https://www.freedesktop.org/software/systemd/man/latest/systemd.resource-control.html#AllowedCPUs=)
configures the cpuset controller. In a 2026-10-11 user-scope probe, the property
was accepted but cpuset was absent from the user manager's available controllers.
After requesting CPU 0 and then widening affinity, both the caller and a new
worker reported CPUs 0 through 19. A strict launcher must reject this condition.

[taskset](https://github.com/util-linux/util-linux/blob/master/schedutils/taskset.c)
before exec is a lighter alternative: Linux threads inherit their creator's
affinity. It does not create a cpuset boundary, so a library can widen its mask
again. Updating all current task masks after pools exist also races thread
creation. Such modes must be reported as inherited affinity, not enforced
process placement.

## Windows and macOS

On Windows,
[`SetProcessAffinityMask`](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-setprocessaffinitymask)
sets the allowed processors for the process's threads; children inherit process
affinity. Processor-group limitations need explicit handling. Process-default
[CPU Sets](https://learn.microsoft.com/en-us/windows/win32/procthread/cpu-sets)
are softer assignments: explicit thread selections override defaults. They are
not equivalent to a restrictive process affinity mask.

The implemented macOS
[Mach affinity policy](https://developer.apple.com/library/archive/releasenotes/Performance/RN-AffinityAPI/)
provides cache-sharing hints, without binding CPU IDs or performance classes.
This backend cannot satisfy an enforced P-core/E-core placement request; strict
class selection must report unsupported rather than label hinted placement as
verified placement.

## Placement and concurrency are separate

A CPU set limits where work executes, not how many threads a library creates.
[`RAYON_NUM_THREADS`](https://docs.rs/rayon/latest/rayon/struct.ThreadPoolBuilder.html#method.num_threads)
controls Rayon's default pool size, but a library's explicit pool size can
override it. Multiple private pools can still oversubscribe the same CPU set.
There is no universal process setting for arbitrary libraries' thread counts.

Reports should record the requested and effective CPU sets, enforcement mode,
core class, and known worker counts. Verify OS restrictions without claiming to
have configured an opaque pool's size. CPU placement also does not reserve CPUs
from unrelated processes; the resource gate remains necessary.
