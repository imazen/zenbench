+++
title = "Profile memory and CPU work"
description = "Explain a timing change with a separate diagnostic run."
weight = 9
+++

Use profiling after you have a concrete question: did allocation traffic change,
did a transient heap spike disappear, or did vector code turn into scalar calls?
Keep the profiler, input and build settings identical across the two diagnostic
arms, then confirm elapsed time with a separate native latency build.

## Choose the quantity before choosing the tool

| Question | Tool or API | What the number means |
|---|---|---|
| How many allocation calls/bytes does the measured thread request? | Installed `AllocProfiler` | Allocation traffic on that thread, not peak live memory |
| How much requested Rust heap is live at a phase peak? | Installed `HeapProfiler` and `HeapPhase` | Process-wide requested bytes across Rust allocator threads |
| Where does intercepted heap use come from? | Linux [Heaptrack](https://github.com/KDE/heaptrack) | Allocation traces and stacks for the intercepted child |
| What is the OS resident/private-commit high-water mark? | Fleet `os_peak` backend | Named OS counters with platform-specific coverage |
| Why does CPU execution change? | Assembly, Linux perf, [iai-callgrind](https://docs.rs/iai-callgrind/latest/iai_callgrind/) | Code and event/instruction diagnostics |

Never label RSS as heap, divide a memory peak by iteration count or sum independent
worker peaks. Requested heap, allocator overhead, direct mappings, native-library
memory and GPU memory are different quantities. Null with a reason means unavailable;
zero is a measured zero.

## Allocation traffic in a Rust benchmark

Enabling `alloc-profiling` makes the API available; install it as the benchmark
binary's global allocator to collect counters:

```rust
#[global_allocator]
static ALLOC: zenbench::AllocProfiler = zenbench::AllocProfiler::system();
```

Counter updates are inside allocation/deallocation/reallocation paths and affect
timed samples. Allocation also updates a shared relaxed atomic activity flag;
there is no universal measured overhead figure. Keep instrumentation identical
across arms, and verify native timing separately without it. Counters cover the
measured thread, so zero does not prove an external Rayon pool is allocation-free.

## Requested live heap in a dedicated memory binary

Enable `heap-profiling` for your **memory build** and install `HeapProfiler` as its
global allocator. Do not also install `AllocProfiler`. A small `examples/memory.rs`
control in that build is:

```rust
#[global_allocator]
static ALLOC: zenbench::HeapProfiler = zenbench::HeapProfiler::system();

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let retained = vec![3u8; 4096];
    std::hint::black_box(&retained);

    let phase = zenbench::HeapPhase::begin()?;
    let scratch = vec![1u8; 65536];
    std::hint::black_box(&scratch);
    drop(scratch);
    let memory = phase.finish()?;

    println!("entry={} peak={} exit={}",
        memory.entry_live_bytes, memory.peak_live_bytes, memory.exit_live_bytes);
    drop(retained);
    Ok(())
}
```

Adapt the phase boundary to preparation, scoring or another declared operation;
join all work before finishing it. Entry, peak and exit are absolute live
requested bytes, including inputs/reference state retained from earlier phases.
`peak - entry` is additional live memory above entry, not ownership-attributed
scratch. Freeing earlier allocations can hide later allocations in that delta.

Heap accounting serializes allocation operations across Rust threads, including
existing Rayon workers. This changes scheduling and cost. Native allocators that
bypass Rust's allocator, fragmentation, metadata, direct mappings, hidden realloc
overlap and device memory are outside coverage. Do not use this binary's timings
to claim native latency. [Heap semantics and controls](https://github.com/imazen/zenbench/blob/main/docs/heap-profiling.md)
cover phase errors and the ledger's independent oracle.

## Inspect the code actually measured

For [cargo-show-asm](https://github.com/pacak/cargo-show-asm), the command is
`cargo asm`. Replace the package, target and symbol with yours:

```sh
cargo asm --help
cargo asm -p PACKAGE --bench my_bench --profile bench
cargo asm -p PACKAGE --bench my_bench --profile bench --asm 'kernel'
cargo asm -p PACKAGE --bench my_bench --profile bench --rust --context 2 'kernel'
```

Start with the symbol listing, then choose the actual monomorphization. If a
function was inlined, inspect its containing caller. Match compiler, target,
features, profile, LTO/PGO and flags to the timed build; runtime dispatch should
remain part of the measured operation when callers pay it.

For retained executables, inspect their bytes without rebuilding:

```sh
cargo asm --file ./builds/before/my_bench --disasm 'kernel'
cargo asm --file ./builds/after/my_bench --disasm 'kernel'
```

File disassembly requires cargo-show-asm built with `disasm` and suitable symbols.
Preserve hashes alongside captures. Look for dispatch inside hot loops, bounds
checks, scalarization, spills, repeated loads and conversion work. Address or
register renaming alone does not establish an optimization.

## Collect separate CPU profiles

A Linux perf command template for matched retained executables is:

```sh
perf record -o before.data -e cycles:u --call-graph dwarf -- ./builds/before/my_bench --group=sort
perf record -o after.data -e cycles:u --call-graph dwarf -- ./builds/after/my_bench --group=sort
perf diff before.data after.data
```

Set the same verified [CPU boundary](@/cpu-placement.md) first, use fresh output
paths and keep event/workload settings identical. A collection permission error
is not an empty valid profile. Whole-process samples include setup, reporting and
teardown outside an internal kernel timer. A hotspot's percentage can rise because
another part became faster; compare absolute work and time too.

Instruction counts and assembly explain a mechanism but do not establish latency
on your target CPU. Heaptrack and iai-callgrind are complementary external tools,
not integrated timing backends. Retain profiler outputs with binary/input identity
and keep them separate from confirming timing evidence.

## Check the complete operation again

Repeat correctness over the declared domain, including tails, strides, alignment,
rounding and overflow semantics. For SIMD, distinguish held-token kernel cost,
public runtime dispatch and end-to-end work. Run A/A controls, then
[freeze and confirm](@/ci-regression.md) the uninstrumented application path.
The [SIMD workflow](https://github.com/imazen/zenbench/blob/main/docs/simd-optimization.md)
and `zenbench guide --topic simd` link retained experiments and tooling details.
