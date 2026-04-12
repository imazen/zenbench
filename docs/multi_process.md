# Cross-OS-process benchmark aggregation

`--best-of-processes=N` re-executes the benchmark binary **N separate
times, each in its own OS process**, and aggregates the per-process
`SuiteResult`s via `aggregate_results`. This complements the in-process
`--best-of-passes=N` entry point.

## Usage

```bash
cargo bench -- --best-of-processes=5
cargo bench -- --mean-of-processes=3
cargo bench -- --median-of-processes=10

# Composable with passes: 3 processes × 2 passes = 6 total runs
cargo bench -- --best-of-processes=3 --best-of-passes=2
```

## What attacks what kind of noise

Rounds, passes, and processes attack *different* sources of variance.

| Source of variance | `max_rounds` (within pass) | `--*-of-passes=N` (within process) | `--*-of-processes=N` (cross process) |
|---|:--:|:--:|:--:|
| **Timer quantization** | ✅ | ✅ | ✅ |
| **Per-sample OS interrupt** | ✅ | ✅ | ✅ |
| **Single-sample cache misses** on hot data | ✅ | ✅ | ✅ |
| **Calibration / iteration-count estimate** | ❌ | ✅ | ✅ |
| **Warmup state** in the hot loop | ❌ | ✅ | ✅ |
| **Heap addresses** of benchmark test data | ❌ | ✅ | ✅ |
| **Data-dependent branch history** | ❌ | ✅ | ✅ |
| **CPU frequency / turbo / thermal state** | ❌ | ❌ | ✅ |
| **ASLR layout for code pages** | ❌ | ❌ | ✅ |
| **Kernel scheduler affinity / NUMA** | ❌ | ❌ | ✅ |
| **Page cache residency** for binary mappings | ❌ | ❌ | ✅ |
| **Branch predictor tables** at hot code addresses | ❌ | ❌ | ✅ |

The key diagnostic: "benchmark reports `mad ±0.1%` within a run but
bounces `±10%` between `cargo bench` invocations" is exactly the
**process-level variance** signature. `--best-of-passes` does not
fix it; `--best-of-processes` does.

## How it works

The `main!` macro detects `--best-of-processes=N` and:

1. Gets `std::env::current_exe()` — the already-compiled benchmark binary.
2. Strips process flags and post-processing flags (`--format=`, `--save-baseline=`, etc.) from child args.
3. Spawns N children sequentially, each with:
   - `ZENBENCH_SUBPROCESS=1` (recursion guard)
   - `ZENBENCH_RESULT_PATH=<temp file>` (JSON output)
   - `ZENBENCH_LAUNCHER_PIDS=<parent PID>` (gate fix)
4. Reads each child's JSON result.
5. Calls `aggregate_results(results, policy)`.
6. Prints the aggregated report and runs `postprocess_result` (baseline save/compare, format output).

No separate binary needed. No recompilation per process.

## When passes are enough

* Iterating on a micro-optimization and want sub-second feedback.
* Benchmarks are fast (< 1s each) and noise is dominated by OS interrupts.
* You already have a warm page cache from a recent build.

## When to use processes

* Publishing performance numbers.
* Branch-predictor-heavy or ASLR-sensitive hot loops.
* Comparing before/after on shared CI runners.
* You want "best of 10 processes" to mean 10 cold-started OS processes.
