+++
title = "CPU placement and shared machines"
description = "Choose one detected class, configure external pools, and keep background work cooperative."
weight = 6
+++

Start here when a launch refuses placement, when your CPU has unequal core/cache
classes, or when Rayon is part of the operation. A CPU boundary and a quiet
machine address different problems: confinement does not reserve a CPU against
unrelated work.

## Inspect first

```sh
zenbench cpu-info
zenbench exec --help
```

Choose IDs from the detected inventory. More cache does not imply faster execution
for your workload. Keep performance/efficiency or unequal cache classes in
separate launches and results. Unknown topology is a reason to configure or
investigate the host, not to accept a confirming run.

An optional diagnostic creates new workers:

```sh
zenbench cpu-info --probe --workers 2
```

It observes those workers; it does not certify existing pools or maximum clocks.

## Launch inside a verified boundary

If the detected inventory has a usable performance class:

```sh
zenbench exec --cpu-policy performance cargo -- bench --bench my_bench -- --group=sort
```

For unequal cache classes without a unique performance ranking, add
`--cpu-set=IDS`, replacing `IDS` with detected IDs/ranges such as the syntax
`0,2-4`. Do not copy those example CPU numbers without checking your inventory.
For an external experiment, the child is the CLI itself:

```sh
zenbench exec --cpu-policy performance zenbench -- pair plan.json --mode=iterate --dry-run
zenbench exec --cpu-policy performance zenbench -- pair plan.json --mode=iterate --output target/pair-placement-01
```

| Platform | Strict placement requirement |
|---|---|
| Linux | A matching effective hard cgroup cpuset, or a delegated cpuset controller usable by the launcher |
| Supported Windows x86/x64 and native ARM64 | An inherited affinity-limited Job boundary; ARM64 uses OS core/cache classes |
| macOS | Advisory hints cannot certify P/E class confinement |
| Windows with multiple processor groups | Strict class selection is unsupported |

Linux `taskset` and a single thread's affinity do not prevent libraries from
widening worker masks. `ZENBENCH_PIN` is a scoped thread hint, not a replacement
for the process boundary. Group constraints must fit inside it.
[Placement details](https://github.com/imazen/zenbench/blob/main/docs/process-affinity.md)
cover OS setup and native controls.

## Measure Rayon or an existing pool

Use normal `g.bench(...)` callbacks, configure the pool's size explicitly and
wait for all measured work to finish inside the callback. Do not put Rayon work
inside `bench_parallel` or `bench_contended`; those APIs create their own workers.
Initialize a controlled pool after establishing the boundary, or use confinement
that applies to the already-existing workers.

Keep the same worker count and affinity contract for both implementations. A host
CPU count is not necessarily the count available inside your selected class.
For deliberate scaling, choose supported counts in that class; `bench_scaling`
probes host physical/logical counts and does not size an opaque external pool.

## Share a busy workstation

Queue participating background work instead of overlapping it with measurements:

```sh
zenbench nice --jobs 4 -- cargo build --release
```

Updated benchmark binaries and `nice` share a FIFO queue. A long job can delay a
benchmark because scheduling is non-preemptive. `nice` defaults to a 300-second
admission wait and 3600-second command timeout. Unwrapped work still runs and
remains visible to benchmark monitoring; cooperation is not exclusive isolation.

The default queue coordinates one user. For multiple accounts, an administrator
can create a shared local directory with suitable group permissions; configure
all participants with the same `ZENBENCH_COORDINATION_DIR`. See
[cooperative scheduling](https://github.com/imazen/zenbench/blob/main/docs/cooperative-scheduling.md).
Linux `nice` requires a working systemd user scope for its hard memory ceiling.
macOS/Windows currently have no hard memory ceiling in this command; requesting
`--memory-mib` is rejected on them. Worker-count environment hints may be ignored
by arbitrary programs, and are not affinity masks.

## When a measurement is refused or noisy

Use [Iterate](@/iterate.md) for useful directional feedback under ordinary load.
Its retained flags and spread describe uncertainty. Pair's default zero queue
wait reports not-run immediately if another cooperating job owns admission;
`--wait SECONDS` permits bounded waiting.

For Confirm, fix the reported placement, stale observation or foreign-load cause
and retain the failed attempt. Whole-host monitoring cannot prove continuous
quietness on selected CPUs or absence of SMT/cache interference. Reserve a suitable
measurement slot after selecting the candidate.

If verified placement is unavailable, an explicit exploratory opt-out is:

```sh
cargo bench --bench my_bench -- --group=sort --cpu-policy=unrestricted
```

That records unverified exploration and cannot authorize baseline promotion.
Do not weaken confirmation's checks to make a candidate pass. You are ready to
[confirm](@/ci-regression.md) when the intended class is verified, the workload
and pool are fixed, and the required resource observations remain valid.
