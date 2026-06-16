# zenbench [![CI](https://img.shields.io/github/actions/workflow/status/imazen/zenbench/ci.yml?style=flat-square)](https://github.com/imazen/zenbench/actions/workflows/ci.yml) [![crates.io](https://img.shields.io/crates/v/zenbench?style=flat-square)](https://crates.io/crates/zenbench) [![lib.rs](https://img.shields.io/crates/v/zenbench?style=flat-square&label=lib.rs&color=blue)](https://lib.rs/crates/zenbench) [![docs.rs](https://img.shields.io/docsrs/zenbench?style=flat-square)](https://docs.rs/zenbench) [![license](https://img.shields.io/crates/l/zenbench?style=flat-square)](https://github.com/imazen/zenbench#license) [![codecov](https://img.shields.io/codecov/c/github/imazen/zenbench?style=flat-square)](https://codecov.io/gh/imazen/zenbench)

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

Existing harnesses run benchmarks **sequentially**. Benchmark A runs on a hot CPU; benchmark B runs on an even hotter CPU with degraded turbo boost. System load changes between runs corrupt results.

Zenbench **interleaves**: each round, all benchmarks run in shuffled order. Round N of A and round N of B execute under identical conditions. Paired statistics on the round-by-round differences detect real changes — not thermal drift.

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
| Cross-run variance inflation | ❌ | ❌ | ✅ pooled t-test |
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

## Quick start

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

## CI regression testing

```bash
# After merging to main — save a baseline
cargo bench -- --save-baseline=main

# On PRs — check for regressions (exits 1 on a >5% slowdown that's also
# statistically significant; see "CI regression gate semantics" below)
cargo bench -- --baseline=main

# Auto-update baseline on clean runs
cargo bench -- --baseline=main --update-on-pass --max-regression=5
```

```text
  Baseline comparison
  ───────────────────
  compress::level_1     16.2µs →   16.4µs    +1.2%    unchanged
  compress::level_6     15.1µs →   15.3µs    +1.3%    unchanged
  compress::level_9     15.0µs →   15.6µs    +4.0%    unchanged
  compress::mixed      401.0µs →  412.3µs    +2.8%    unchanged
  decompress::zenflate  91.5µs →   92.7µs    +1.3%    unchanged

  Summary: 0 regressions, 0 improvements, 5 unchanged

[zenbench] PASS: no regressions exceed 5% threshold
```

Full CI guide with GitHub Actions workflows: [REGRESSION-TESTING.md](REGRESSION-TESTING.md)

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

`--max-regression=N` is **significance-gated**, not a raw percentage cutoff. A
benchmark fails the gate (counts as a regression, exit code 1) only when it is
**both**:

- more than `N%` slower than the saved baseline, **and**
- statistically significant — a pooled two-sample t-test on the baseline-vs-new
  samples gives `t > 2.0` (≈ p < 0.05). With < 2 samples per side, or zero
  variance, the percentage alone decides.

This means a noisy `+10%` swing on a loaded CI runner can **pass** the gate if
the variance is high enough that the t-test can't confirm it — by design, so
flaky runners don't fail your build on noise. If you need a hard percentage
ceiling that ignores significance, gate on the raw `pct_change` in the CSV/JSON
output yourself rather than relying on `--max-regression`.

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

Full upgrade ladder: [MIGRATION.md](MIGRATION.md)

## Output formats

```bash
cargo bench                           # tree display (default, stderr)
cargo bench -- --style=table          # bordered tables with min column
cargo bench -- --format=json          # structured JSON (stdout)
cargo bench -- --format=csv           # spreadsheet-friendly (stdout)
cargo bench -- --format=llm           # key=value for AI tools (stdout)
cargo bench -- --format=md            # markdown tables (stdout)
```

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

## Configuration

```rust,ignore
group.config()
    .max_rounds(200)              // default 200
    .noise_threshold(0.02)        // ±2% significance gate
    .bootstrap_resamples(100_000) // CI precision (default 10K)
    .linear_sampling(true)        // slope regression for sub-100ns
    .cold_start(true)             // 1 iter + cache firewall
    .stack_jitter(true)           // random alignment (default on)
    .sort_by_speed(true);         // fastest first in report
```

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

## Image tech I maintain

