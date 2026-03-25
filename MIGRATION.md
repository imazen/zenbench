# Migrating from criterion to zenbench

## Quick start (2-line change)

Add zenbench alongside criterion — no need to remove criterion first:

```toml
# Cargo.toml
[dev-dependencies]
criterion = { version = "0.8", features = ["html_reports"] }  # keep during migration
zenbench = { version = "0.1", features = ["criterion-compat"] }
```

For each bench file, change one import:

```rust
// Before:
use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId, Throughput};

// After:
use zenbench::criterion_compat::*;
use zenbench::{criterion_group, criterion_main};
```

Done. Your benchmark code compiles unchanged. Run with:

```bash
cargo bench --bench my_bench
```

Migrate files one at a time. Both criterion and zenbench bench files
coexist — no conflicts. Remove `criterion` from Cargo.toml when all
files are migrated.

---

## What works unchanged

Everything from criterion's public API that real benchmarks use:

| criterion API | zenbench compat | Notes |
|---|---|---|
| `criterion_group!` | ✅ | |
| `criterion_main!` | ✅ | |
| `Criterion::default()` | ✅ | |
| `c.benchmark_group("name")` | ✅ | |
| `c.bench_function("name", \|b\| ...)` | ✅ | |
| `c.bench_with_input(id, &input, \|b, input\| ...)` | ✅ | |
| `group.bench_function("name", \|b\| ...)` | ✅ | |
| `group.bench_with_input(id, &input, \|b, input\| ...)` | ✅ | |
| `group.throughput(Throughput::Elements(n))` | ✅ | |
| `group.throughput(Throughput::Bytes(n))` | ✅ | |
| `group.finish()` | ✅ | |
| `BenchmarkId::new("name", param)` | ✅ | |
| `BenchmarkId::from_parameter(param)` | ✅ | |
| `b.iter(\|\| ...)` | ✅ | |
| `b.iter_batched(setup, routine, BatchSize::...)` | ✅ | |
| `b.iter_batched_ref(setup, routine, BatchSize::...)` | ✅ | |
| `black_box(value)` | ✅ | |
| `c.sample_size(n)` | ✅ | Maps to max_rounds |
| `c.measurement_time(dur)` | ✅ | Maps to max_time |
| `c.warm_up_time(dur)` | ✅ | Maps to warmup_time |
| `c.noise_threshold(f)` | ✅ | Maps to noise_threshold |

Closures can borrow local data freely — no `move` or `Clone` required.
This matches criterion's behavior exactly.

---

## What you get for free

After the import swap, these features are available immediately:

**CI regression testing** (just add CLI flags):
```bash
cargo bench -- --save-baseline=main           # save after merge
cargo bench -- --baseline=main                # check PR (exit 1 on regression)
cargo bench -- --baseline=main --update-on-pass  # auto-ratchet
```

**Output formats:**
```bash
cargo bench -- --format=json    # machine-readable
cargo bench -- --format=csv     # spreadsheet
cargo bench -- --format=llm     # AI-friendly key-value
cargo bench -- --format=md      # markdown tables
```

**Resource gating:** Automatically waits for quiet system before
measuring. No more "re-run on a quiet machine" advice.

**Hardware detection:** TSC timer, stack alignment jitter, loop
overhead compensation — all automatic, no configuration needed.

---

## Level 2: One-line enhancements

Add these to your existing criterion-compat code to unlock zenbench
features. Each is a single line — no structural changes.

### Custom throughput units

```rust
// Before (criterion):
group.throughput(Throughput::Elements(1000));
// Output: "1.23 Gops/s"

// After (add one line):
group.throughput(Throughput::Elements(1000));
group.throughput_unit("pixels");
// Output: "1.23 Gpixels/s"
```

### Baseline comparisons

```rust
// Compare everything against "reference" instead of first benchmark:
group.baseline("reference");
```

### Visual organization

```rust
group.subgroup("Scalar implementations");
group.bench_function("scalar_v1", |b| ...);
group.bench_function("scalar_v2", |b| ...);

group.subgroup("SIMD implementations");
group.bench_function("avx2", |b| ...);
group.bench_function("neon", |b| ...);
```

