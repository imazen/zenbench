# Requested heap peaks

Enable `heap-profiling` in a dedicated memory binary and install
`zenbench::HeapProfiler::system()` as its `#[global_allocator]`. This profiler
wraps `System`; it is separate from the thread-local `AllocProfiler` used for
allocation traffic in timing benchmarks.

```rust
#[global_allocator]
static ALLOC: zenbench::HeapProfiler = zenbench::HeapProfiler::system();

let input = prepare_input();
let phase = zenbench::HeapPhase::begin()?;
let reference = prepare_reference(&input);
let preparation = phase.finish()?;

let phase = zenbench::HeapPhase::begin()?;
let output = score(&reference, &input); // Join all work before finish.
let score_memory = phase.finish()?;
```

Entry, peak and exit are absolute live requested bytes across all Rust threads,
including existing Rayon workers and cross-thread frees. Phase boundaries never
zero the live ledger. Reference/input allocations retained from preparation
remain visible during scoring. `peak_live_bytes - entry_live_bytes` is additional
live memory above entry, not ownership-attributed scratch. A free of existing
memory can hide a later allocation in that difference. A second active phase is
an error; dropping a phase releases it without returning a measurement.

One lock orders the system allocation operation, its accounting and every
snapshot/boundary. The lock and counters allocate nothing. Alloc/realloc failures
leave the live count unchanged; successful realloc changes the old size to the
new size. `requested_bytes` is successful allocation sizes plus successful
realloc new sizes, not net growth. Counter overflow/inconsistent accounting
invalidates the result instead of wrapping. The global snapshot also exposes the
lifetime peak, which is never reset.

These are instrumented requested-byte peaks. Serialization changes scheduling
and cost. Do not time this binary to claim native latency. Physical allocator
metadata/fragmentation, hidden realloc overlap, direct mappings, C-library
allocations that bypass Rust's global allocator and device memory are excluded.
Use external heaptrack and named OS resident/commit counters for those different
views. Never add their byte counts together or divide a peak by iterations.

The example `heap_phases` exercises transient spikes, retained allocations,
simultaneous versus sequential workers, cross-thread frees, an existing Rayon
pool and realloc growth/shrink. `--exercise-oom` additionally makes fallible
impossible-size requests in a 64-bit process; callers choose that control
explicitly. The allocator's implementation exception is limited to forwarding
unchanged `GlobalAlloc` pointer/layout arguments to `System`; phase/counter code
is safe Rust.

`just heap-check` runs the controls. `just heap-oracle NEW_OUTPUT_DIRECTORY`
checks the process lifetime ledger against heaptrack's Rust allocator
contributions at its global peak, using exact integer bytes. It retains the
trace and stack export outside tracked source; it fails if the profiler is
missing or its stack export lacks the allocator frames. This is a control of
the instrumented execution, not a latency or real-world workload claim.
