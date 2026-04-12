# `zenbench-driver` — cross-OS-process aggregation driver

`zenbench-driver` is a small binary that launches a `cargo bench`
command **N separate times, each in its own OS process**, collects the
JSON `SuiteResult` from each child, and aggregates them via
`zenbench::aggregate_processes`. It complements — it does not replace —
the in-process `run_processes` / `--{best,mean,median}-of-processes=N`
entry point.

## Why another driver?

Rounds, warmups, and re-instantiated `Engine`s inside **one** OS process
can only reset so much noise:

| Noise source | In-process `run_processes` | `zenbench-driver` |
|---|---|---|
| Round/iteration count (calibration) | reset per process | reset per child |
| Engine internal state | reset per process | reset per child |
| Cache working set | mostly displaced by warmup | fully cold on each `exec` |
| Branch predictor / BTB | partly reset by warmup | fully reset on each `exec` |
| ASLR layout | **fixed for the whole run** | **new per child** |
| CPU frequency / C-state at startup | **already warm** | **cold per child** |
| Kernel scheduler affinity decisions | **sticky for the lifetime of the process** | **re-decided per child** |
| Page cache from prior workloads | **same for every trial** | **may differ per child** |

The first block of rows is what `run_processes` already handles. The
second block — ASLR, CPU frequency, scheduler — is what this driver adds.
If you care about those, run your benchmark in separate processes.

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
  aggregate_processes(vec![r1, r2, ..., rN], policy)
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

4. Call `zenbench::aggregate_processes(results, policy)`. This is the
   **same** function `run_processes` calls internally, so the in-process
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
pub fn aggregate_processes(
    processes: Vec<SuiteResult>,
    policy: ProcessAggregation,
) -> SuiteResult
```

This is the same function `run_processes` calls after collecting its N
in-process results. If you want to build your own driver (e.g., a shell
loop over hosts on a cluster), you can:

1. Run `cargo bench` N times with `ZENBENCH_RESULT_PATH=<path>` set
   differently each time.
2. `SuiteResult::load` each JSON.
3. Call `zenbench::aggregate_processes(results, policy)`.
4. `result.print_report()` or `result.to_llm()`, etc.

That's all `zenbench-driver` is under the hood.

## When `run_processes` is enough

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