### Sort by speed

```rust
group.sort_by_speed();  // fastest first in report
```

---

## Level 3: Switch to native API

When you want interleaved execution (precise A-vs-B comparison within
the same thermal/load window), switch to the native API.

### The diff

```rust
// CRITERION-COMPAT (sequential, like criterion):
use zenbench::criterion_compat::*;
use zenbench::{criterion_group, criterion_main};

fn bench_sort(c: &mut Criterion) {
    let mut group = c.benchmark_group("sort");
    group.throughput(Throughput::Elements(1000));
    group.bench_function("std_sort", |b| {
        b.iter(|| { let mut v = data.clone(); v.sort(); v })
    });
    group.bench_function("unstable", |b| {
        b.iter(|| { let mut v = data.clone(); v.sort_unstable(); v })
    });
    group.finish();
}
criterion_group!(benches, bench_sort);
criterion_main!(benches);
```

```rust
// NATIVE ZENBENCH (interleaved, paired statistics):
zenbench::main!(|suite| {
    suite.compare("sort", |group| {
        group.throughput(zenbench::Throughput::Elements(1000));
        group.bench("std_sort", |b| {
            b.iter(|| { let mut v = data.clone(); v.sort(); v })
        });
        group.bench("unstable", |b| {
            b.iter(|| { let mut v = data.clone(); v.sort_unstable(); v })
        });
    });
});
```

### What changes

| criterion-compat | native | |
|---|---|---|
| `c.benchmark_group("name")` | `suite.compare("name", \|group\| { ... })` | Callback instead of builder |
| `group.bench_function("name", \|b\| ...)` | `group.bench("name", \|b\| ...)` | Shorter name |
| `group.finish()` | *(implicit)* | Closure end = finish |
| `criterion_group!` + `criterion_main!` | `zenbench::main!(\|suite\| { ... })` | Single macro |
| Sequential execution | **Interleaved round-robin** | The key difference |

### What you gain

- **Interleaved execution**: All benchmarks in a group run in shuffled
  order each round. System state (thermal, load, cache) affects all
  benchmarks equally → paired differences are precise.
- **Paired statistics**: Bootstrap CI on the paired round-by-round
  differences. Can detect 1-2% changes reliably.
- **Auto-convergence**: Stops measuring when the CI is tight enough,
  saving time on clean systems.
- **Cohen's d effect size**: Standardized "how big is this difference?"
- **Drift detection**: Spearman correlation catches thermal throttling.

---

## What's different from criterion

### Output location

| | criterion | zenbench |
|---|---|---|
| Terminal report | stderr | stderr |
| HTML plots | `target/criterion/` | *(not generated)* |
| JSON results | `target/criterion/*/estimates.json` | `/tmp/zenbench/*.txt` (LLM format) |
| Baselines | `target/criterion/*/base/` | `.zenbench/baselines/*.json` |

### Statistical approach

| | criterion | zenbench |
|---|---|---|
| Bootstrap resamples | 100K | 10K (configurable) |
| Significance test | Welch t-test | Wilcoxon signed-rank |
| CI method | Percentile | Percentile (same) |
| Outlier handling | Classify only | IQR removal on paired diffs |
| Noise threshold | ±1% | ±1% (configurable) |
| Effect size | None | Cohen's d |
| Drift detection | None | Spearman correlation |

### Measurement approach

| | criterion | zenbench |
|---|---|---|
| Execution order | Sequential | Interleaved (native) / Sequential (compat) |
| Iteration estimation | Linear sweep + OLS | Precision-driven + sample target |
| Overhead compensation | Slope regression | Loop subtraction |
| Timer | `Instant::now()` | TSC (rdtsc/rdtscp) with Instant fallback |
| Stack jitter | alloca per sample | Recursive trampoline |
| Warmup | 3s wall time | Configurable (default 500ms) |

---

---

## Migrating from divan

Divan uses attribute macros. Zenbench uses function registration.

### Side-by-side comparison

