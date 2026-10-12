# zenbench [![CI](https://img.shields.io/github/actions/workflow/status/imazen/zenbench/ci.yml?style=flat-square&label=CI)](https://github.com/imazen/zenbench/actions/workflows/ci.yml) [![crates.io](https://img.shields.io/crates/v/zenbench?style=flat-square)](https://crates.io/crates/zenbench) [![lib.rs](https://img.shields.io/crates/v/zenbench?style=flat-square&label=lib.rs&color=blue)](https://lib.rs/crates/zenbench) [![docs.rs](https://img.shields.io/docsrs/zenbench?style=flat-square)](https://docs.rs/zenbench) [![MSRV](https://img.shields.io/badge/MSRV-1.85-blue?style=flat-square)](https://doc.rust-lang.org/cargo/reference/manifest.html#the-rust-version-field) [![license](https://img.shields.io/crates/l/zenbench?style=flat-square)](#license) [![codecov](https://img.shields.io/codecov/c/github/imazen/zenbench?style=flat-square)](https://codecov.io/gh/imazen/zenbench)

The CLI installs with Cargo. [Installation instructions](https://github.com/imazen/zenbench/blob/main/docs/installation.md)
cover the current Git build and released versions. Run `zenbench --help` for
offline workflows, `zenbench pair --help` for executable/environment comparisons,
and `zenbench nice --help` for cooperative background jobs.
[Fleet deployment](https://github.com/imazen/zenbench/blob/main/docs/fleet.md) covers capability probing and verified bundles.
[Heap profiling](https://github.com/imazen/zenbench/blob/main/docs/heap-profiling.md) separates memory peaks from latency.

Interleaved microbenchmarking for Rust with paired statistics, CI regression testing, and hardware-adaptive measurement.

**[Documentation](https://imazen.github.io/zenbench)** · **[Example HTML Report](https://imazen.github.io/zenbench/example-report.html)** · **[Tutorial](https://imazen.github.io/zenbench/getting-started/)**

```text
  compress_64k  200 rounds × 67 calls
                       mean ±mad µs  95% CI vs base          iB/s
  ├─ sequential
  │  ├─ level_1        16.2 ±0.5µs  [15.8–16.6]µs          3.78G
  │  ├─ level_6        15.1 ±0.5µs  [-4.7%–-3.5%]          4.05G
  │  ╰─ level_9        15.0 ±0.5µs  [-5.5%–-4.2%]          4.06G
  ╰─ patterns
     ├─ sequential     15.1 ±0.5µs  [-5.8%–-4.4%]          4.03G
     ╰─ mixed         401.0 ±8.1µs  [+2370%–+2385%]         156M

  level_9       ██████████████████████████████████████████████ 4.06 GiB/s
  level_6       ██████████████████████████████████████████████ 4.05 GiB/s
  sequential    █████████████████████████████████████████████ 4.03 GiB/s
  level_1       ███████████████████████████████████████████ 3.78 GiB/s
  mixed         ██ 156 MiB/s
```

## Why zenbench

Zenbench **interleaves**: each round, benchmarks run in shuffled order, keeping
comparison arms close in time. Paired observations help account for shared noise;
they do not guarantee identical clocks, caches or scheduling. Local results are
exploratory. Fixed-process confirmation adds an explicit plan, independent-run
intervals and fail-closed evidence checks.

<!-- crates.io:skip-start -->
### vs criterion and divan

| Feature | criterion | divan | zenbench |
|---|:---:|:---:|:---:|
| **Execution model** | | | |
| Interleaved round-robin | ❌ | ❌ | ✅ |
| Auto-convergence (stop when precise) | ❌ | ❌ | ✅ |
| Resource gating (detect other benchmarks) | ❌ | ❌ | ✅ |
| **Statistics** | | | |
| Bootstrap confidence intervals | ✅ | ❌ | ✅ |
| Paired comparison test | Welch t | ❌ | Wilcoxon |
| Effect size metric | ❌ | ❌ | Cohen's d |
| Drift detection (thermal/load) | ❌ | ❌ | Spearman r |
| Noise threshold (suppress trivial diffs) | ✅ fixed 1% | ❌ | ✅ configurable |
| **Measurement** | | | |
| Hardware TSC timer (rdtsc/cntvct) | ❌ | ✅ opt-in | ✅ auto |
| Overhead compensation | slope regression | loop subtraction | loop subtraction |
| Stack alignment jitter | ✅ alloca (unsafe) | ❌ | ✅ safe trampoline |
| Deferred drop (exclude Drop from timing) | ❌ | ✅ MaybeUninit | ✅ Vec collect |
| Allocation profiling (GlobalAlloc) | ❌ | ✅ | ✅ |
| **CI / Workflow** | | | |
| Save/load baselines | ❌ | ❌ | ✅ `--baseline=` |
| Regression exit codes (0/1/2) | ❌ | ❌ | ✅ |
| Auto-update baseline on pass | ❌ | ❌ | ✅ `--update-on-pass` |
| Multi-pass aggregation (in-process) | ❌ | ❌ | ✅ `--best-of-passes=N` |
| Multi-process aggregation (cross-ASLR) | ❌ | ❌ | ✅ `--best-of-processes=N` |
| Hardware fingerprint / testbed ID | ❌ | ❌ | ✅ |
| Process-level confirmation | ❌ | ❌ | ✅ fixed-plan log-ratio / Welch intervals |
| **Output** | | | |
| Terminal report | table | tree | tree (default) + table |
| Bar chart | ❌ | ❌ | ✅ sorted, throughput |
| JSON / CSV / Markdown | ✅ JSON | ❌ | ✅ JSON + CSV + LLM + MD |
| HTML plots (violin/PDF/regression) | ✅ plotters.rs | ❌ | ❌ |
| HTML report (self-contained, SVG) | ❌ | ❌ | ✅ `--format=html` |
| Streaming per-group | ❌ | ❌ | ✅ |
| Adaptive column layout | ❌ | ❌ | ✅ terminal-width aware |
| **API** | | | |
| Async benchmarks | ✅ to_async() | ❌ | ✅ iter_async() |
| Thread contention testing | ❌ | ✅ threads attr | ✅ bench_contended() |
| Thread scaling analysis | ❌ | ❌ | ✅ bench_scaling() |
| Drop-in criterion migration | — | ❌ | ✅ zero code changes |
| Attribute macros | ❌ | ✅ `#[divan::bench]` | ❌ |
| **Platform** | | | |
| Linux x86_64 / aarch64 | ✅ | ✅ | ✅ |
| Windows x86_64 / ARM64 | ✅ | ✅ | ✅ |
| macOS ARM64 / Intel | ✅ | ✅ | ✅ |
<!-- crates.io:skip-end -->

## Quick start

For the edit → confirm workflow, run `zenbench guide` or
`cargo bench --bench my_bench -- --help`. The [workflow guide](https://github.com/imazen/zenbench/blob/main/docs/benchmark-workflows.md)
covers source revisions, retained executables, CPU/Rayon setup, profiling and small Git exports.
The [measurement protocol](https://github.com/imazen/zenbench/blob/main/METHODOLOGY.md) documents the statistical
methods, busy-machine tradeoffs, limits, and measured wall-time/storage costs.

```toml
# Cargo.toml
[dev-dependencies]
zenbench = "0.1"

[[bench]]
name = "my_bench"
harness = false
```

```rust,no_run
use zenbench::prelude::*;

fn bench_sort(suite: &mut Suite) {
    suite.group("sort", |g| {
        g.throughput(Throughput::Elements(1000));

        g.bench("std_sort", |b| {
            b.with_input(|| (0..1000).rev().collect::<Vec<i32>>())
                .run(|mut v| { v.sort(); v })
        });

        g.bench("sort_unstable", |b| {
            b.with_input(|| (0..1000).rev().collect::<Vec<i32>>())
                .run(|mut v| { v.sort_unstable(); v })
        });
    });
}

zenbench::main!(bench_sort);
```

## Retaining results

Raw results are compressed in the ignored `.zenbench/data/` directory. Export
only selected baselines with `zenbench data snapshot <result> --output benchmarks/baseline.csv`:
one stable row per benchmark, with a hash referencing the complete evidence.
See [storage and compact exports](https://github.com/imazen/zenbench/blob/main/docs/result-storage.md).

## CPU placement and affinity

By default, detected asymmetric or unknown CPU topology requires a verified
process boundary within one CPU class. Otherwise zenbench aborts before
calibration or timed work. Inspect `zenbench cpu-info`, then launch a benchmark
with `zenbench exec --cpu-policy performance ./my-benchmark` (or `efficiency`).
Linux requires an enabled cpuset controller; Windows x86/x64 uses a process mask.
These restrictions cover private Rayon pools and ordinary threads. macOS hints
cannot certify P/E placement.

Use `ZENBENCH_CPU_POLICY=unrestricted` only to opt out explicitly; every result
row records `UNVERIFIED_UNRESTRICTED`. Configure crate-wide policy with
`Suite::cpu_policy` / `Suite::cpu_set`, and scoped defaults with `Suite::affinity`.
See [verified process placement](https://github.com/imazen/zenbench/blob/main/docs/process-affinity.md) for CLI/environment
precedence, platform limits, and separate P/E runs.


Pin a comparison group to logical CPU IDs with `g.affinity(&[2, 3])`. The scope
covers warmup, iteration estimation, every measured round, and zenbench-created
parallel/contention workers. It restores the calling thread's prior affinity when
the group ends, including panic unwinding. Existing external thread pools retain
their own affinity; configure those pools separately.

Rayon parallel iterators use Rayon's current pool. Configure that pool's worker
affinity and thread count explicitly, and execute the benchmark through
[`ThreadPool::install`](https://docs.rs/rayon/latest/rayon/struct.ThreadPool.html#method.install).
Hold each worker's `AffinityGuard` for its entire worker lifetime, for example
around `thread.run()` in a
[`spawn_handler`](https://docs.rs/rayon/latest/rayon/struct.ThreadPoolBuilder.html#method.spawn_handler).
An existing pool is unaffected by group pinning; a global pool initialized during
one group keeps its worker configuration for subsequent groups.


For a whole harness, set `ZENBENCH_PIN=2-7` or `ZENBENCH_PIN=0,2-4,7` before
launching it. Ranges are inclusive, IDs are zero based, and the environment is
parsed once per process. A group's explicit setting overrides the environment.
Empty sets, malformed syntax, unavailable CPUs, and OS errors abort the benchmark
instead of producing results with unapplied affinity.

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

## Iteration and confirmation

Local runs default to exploratory **Iterate**: one process, up to 30 rounds and
100 ms warmup per arm. Use **Confirm** for a frozen candidate; add `--scope=ID`
for a named hardware/workload cell. Confirm uses five fresh processes by default
and refuses incomplete, noisy or unverified evidence. The existing Evidence
spelling remains supported with the same protocol and a required scope.

```sh
cargo bench -- --mode=confirm --workload-id=inputs-v3 --dry-run
cargo bench -- --mode=confirm --workload-id=inputs-v3 --save-baseline=main
cargo bench -- --mode=confirm --workload-id=inputs-v3 --baseline=main --update-on-pass
```

A baseline updates only when every required case establishes non-inferiority
within the declared margin. Missing cases and inconclusive intervals cannot pass.
CPU scope is separate from mode. The CLI and environment override `Suite::plan()`
and suite CPU defaults. See [modes and assumptions](https://github.com/imazen/zenbench/blob/main/docs/benchmark-modes.md) and
[regression testing](https://github.com/imazen/zenbench/blob/main/REGRESSION-TESTING.md).

## Thread scaling

```rust,ignore
suite.group("scaling", |g| {
    g.throughput(Throughput::Elements(10_000));
    g.bench_scaling("work", |b, _tid| {
        b.iter(|| expensive_computation())
    });
});
```

```text
  scaling  200 rounds × 77 calls
                    mean ±mad µs  95% CI vs base    items/s
  ├─ sqrt_1t        4.2 ±0.1µs  [4.2–4.3]µs       2.37G
  ├─ sqrt_2t        4.7 ±0.1µs  [+10.7%–+12.6%]   2.12G
  ├─ sqrt_4t        5.8 ±0.1µs  [+36.0%–+38.8%]   1.72G
  ├─ sqrt_8t        8.5 ±0.3µs  [+91.6%–+101%]    1.17G
  ╰─ sqrt_16t      14.2 ±0.3µs  [+232%–+245%]      703M

  sqrt_1t   ██████████████████████████████████████████████████ 2.37G
  sqrt_2t   █████████████████████████████████████████████ 2.12G
  sqrt_4t   ████████████████████████████████████ 1.72G
  sqrt_8t   █████████████████████████ 1.17G
  sqrt_16t  ███████████████ 703M
```

## Subgroups and organization

```rust,ignore
suite.group("dispatch", |g| {
    g.throughput(Throughput::Elements(100));
    g.throughput_unit("checks");

    g.subgroup("Generic (monomorphized)");
    g.bench("impl Stop (Stopper)", |b| b.iter(|| check_stopper()));
    g.bench("impl Stop (FnStop)", |b| b.iter(|| check_fn()));

    g.subgroup("Dynamic dispatch");
    g.bench("&dyn Stop", |b| b.iter(|| check_dyn()));
    g.bench("StopToken", |b| b.iter(|| check_token()));

    g.baseline("impl Stop (Stopper)");
    g.config().sort_by_speed(true); // native API; the no-arg sort_by_speed() is criterion-compat only
});
```

```text
  dispatch  200 rounds × 10K calls
                                mean ±mad ns  95% CI vs base     checks/s
  ├─ Generic (monomorphized)
  │  ├─ impl Stop (FnStop)      19.7 ±0.3ns  [-49.1%–-47.2%]      5.08G
  │  ╰─ impl Stop (Stopper)     38.5 ±0.5ns  [37.9–39.1]ns        2.60G
  ╰─ Dynamic dispatch
     ├─ StopToken                97.2 ±1.2ns  [+148%–+154%]        1.03G
     ╰─ &dyn Stop              112.5 ±3.1ns  [+176%–+193%]         889M

  impl Stop (FnStop)   ██████████████████████████████████████████████ 5.08G
  impl Stop (Stopper)  █████████████████████████████ 2.60G
  StopToken            ████████████ 1.03G
  &dyn Stop            ██████████ 889M
```

## How to read an A/B result

The `95% CI vs base` column is the bootstrap 95% confidence interval for the
candidate's change against the group `baseline`, shown as `[lo% .. hi%]`
(negative = candidate is faster). The baseline row itself shows `[lo .. hi]ns`.

**When is a difference "significant"?** A change is reported significant only
when **both** of these hold:

1. The 95% CI excludes zero — i.e. `[lo .. hi]` does not straddle 0 (when a
   `noise_threshold` is set, the CI must clear ±threshold of the baseline, not
   just 0).
2. The difference is **above the timer's quantization floor**. If the
   per-iteration difference is smaller than ~2× what the hardware timer can
   resolve, significance is forced to `false` regardless of the CI.

So a CI that excludes zero is **necessary but not sufficient** — a result like
`ci=[-1.33% .. -0.48%]` can still report `significant=false` when the absolute
difference is below the timer resolution (this is the "resolution-limited"
case). The significance test runs on the absolute per-iteration nanosecond CI;
the percentages in the table are that same CI divided by the baseline mean.

zenbench does **not** add a separate p-value cutoff here — the Wilcoxon
p-value and Cohen's d are reported alongside, but the significance flag is
CI-plus-timer-floor as described above.

**Footnotes** flag results you should not over-read:

- `[N] CI [lo% .. hi%] crosses zero — cannot confirm a difference` — the change
  is **not** significant: the interval straddles zero, so the sign of the
  change is unresolved. Treat it as noise, not a real win or regression.
- `[N] real but tiny (effect 0.NN) — unlikely to matter` — significant, but
  Cohen's d < 0.2, so the effect is too small to care about.
- Drift / high-CV / sub-ns / near-timer-resolution footnotes flag noisy or
  unmeasurable conditions; rerun on a quieter machine before trusting them.

## CI regression gate semantics

`--max-regression=N` uses process-level intervals with a declared practical margin.
Exit 1 means a confirmed regression; exit 2 means inconclusive or invalid evidence.
A wide interval cannot pass merely because it fails to detect a regression.
The [shared comparison policy](https://github.com/imazen/zenbench/blob/main/docs/benchmark-modes.md#decisions-and-promotion)
uses fixed independent processes, paired log ratios within a binary and Welch
log-latency intervals across retained baseline populations. The within-process
statistics above remain exploratory.

## Don't accidentally measure nothing

`iter` and `with_input(...).run(...)` automatically pass the closure's return
value (and `run`'s input) through `black_box`, so **returning the result of
your work from the closure is what defeats dead-code elimination** — the
compiler can't prove the value is unused. If your closure returns `()` and the
work has no observable effect, the optimizer can delete it and you'll measure
an empty loop (watch for the `sub-ns with near-zero variance — likely
optimized away` footnote).

Return the value when you can. When you can't (the work is a side effect, or
you want to block a specific input from being const-folded), wrap it explicitly
with `zenbench::black_box` (also in the prelude) or `std::hint::black_box`:

```rust,ignore
g.bench("hashes", |b| b.iter(|| compute_hash(&data)));        // returns → black_boxed for you
g.bench("in_place", |b| b.iter(|| { sort_in_place(&mut buf); black_box(&buf); }));
```

## Throughput is per call

`Throughput::Bytes(N)` / `Throughput::Elements(N)` declare the work done by a
**single** `iter`/`run` invocation — per call, not per round or per sample.
Reported throughput is `N / mean_time_per_call`. If one `b.iter(|| ...)`
compresses a 64 KiB block, set `Throughput::Bytes(64 * 1024)`; the reported
`GiB/s` is then bytes-per-call ÷ the mean per-call time.

Throughput belongs to the **group**: every bench in a group shares it, and a
second `g.throughput(...)` call replaces the first. Benchmarks over different
input sizes therefore need one group per size, not one throughput call per
bench inside a single group.

## Migrating from criterion

Add zenbench alongside criterion — migrate one file at a time:

```toml
[dev-dependencies]
criterion = "0.8"                                          # keep
zenbench = { version = "0.1", features = ["criterion-compat"] }  # add
```

Change one import per file — **zero code changes** to benchmark functions:

```rust,ignore
// Before:
use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId, Throughput};

// After:
use zenbench::criterion_compat::*;
use zenbench::{criterion_group, criterion_main};
```

Closures can borrow local data — no `move` or `Clone` needed. Your existing `criterion_group!`, `criterion_main!`, `bench_function`, `bench_with_input`, `BenchmarkId`, `Throughput`, `group.sample_size()`, `group.measurement_time()`, and `group.finish()` all work unchanged.

Full upgrade ladder: [MIGRATION.md](https://github.com/imazen/zenbench/blob/main/MIGRATION.md)

## Output formats

```bash
cargo bench                           # tree display + sorted bar chart (default, stderr)
cargo bench -- --style=table          # bordered tables with min column
cargo bench -- --format=html          # self-contained SVG report (stdout)
cargo bench -- --format=json          # structured JSON (stdout)
cargo bench -- --format=csv           # spreadsheet-friendly (stdout)
cargo bench -- --format=llm           # key=value for AI tools (stdout)
cargo bench -- --format=md            # markdown tables (stdout)
cargo bench -- --no-busy-gate         # disable the resource gate (or ZENBENCH_NO_BUSY_GATE=1)
```

The default terminal output ends with a sorted, throughput-labelled bar chart
(fastest first) — the right chart for "which is fastest?". `--format=html` writes a
self-contained report (inline SVG bar charts, expandable per-benchmark stats, no
JavaScript or external assets); see the
[example report](https://imazen.github.io/zenbench/example-report.html). For
publication-quality SVG charts (grouped bars, themes) enable the `charts` feature
(`charts-rs`), and the `quickchart` module emits ready-to-embed chart image URLs.

Every run retains a complete gzip packet under `.zenbench/data/` and prints its
evidence reference. `ZENBENCH_DATA_DIR` selects an external durable root.
`ZENBENCH_NO_SAVE=1` disables storage for Iterate only; confirmation requires it. The cross-process lock that keeps
concurrent benchmark runs from overlapping lives in the per-user cache directory
(`~/.cache/zenbench/`, `%LOCALAPPDATA%\zenbench\` on Windows).

## Multi-pass and multi-process

```bash
# In-process passes: resets calibration, warmup, heap addresses
cargo bench -- --best-of-passes=3
cargo bench -- --mean-of-passes=5

# Cross-OS-process: also resets ASLR, CPU freq, scheduler, page cache
cargo bench -- --best-of-processes=3
cargo bench -- --median-of-processes=5

# Composable: 3 processes × 2 passes = 6 total runs
cargo bench -- --best-of-processes=3 --best-of-passes=2
```

Policies: `best` (min mean — use on noisy hosts), `mean` (unbiased average), `median` (robust to outliers). See `docs/multi_process.md` for which noise sources each level attacks.

## API reference

```rust,ignore
use zenbench::prelude::*;

// Interleaved comparison group
suite.group("name", |g| {
    g.throughput(Throughput::Bytes(1024));
    g.subgroup("variant");
    g.bench("impl", |b| b.iter(|| work()));
    g.bench("with_setup", |b| {
        b.with_input(|| make_data()).run(|data| process(data))
    });
    g.bench("deferred_drop", |b| {
        b.iter_deferred_drop(|| Vec::<u8>::with_capacity(1024))
    });
});

// Single function shorthand
suite.bench_fn("fibonacci", || fib(20));

// Thread contention
g.bench_contended("mutex", 4, || Mutex::new(Map::new()), |b, m, tid| {
    b.iter(|| { m.lock().unwrap().insert(tid, 42); })
});

// Automatic thread scaling (probes 1..num_cpus)
g.bench_scaling("work", |b, _tid| b.iter(|| compute()));
```

### Auditable latency samples

`SuiteResult::save` includes `ComparisonResult::samples` for completed engine
rounds. Each `RoundSample` records the actual iteration count, execution order,
and total raw and overhead-compensated nanoseconds in benchmark declaration
order. Warmup and gate waits are excluded. Summaries still use compensated
per-iteration times. Individual-call latency percentiles require **every
sample's actual iteration count to be one**; dividing batched durations does
not recover individual latency tails. Resource admission and representative
inputs must be established separately.

Historical JSON, immediate-mode compatibility results, and multi-run aggregates
have empty samples. Aggregation clears them even for `Best`, since different
arms can win in different runs. Keep original single-run results for paired
analysis; no synthetic round population is inferred from summaries.

Each retained round also has `gate_clean`: `Some(true)` for a clean pre-round
resource check, `Some(false)` for a flagged check, and `None` when disabled or
absent in older JSON. These checks do not monitor the entire timed body. A
strict group's excessive-noise verdict propagates to `SuiteResult::unreliable`,
but strictness permits its configured number of noisy checks; use round status
when requiring every retained round to have passed admission.

## Configuration

```rust,ignore
group.config()
    .max_rounds(200)              // default 30; explicit override
    .noise_threshold(0.02)        // ±2% significance gate
    .bootstrap_resamples(100_000) // CI precision (default 10K)
    .linear_sampling(true)        // slope regression for sub-100ns
    .cold_start(true)             // 1 iter + cache firewall
    .stack_jitter(true)           // random alignment (default on)
    .sort_by_speed(true);         // fastest first in report
```

## Cargo features

| Feature | Default | Adds | Build MSRV |
|---|---|---|---|
| `cli` | yes | the `zenbench` CLI: background runs, results, baselines, self-compare | 1.85 |
| `daemon` | yes | nothing yet: the `daemon` module compiles either way | 1.85 |
| `precise-timing` | yes | TSC / `cntvct_el0` timer and asm fences | 1.85 |
| `alloc-profiling` | yes | `AllocProfiler` for per-benchmark heap counts | 1.85 |
| `criterion-compat` | no | drop-in criterion API for migration | 1.85 |
| `async` | no | `Bencher::iter_async` on a tokio runtime | 1.85 |
| `cpu-time` | no | per-thread CPU time next to wall time | 1.85 |
| `charts` | no | SVG charts via `charts-rs` | 1.88 |
| `wasm` | no | `zenbench::wasm`: wasmtime runner with simd128 and WASI p1 | 1.96 |

The crate's declared `rust-version` is 1.85. `charts` and `wasm` pull in
dependencies that need newer compilers; `zenbench::wasm` re-exports
`wasmtime`, so its major version follows wasmtime's.

## Platform support

Tested on all targets via GitHub Actions CI:

| Platform | Timer | Notes |
|---|---|---|
| Linux x86_64 | TSC (rdtsc) | Full support |
| Linux aarch64 | Counter (cntvct_el0) | Full support |
| Windows x86_64 | TSC (rdtsc) | Full support |
| Windows ARM64 | Instant (~300ns) | No hardware counter in user mode |
| macOS ARM64 | Counter (cntvct_el0) | Full support |
| macOS Intel | TSC (rdtsc) | Full support |

## License

MIT OR Apache-2.0

## Image tech I maintain

| | |
|:--|:--|
| **Codecs** ¹ | [zenjpeg] · [zenpng] · [zenwebp] · [zengif] · [zenavif] · [zenjxl] · [zenjxl-decoder] · [jxl-encoder] · [zenbitmaps] · [heic] · [zentiff] · [zenpdf] · [zensvg] · [zenjp2] · [zenraw] · [ultrahdr] |
| Codec internals | [zenrav1e] · [rav1d-safe] · [zenravif] · [zenavif-parse] · [zenavif-serialize] |
| Compression | [zenflate] · [zenzop] · [zenzstd] |
| Processing | [zenresize] · [zenquant] · [zenblend] · [zenfilters] · [zensally] · [zentone] |
| Pixels & color | [zenpixels] · [zenpixels-convert] · [linear-srgb] · [garb] · [zenyuv] |
| Pipeline & framework | [zenpipe] · [zencodec] · [zencodecs] · [zenlayout] · [zennode] · [zenwasm] · [zentract] |
| Metrics | [zensim] · [fast-ssim2] · [butteraugli] · [zenmetrics] · [resamplescope-rs] |
| Pickers & ML | [zenanalyze] · [zenpredict] · [zenpicker] · [zenanalyze-api] |
| Test corpora | [codec-corpus] · [imazen-26] |
| Products | [Imageflow] image engine ([.NET][imageflow-dotnet] · [Node][imageflow-node] · [Go][imageflow-go]) · [Imageflow Server] · [ImageResizer] (C#) |

<sub>¹ pure-Rust, `#![forbid(unsafe_code)]` codecs, as of 2026</sub>

### General Rust awesomeness

**zenbench** · [archmage] · [magetypes] · [enough] · [whereat] · [cargo-copter] · [zenutils]

[Open source](https://www.imazen.io/open-source) · [@imazen](https://github.com/imazen) · [@lilith](https://github.com/lilith) · [lib.rs/~lilith](https://lib.rs/~lilith)

[zenjpeg]: https://github.com/imazen/zenjpeg
[zenpng]: https://github.com/imazen/zenpng
[zenwebp]: https://github.com/imazen/zenwebp
[zengif]: https://github.com/imazen/zengif
[zenavif]: https://github.com/imazen/zenavif
[zenjxl]: https://github.com/imazen/zenjxl
[zenjxl-decoder]: https://github.com/imazen/zenjxl-decoder
[jxl-encoder]: https://github.com/imazen/jxl-encoder
[zenbitmaps]: https://github.com/imazen/zenbitmaps
[heic]: https://github.com/imazen/heic
[zentiff]: https://github.com/imazen/zenextras
[zenpdf]: https://github.com/imazen/zenextras
[zensvg]: https://github.com/imazen/zenextras
[zenjp2]: https://github.com/imazen/zenextras
[zenraw]: https://github.com/imazen/zenraw
[ultrahdr]: https://github.com/imazen/ultrahdr
[zenrav1e]: https://github.com/imazen/zenrav1e
[rav1d-safe]: https://github.com/imazen/rav1d-safe
[zenravif]: https://github.com/imazen/cavif-rs
[zenavif-parse]: https://github.com/imazen/zenavif
[zenavif-serialize]: https://github.com/imazen/zenavif
[zenflate]: https://github.com/imazen/zenflate
[zenzop]: https://github.com/imazen/zenzop
[zenzstd]: https://github.com/imazen/zenzstd
[zenresize]: https://github.com/imazen/zenresize
[zenquant]: https://github.com/imazen/zenquant
[zenblend]: https://github.com/imazen/zenblend
[zenfilters]: https://github.com/imazen/zenpipe
[zensally]: https://github.com/imazen/zensally
[zentone]: https://github.com/imazen/zentone
[zenpixels]: https://github.com/imazen/zenpixels
[zenpixels-convert]: https://github.com/imazen/zenpixels
[linear-srgb]: https://github.com/imazen/linear-srgb
[garb]: https://github.com/imazen/garb
[zenyuv]: https://github.com/imazen/zenjpeg
[zenpipe]: https://github.com/imazen/zenpipe
[zencodec]: https://github.com/imazen/zencodec
[zencodecs]: https://github.com/imazen/zenpipe
[zenlayout]: https://github.com/imazen/zenpipe
[zennode]: https://github.com/imazen/zennode
[zenwasm]: https://github.com/imazen/zenwasm
[zentract]: https://github.com/imazen/zentract
[zensim]: https://github.com/imazen/zensim
[fast-ssim2]: https://github.com/imazen/fast-ssim2
[butteraugli]: https://github.com/imazen/butteraugli
[zenmetrics]: https://github.com/imazen/zenmetrics
[resamplescope-rs]: https://github.com/imazen/resamplescope-rs
[zenanalyze]: https://github.com/imazen/zenanalyze
[zenpredict]: https://github.com/imazen/zenanalyze
[zenpicker]: https://github.com/imazen/zenanalyze
[zenanalyze-api]: https://github.com/imazen/zenanalyze
[codec-corpus]: https://github.com/imazen/codec-corpus
[imazen-26]: https://github.com/imazen/imazen-26
[archmage]: https://github.com/imazen/archmage
[magetypes]: https://github.com/imazen/archmage
[enough]: https://github.com/imazen/enough
[whereat]: https://github.com/lilith/whereat
[cargo-copter]: https://github.com/imazen/cargo-copter
[zenutils]: https://github.com/imazen/zenutils
[Imageflow]: https://github.com/imazen/imageflow
[Imageflow Server]: https://github.com/imazen/imageflow-dotnet-server
[ImageResizer]: https://github.com/imazen/resizer
[imageflow-dotnet]: https://github.com/imazen/imageflow-dotnet
[imageflow-node]: https://github.com/imazen/imageflow-node
[imageflow-go]: https://github.com/imazen/imageflow-go

### Benchmark owners in separate processes

`ZENBENCH_LAUNCHER_PIDS` lists comma-separated launcher/worker PIDs belonging
 to the current benchmark. Resource admission subtracts their CPU contribution
 from global load and excludes them from foreign heavy-process/benchmark scans.
 RAM and temperature checks remain global. Only explicitly listed PIDs and the
 measuring process are excluded; unrelated processes remain visible.

Linux heavy-process counts count each process leader once. User-thread entries
are deduplicated for both benchmark owners and foreign processes; global CPU
admission still includes all foreign thread work.

Set `ZENBENCH_GATE_TRACE=1` to log each round's gate verdict, blocking reason,
system snapshot and foreign heavy-process identities to stderr. The trace is
opt-in and runs outside benchmark timers; it uses the existing admission checks.

The `paired_rounds` development example analyzes retained external-worker
 timings through the same private `stats.rs` owner as the engine, including
 paired IQR filtering and 10,000-resample bootstrap intervals. It accepts a
 JSON array of `{baseline, candidate, iterations, timer_resolution_ns}` packets
 on stdin and writes analyses on stdout. It does not run a workload or modify
 the public library API.
