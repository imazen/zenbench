# `zenbench-driver` — cross-OS-process aggregation driver

`zenbench-driver` is a small binary that launches a `cargo bench`
command **N separate times, each in its own OS process**, collects the
JSON `SuiteResult` from each child, and aggregates them via
`zenbench::aggregate_results`. It complements — it does not replace —
the in-process `run_passes` / `--{best,mean,median}-of-passes=N`
entry point.

## What attacks what kind of noise

Rounds, passes, and processes attack *different* sources of variance.
A benchmark tool that doesn't pay attention to which is which will
report numbers with whichever kind of noise it couldn't reset — and
since the numbers look just as tight on either side, you won't notice
until you re-run and they bounce.

| Source of variance | `max_rounds` (within pass) | `run_passes` / `--*-of-passes=N` (within process) | `zenbench-driver --processes=N` (cross process) |
|---|:--:|:--:|:--:|
| **Timer quantization** | ✅ | ✅ | ✅ |
| **Per-sample OS interrupt** during measurement window | ✅ | ✅ | ✅ |
| **Single-sample cache misses** on hot data | ✅ | ✅ | ✅ |
| **Calibration / iteration-count estimate** | ❌ (one per run) | ✅ rerun per pass | ✅ rerun per child |
| **Warmup state** in the hot loop | ❌ (warmup runs once) | ✅ re-warmed per pass | ✅ re-warmed per child |
| **Heap addresses** of benchmark test data | ❌ (one malloc) | ✅ re-allocated each pass | ✅ new heap per child |
| **Data-dependent branch history** | ❌ | ✅ data moved → re-trained | ✅ |
| **CPU frequency / turbo / thermal state** | ❌ | ❌ (same process) | ✅ fresh per `execve` |
| **ASLR layout for code pages** | ❌ | ❌ (set at startup) | ✅ new per `execve` |
| **Kernel scheduler affinity / NUMA** | ❌ | ❌ (same PID) | ✅ re-decided per child |
| **Page cache residency** for binary mappings | ❌ | ❌ (same mappings) | ✅ |
| **Branch predictor tables** at hot code addresses | ❌ | ❌ (same code pages, same history) | ✅ |
| **Background contention** on co-tenant cores | depends on external timing | depends on external timing | partial — captures variation between child launches |

The key observation: the green ticks for `run_passes` and
`zenbench-driver` are a *strict superset* of the ticks for
`max_rounds`, and `zenbench-driver`'s ticks are a strict superset of
`run_passes`. More rounds inside one pass cannot attack anything
passes can't already attack, and more passes inside one process
cannot attack anything a cross-process driver doesn't already attack.

This also means the diagnostic "benchmark reports `mad ±0.1%` within
a run but bounces `±10%` between cargo bench invocations" is
exactly the **process-level variance** signature. `run_passes` does
not fix it; `zenbench-driver` does.

If you don't care about those (say, you're sanity-checking a micro-
optimization on a dev machine and want fast iteration), use
`cargo bench -- --best-of-processes=3` instead. It's faster because it
skips the per-child cold-start cost.

## How it flows, end to end

```
┌─────────────────┐   spawn w/ ZENBENCH_RESULT_PATH=/tmp/zenbench-driver-<id>-0.json
│ zenbench-driver │──────────────────────────────────────────────────┐
└─────────────────┘                                                  │
         │                                                           ▼
         │                                            ┌──────────────────────────┐
         │                                            │ cargo bench --bench foo  │
         │                                            │  ↓                       │
         │                                            │ bench binary (child)     │
         │                                            │  ↓ postprocess_result()  │
         │                                            │ result.save(path)        │
         │                                            └──────────────────────────┘
         │                                                           │
         │     wait, read JSON, delete temp file                     │
         │◄──────────────────────────────────────────────────────────┘
         │
         │ repeat N times (sequentially — not in parallel)
         │
         ▼
  aggregate_results(vec![r1, r2, ..., rN], policy)
         │
         ▼
  Print aggregated report (stderr terminal form + stdout formatted form)
```

Concretely:

1. Driver parses `--processes=N`, `--policy=best|mean|median`,
   `--format=llm|csv|md|json`, and collects the rest of `argv` after
   `--` as the child command (typically `cargo bench ...`).

2. For `i` in `0..N`:
    * Pick a unique temp path: `/tmp/zenbench-driver-<run-id>-<i>.json`
    * Spawn the child with
      `ZENBENCH_RESULT_PATH=<path>` and `ZENBENCH_FORMAT=json` in the
      environment. The child's stdin is `/dev/null`, stdout is
      discarded, stderr is inherited (so you see per-process progress).
    * `wait()` for the child. Non-zero exit → driver exits 1 and cleans
      up any temp files already created.
    * Read the JSON back via `SuiteResult::load(path)`. Missing file or
      parse error → same failure path.

3. After all N children succeed, delete the temp files (before the
   report step — so a panic in aggregation doesn't leak files).

4. Call `zenbench::aggregate_results(results, policy)`. This is the
   **same** function `run_passes` calls internally, so the in-process
   and cross-process drivers produce directly comparable summaries.

5. Print the aggregated report:
    * Header + groups + footer to stderr via
      `SuiteResult::print_report()`.
    * One additional formatted dump to stdout in the requested format
      (`llm`/`csv`/`md`/`json`).

## Why sequential, not parallel

Running N children in parallel would defeat the purpose: they'd fight
over the same cores and thermal headroom. The driver runs one child at
a time, waits for completion, then starts the next.

## Error handling

* **Child spawn fails** (`ENOENT`, permission, etc.): driver exits 1,
  prints which child index failed, cleans up any temp files already
  written by earlier successful children.
* **Child exits non-zero**: same.
* **Child exits 0 but no result file was written**: same. This catches
  benchmarks that forget to call `zenbench::main!` / `run()`, or crash
  after results should have been saved.
* **Result JSON fails to parse**: same.

There is no retry logic. If one trial is contaminated, re-run the whole
command with a fresh `--processes=N`. The driver is not in the business
of deciding which samples to keep.

## Args forwarded verbatim

Everything after `--` is passed as-is to the child. The usual patterns:

```
zenbench-driver --processes=5 --policy=best -- cargo bench --bench sorting
zenbench-driver --processes=5 --policy=mean -- cargo bench --bench sorting -- --group=sort
zenbench-driver --processes=5 --policy=median --format=md -- ./target/release/my-bench
```

Yes, the `--` may appear twice — once to separate driver args from the
child command, and again inside `cargo bench`'s own syntax to forward
extra args to the harness. The driver stops parsing at the **first**
`--` and copies the rest unchanged.

## Relationship to the in-process API

The driver deliberately does **not** reimplement aggregation. It calls:

```rust
pub fn aggregate_results(
    runs: Vec<SuiteResult>,
    policy: Aggregation,
) -> SuiteResult
```

This is the same function `run_passes` calls after collecting its N
in-process results. If you want to build your own driver (e.g., a shell
loop over hosts on a cluster), you can:

1. Run `cargo bench` N times with `ZENBENCH_RESULT_PATH=<path>` set
   differently each time.
2. `SuiteResult::load` each JSON.
3. Call `zenbench::aggregate_results(results, policy)`.
4. `result.print_report()` or `result.to_llm()`, etc.

That's all `zenbench-driver` is under the hood.

## When `run_passes` is enough

* You're iterating on a micro-optimization and want sub-second feedback.
* The benchmarks are fast (< 1s each) and noise is dominated by OS
  interrupts, not CPU frequency.
* You already have a warm page cache from a recent build and don't
  care about cold-start bias.

## When to reach for `zenbench-driver`

* Publishing performance numbers, not just iterating.
* Benchmark changes something ASLR- or CPU-frequency-sensitive (e.g.
  branch-predictor-heavy hot loops).
* Comparing before/after on a shared CI runner where neighbor noise
  can bleed across trials in the same process.
* You want the reported `median of 10 processes` to actually mean
  "10 cold-started OS processes", not "10 Engine objects inside one
  process".