```rust
// DIVAN:
use divan::{Bencher, black_box};

fn main() { divan::main(); }

#[divan::bench]
fn fibonacci() -> u64 {
    black_box(fib(20))
}

#[divan::bench(args = [100, 1000, 10000])]
fn sort(bencher: Bencher, n: usize) {
    bencher.with_inputs(|| (0..n).rev().collect::<Vec<u32>>())
           .bench_values(|mut v| { v.sort(); v })
}
```

```rust
// ZENBENCH (function list form):
fn bench_fib(suite: &mut zenbench::Suite) {
    suite.compare("fibonacci", |group| {
        group.bench("fib_20", |b| b.iter(|| zenbench::black_box(fib(20))));
    });
}

fn bench_sort(suite: &mut zenbench::Suite) {
    suite.compare("sort", |group| {
        group.throughput(zenbench::Throughput::Elements(10000));
        for n in [100, 1000, 10000] {
            group.bench(format!("sort_{n}"), move |b| {
                b.with_input(|| (0..n).rev().collect::<Vec<u32>>())
                    .run(|mut v| { v.sort(); v })
            });
        }
    });
}

zenbench::main!(bench_fib, bench_sort);
```

### Key differences

| | divan | zenbench |
|---|---|---|
| Registration | `#[divan::bench]` attribute | Function taking `&mut Suite` |
| Entry point | `divan::main()` | `zenbench::main!(func1, func2)` |
| Parameterization | `args = [...]` attribute | `for` loop in function body |
| Input generation | `bencher.with_inputs(gen).bench_values(f)` | `b.with_input(gen).run(f)` |
| Thread testing | `#[divan::bench(threads = [1,2,4])]` | `group.bench_parallel("name", 4, \|b, tid\| ...)` |
| Alloc profiling | `#[global_allocator] AllocProfiler` | Same: `#[global_allocator] AllocProfiler` |
| Output | Terminal only | Terminal + JSON/CSV/LLM/Markdown |
| Statistics | min/max/median/mean | + Bootstrap CI, Wilcoxon, Cohen's d |
| CI regression | Not built in | `--baseline`, `--save-baseline`, exit codes |
| Interleaving | No | Yes (benchmarks in same group shuffled per round) |

### What you gain

- **Paired statistics**: Interleaved execution means A-vs-B comparisons
  within a group are measured under identical system conditions.
- **CI regression testing**: `--baseline=main --max-regression=5` blocks
  PRs on performance regressions.
- **Resource gating**: Waits for quiet system before measuring.
- **Machine-readable output**: JSON, CSV, LLM formats for dashboards.
- **Subgroups, baselines, throughput units**: Richer reporting.

### What you lose

- **Attribute macro ergonomics**: Divan's `#[divan::bench]` is the
  shortest possible syntax. Zenbench requires explicit function bodies
  with `suite.compare()` / `group.bench()`.
- **Automatic type/const parameterization**: Divan's `types = [Vec, LinkedList]`
  and `consts = [1, 2, 4]` have no direct equivalent — use `for` loops.
- **Deferred drop by default**: Divan automatically defers drop via
  `MaybeUninit`. Zenbench requires explicit `b.iter_deferred_drop()`.

---

## FAQ

**Q: Do I need to change my Cargo.toml `[[bench]]` sections?**
No. The `harness = false` and bench names stay the same.

**Q: Can I use both criterion and zenbench in the same `Cargo.toml`?**
Yes. No name conflicts. Each bench file chooses which to use via its import.

**Q: Will my CI scripts break?**
No. `cargo bench` works the same. Add `-- --save-baseline=main` for
regression detection.

**Q: What about `criterion::Benchmark` and other advanced types?**
The compat layer covers the common API. Esoteric types like `Benchmark`,
`PlotConfiguration`, `SamplingMode` are not supported. If you use these,
stay on criterion for those files and migrate the rest.

**Q: Does the compat layer support async benchmarks?**
Not yet through the compat layer. Use the native API with
`b.iter_async(runtime, || async { ... })` (requires `async` feature).

**Q: My benchmarks use `criterion::Criterion::configure()`?**
The compat `Criterion` accepts `sample_size()`, `measurement_time()`,
`warm_up_time()`, and `noise_threshold()`. Other config methods are
accepted but ignored.
