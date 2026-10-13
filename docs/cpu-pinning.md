# Thread pinning

Start with the [basic benchmark setup](https://docs.rs/zenbench/latest/zenbench/).
Use this guide when you need a specific thread mask or an external worker pool.
For process-wide CPU classes on asymmetric hardware, see
[verified CPU placement](https://docs.rs/zenbench/latest/zenbench/platform/index.html#verified-cpu-placement). A thread mask does not
certify a process-wide CPU class.

## Pin a comparison group

Choose logical CPU IDs from the machine you are measuring, then set
`g.affinity(&[2, 3])` for that group. The scope
covers warmup, iteration estimation, every measured round, and zenbench-created
parallel/contention workers. It restores the calling thread's prior affinity when
the group ends, including panic unwinding. Existing external thread pools retain
their own affinity; configure those pools separately.

## External thread pools

Rayon parallel iterators use Rayon's current pool. Configure that pool's worker
affinity and thread count explicitly, and execute the benchmark through
[`ThreadPool::install`](https://docs.rs/rayon/latest/rayon/struct.ThreadPool.html#method.install).
Hold each worker's `AffinityGuard` for its entire worker lifetime, for example
around `thread.run()` in a
[`spawn_handler`](https://docs.rs/rayon/latest/rayon/struct.ThreadPoolBuilder.html#method.spawn_handler).
An existing pool is unaffected by group pinning; a global pool initialized during
one group keeps its worker configuration for subsequent groups.


## Harness defaults

For a whole harness, set `ZENBENCH_PIN=2-7` or `ZENBENCH_PIN=0,2-4,7` before
launching it. Ranges are inclusive, IDs are zero based, and the environment is
parsed once per process. A group's explicit setting overrides the environment.
Empty sets, malformed syntax, unavailable CPUs, and OS errors abort the benchmark
instead of producing results with unapplied affinity.

## Manual scopes and platform limits

For a manual scope, use `zenbench::platform::pin_current_thread(&[2])?` and keep
the returned `AffinityGuard` alive. `current_thread_affinity()` queries the mask.
Linux enforces the entire requested set; Windows IDs belong to the calling
thread's current processor group. macOS maps each CPU set to one opaque
[Mach affinity tag](https://developer.apple.com/library/archive/releasenotes/Performance/RN-AffinityAPI/):
this is a best-effort cache-sharing hint, **not a binding to those CPUs**. The guard
restores the prior tag; hardware that rejects the hint returns an error. macOS
has no CPU-mask query, so `current_thread_affinity()` returns `Unsupported` there.
Other targets return `AffinityError::Unsupported` for pinning too.
See [the pinned example](https://github.com/imazen/zenbench/blob/main/examples/pinned.rs)
(`ZENBENCH_CPU_POLICY=unrestricted cargo run --example pinned`) for
before/during/after scoped mask checks on an available CPU. This example opts out
of process-class certification explicitly.