| | |
|:--|:--|
| State of the art codecs* | [zenjpeg] · [zenpng] · [zenwebp] · [zengif] · [zenavif] ([rav1d-safe] · [zenrav1e] · [zenavif-parse] · [zenavif-serialize]) · [zenjxl] ([jxl-encoder] · [zenjxl-decoder]) · [zentiff] · [zenbitmaps] · [heic] · [zenraw] · [zenpdf] · [ultrahdr] · [mozjpeg-rs] · [webpx] |
| Compression | [zenflate] · [zenzop] |
| Processing | [zenresize] · [zenfilters] · [zenquant] · [zenblend] |
| Metrics | [zensim] · [fast-ssim2] · [butteraugli] · [resamplescope-rs] · [codec-eval] · [codec-corpus] |
| Pixel types & color | [zenpixels] · [zenpixels-convert] · [linear-srgb] · [garb] |
| Pipeline | [zenpipe] · [zencodec] · [zencodecs] · [zenlayout] · [zennode] |
| ImageResizer | [ImageResizer] (C#) — 24M+ NuGet downloads across all packages |
| [Imageflow][] | Image optimization engine (Rust) — [.NET][imageflow-dotnet] · [node][imageflow-node] · [go][imageflow-go] — 9M+ NuGet downloads across all packages |
| [Imageflow Server][] | [The fast, safe image server](https://www.imazen.io/) (Rust+C#) — 552K+ NuGet downloads, deployed by Fortune 500s and major brands |

<sub>* as of 2026</sub>

### General Rust awesomeness

[archmage] · [magetypes] · [enough] · [whereat] · **zenbench** · [cargo-copter]

[And other projects](https://www.imazen.io/open-source) · [GitHub @imazen](https://github.com/imazen) · [GitHub @lilith](https://github.com/lilith) · [lib.rs/~lilith](https://lib.rs/~lilith) · [NuGet](https://www.nuget.org/profiles/imazen) (over 30 million downloads / 87 packages)

[zenjpeg]: https://crates.io/crates/zenjpeg
[zenpng]: https://crates.io/crates/zenpng
[zenwebp]: https://crates.io/crates/zenwebp
[zengif]: https://crates.io/crates/zengif
[zenavif]: https://crates.io/crates/zenavif
[rav1d-safe]: https://crates.io/crates/rav1d-safe
[zenrav1e]: https://crates.io/crates/zenrav1e
[zenavif-parse]: https://crates.io/crates/zenavif-parse
[zenavif-serialize]: https://crates.io/crates/zenavif-serialize
[zenjxl]: https://crates.io/crates/zenjxl
[jxl-encoder]: https://crates.io/crates/jxl-encoder
[zenjxl-decoder]: https://crates.io/crates/zenjxl-decoder
[zentiff]: https://crates.io/crates/zentiff
[zenbitmaps]: https://crates.io/crates/zenbitmaps
[heic]: https://crates.io/crates/heic
[zenraw]: https://crates.io/crates/zenraw
[zenpdf]: https://crates.io/crates/zenpdf
[ultrahdr]: https://crates.io/crates/ultrahdr
[mozjpeg-rs]: https://crates.io/crates/mozjpeg-rs
[webpx]: https://crates.io/crates/webpx
[zenflate]: https://crates.io/crates/zenflate
[zenzop]: https://crates.io/crates/zenzop
[zenresize]: https://crates.io/crates/zenresize
[zenfilters]: https://crates.io/crates/zenfilters
[zenquant]: https://crates.io/crates/zenquant
[zenblend]: https://crates.io/crates/zenblend
[zensim]: https://crates.io/crates/zensim
[fast-ssim2]: https://crates.io/crates/fast-ssim2
[butteraugli]: https://crates.io/crates/butteraugli
[resamplescope-rs]: https://crates.io/crates/resamplescope-rs
[codec-eval]: https://crates.io/crates/codec-eval
[codec-corpus]: https://crates.io/crates/codec-corpus
[zenpixels]: https://crates.io/crates/zenpixels
[zenpixels-convert]: https://crates.io/crates/zenpixels-convert
[linear-srgb]: https://crates.io/crates/linear-srgb
[garb]: https://crates.io/crates/garb
[zenpipe]: https://crates.io/crates/zenpipe
[zencodec]: https://crates.io/crates/zencodec
[zencodecs]: https://crates.io/crates/zencodecs
[zenlayout]: https://crates.io/crates/zenlayout
[zennode]: https://crates.io/crates/zennode
[ImageResizer]: https://imageresizing.net
[Imageflow]: https://github.com/imazen/imageflow
[imageflow-dotnet]: https://www.nuget.org/packages/Imageflow.AllPlatforms
[imageflow-node]: https://www.npmjs.com/package/@imazen/imageflow-node
[imageflow-go]: https://github.com/imazen/imageflow-go
[Imageflow Server]: https://github.com/imazen/imageflow-dotnet-server
[archmage]: https://crates.io/crates/archmage
[magetypes]: https://crates.io/crates/magetypes
[enough]: https://crates.io/crates/enough
[whereat]: https://crates.io/crates/whereat
[cargo-copter]: https://crates.io/crates/cargo-copter

## License

MIT OR Apache-2.0
