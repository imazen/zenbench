# Writing benchmarks

Start with the [Cargo setup and first benchmark](https://docs.rs/zenbench/latest/zenbench/).
Then choose what belongs inside each timed operation.

## Choose a timing boundary

| Goal | API | Timed work |
|---|---|---|
| Time the complete operation | `b.iter(|| work())` | Closure and destruction of its returned output |
| Exclude fresh input setup and output destruction | `b.with_input(|| make_input()).run(|input| work(input))` | Work closure, with a timer per operation |
| Defer output destruction until the batch ends | `b.iter_deferred_drop(|| work())` | Work and output storage; outputs stay live until the batch ends |

Keep the timing boundary the same across comparison arms. Setup costs and deferred
destruction can change the question being measured.

## Compare implementations

Register implementations in one group to run them in shuffled, serial rounds.
This example measures sorting an existing vector, excluding its construction and
returned-output destruction:

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


## Auditable latency samples

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


## Next steps

[CPU pinning](https://docs.rs/zenbench/latest/zenbench/platform/index.html#thread-pinning) ·
[Confirming a change](https://github.com/imazen/zenbench/blob/main/docs/benchmark-modes.md) ·
[Criterion migration](https://github.com/imazen/zenbench/blob/main/MIGRATION.md) ·
[Retaining evidence](https://github.com/imazen/zenbench/blob/main/docs/result-storage.md)
