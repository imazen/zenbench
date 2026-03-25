# Regression Testing with zenbench

How to use zenbench for performance regression testing — locally,
in CI, and across versions.

## Quick start

```bash
# Save a baseline after merging to main
cargo bench -- --save-baseline=main

# Check a PR against the baseline (exit 1 if regressions > 5%)
cargo bench -- --baseline=main

# Tighter threshold for critical paths
cargo bench -- --baseline=main --max-regression=2.0

# Auto-update baseline when the run passes
cargo bench -- --baseline=main --update-on-pass
```

Works with both `zenbench::main!()` and `criterion_main!()` macros.

---

## User stories

### 1. Developer: "Did my change make anything slower?"

Run benchmarks and compare against the last known-good state:

```bash
# One-time: save a baseline from main
git checkout main
cargo bench -- --save-baseline=main

# Back to your feature branch
git checkout feature/my-change
cargo bench -- --baseline=main
```

Output:
```
  Baseline comparison
  ───────────────────
  sort::std_sort                     245.3ns →    243.1ns   -0.90%    unchanged
  sort::unstable                     198.7ns →    312.4ns  +57.22%  ▲ REGRESSION
  parse::json_small                   1.2µs →      1.1µs   -4.17%  ▼ improved

  Summary: 1 regressions, 1 improvements, 1 unchanged

[zenbench] FAIL: 1 regression(s) exceed 5.0% threshold
```

Exit code 1 — your shell/CI knows it failed.

### 2. CI: Block PRs on performance regressions

**GitHub Actions workflow:**

```yaml
name: Benchmarks
on: [pull_request]

jobs:
  bench:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Restore baseline
        uses: actions/cache@v4
        with:
          path: .zenbench/baselines
          key: bench-baselines-${{ github.base_ref }}

      - name: Run benchmarks
        run: |
          cargo bench -- --baseline=main --max-regression=5.0

      # If we get here, no regressions. Update the baseline for next time.
      - name: Update baseline
        if: github.ref == 'refs/heads/main'
        run: |
          cargo bench -- --save-baseline=main
```

The `actions/cache` restores the baseline from the last main-branch run.
PRs compare against it. Merges to main update it.

### 3. CI: Track performance on every merge to main

```yaml
name: Bench tracking
on:
  push:
    branches: [main]

jobs:
  bench:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Restore baseline
        uses: actions/cache@v4
        with:
          path: .zenbench/baselines
          key: bench-baselines-main

      - name: Benchmark and update baseline
        run: |
          cargo bench -- --baseline=main --update-on-pass --max-regression=10.0

          # Also save a commit-tagged snapshot for historical comparison
          cargo bench -- --save-baseline=commit-$(git rev-parse --short HEAD)
```

`--update-on-pass` auto-ratchets the baseline: if the current run has
no regressions beyond the threshold, it becomes the new baseline. This
means the baseline always reflects the most recent passing commit.

### 4. Developer: Compare against a specific commit

Use the CLI's `self-compare` for interleaved comparison against any git
ref — both versions built and measured on the same machine in the same
session:

```bash
# Compare against the most recent version tag
zenbench self-compare --bench my_bench

# Compare against a specific commit
zenbench self-compare --bench my_bench --ref abc1234

# Compare against a branch
zenbench self-compare --bench my_bench --ref origin/main
```

This builds both versions via git worktrees and interleaves their
execution. Results are paired — far more sensitive than comparing
baselines from separate runs.

### 5. Migrating from criterion

Change one import:

```rust
// Before:
use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};

// After:
use zenbench::criterion_compat::*;
```

Your existing `criterion_group!`/`criterion_main!` macros, `bench_function`,
`bench_with_input`, `BenchmarkId`, `Throughput`, and `group.finish()` all
work unchanged. You immediately get:

- `--save-baseline=main` / `--baseline=main` for regression testing
- Interleaved execution with paired statistics
- Resource gating (CPU load, temperature, heavy processes)
- Bootstrap confidence intervals with noise threshold
- TSC timing and stack alignment jitter
- Allocation profiling (with `AllocProfiler`)
- JSON/CSV/LLM/Markdown output formats

### 6. Parameterized benchmarks

Both native and criterion-compat APIs support parameterized benchmarks:

**Native:**
```rust
zenbench::main!(|suite| {
    suite.compare("sort", |group| {
        for size in [100, 1000, 10000] {
            let data: Vec<u32> = (0..size).rev().collect();
            group.bench(format!("std_sort/{size}"), move |b| {
                let d = data.clone();
                b.with_input(move || d.clone())
                    .run(|mut v| { v.sort(); v })
            });
        }
    });
});
```

**Criterion-compat:**
```rust
fn bench_sort(c: &mut Criterion) {
    let mut group = c.benchmark_group("sort");
    for size in [100, 1000, 10000] {
        group.bench_with_input(
            BenchmarkId::new("std_sort", size),
            &size,
            |b, &size| {
                b.iter(|| {
                    let mut v: Vec<u32> = (0..size).rev().collect();
                    v.sort();
                    v
                })
            },
        );
    }
    group.finish();
}
```

### 7. Incremental migration from criterion

You don't have to migrate all benchmarks at once. Keep both deps and
migrate one file at a time:

```toml
# Cargo.toml — keep both during migration
[dev-dependencies]
criterion = { version = "0.8", features = ["html_reports"] }
zenbench = { version = "0.1", features = ["criterion-compat"] }
```

For each bench file, change the import (no other code changes needed):

```rust
// Before:
use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId, Throughput};

// After:
use zenbench::criterion_compat::*;
use zenbench::{criterion_group, criterion_main};
```

Both migrated and unmigrated bench files work side by side. Run both
to verify parity, then drop `criterion` from Cargo.toml when done.

**What you get immediately per migrated file:**
- `--save-baseline` / `--baseline` for CI regression detection
- Resource gating (CPU load, temperature, heavy processes)
- Bootstrap CIs with noise threshold
- Hardware TSC timing and stack alignment jitter
- Allocation profiling (with `AllocProfiler`)
- JSON/CSV/LLM/Markdown output formats

**What you don't get via criterion-compat (use native API for these):**
- Interleaved execution (criterion-compat runs sequentially, like criterion)
- Paired statistical comparisons between benchmarks in the same group
- Auto-convergence (criterion-compat uses fixed round count)

### 8. Release gates

Before publishing a crate, verify performance hasn't regressed from the
last release:

```bash
# Save baseline from the current release tag
git checkout v0.3.0
cargo bench -- --save-baseline=release-v0.3.0

# Check the new version
git checkout main
cargo bench -- --baseline=release-v0.3.0 --max-regression=3.0
```

Or use `self-compare` for interleaved measurement:

```bash
zenbench self-compare --bench my_bench --ref v0.3.0
```

### 8. Allocation regression testing

Track heap allocation counts alongside timing:

```rust
#[global_allocator]
static ALLOC: zenbench::AllocProfiler = zenbench::AllocProfiler::system();

zenbench::main!(|suite| {
    suite.compare("collections", |group| {
        group.bench("vec_push", |b| {
            b.iter_deferred_drop(|| {
                let mut v = Vec::new();
                for i in 0..100 {
                    v.push(i);
                }
                v
            })
        });
    });
});
```

Output includes `allocs/iter` and `bytes/iter` alongside timing data.
Use `--format=csv` or `--format=json` to capture allocation stats for
automated analysis.

---

## Configuration reference

### CLI flags (after `--` in `cargo bench`)

| Flag | Default | Description |
|------|---------|-------------|
| `--save-baseline=NAME` | — | Save results as named baseline |
| `--baseline=NAME` | — | Compare against named baseline |
| `--max-regression=PCT` | 5.0 | Max allowed regression (%) |
| `--update-on-pass` | off | Update baseline if no regressions |
| `--format=FMT` | terminal | Output: `llm`, `csv`, `md`, `json` |
| `--group=NAME` | all | Run only matching group |

### Exit codes

| Code | Meaning |
|------|---------|
| 0 | Pass — no regressions (or no baseline comparison) |
| 1 | Fail — regressions exceed threshold |
| 2 | Error — baseline not found, save failed |

### Baseline storage

Baselines are stored in `.zenbench/baselines/<name>.json`. These are
regular `SuiteResult` JSON files. You can:

- Commit them to git for reproducibility
- Cache them in CI (recommended for cross-job comparison)
- Gitignore them if they're large

### CLI management

```bash
zenbench baseline list           # Show all saved baselines
zenbench baseline show main      # Show details of a baseline
zenbench baseline delete old     # Delete a baseline
```

---

## How it works

### Saved baseline comparison

When you use `--baseline=main`, zenbench:

1. Runs the full benchmark suite (interleaved, converging)
2. Loads `.zenbench/baselines/main.json`
3. Matches benchmarks by group name + benchmark name
4. Computes percentage change: `(new_mean - baseline_mean) / baseline_mean`
5. Flags regressions exceeding `--max-regression`
6. Exits with code 0 (pass) or 1 (fail)

This is a **cross-run comparison** — the baseline and current run may
have executed on different hardware, at different times, under different
system conditions. The comparison uses raw mean values, not paired
statistics. For noisy environments, use generous thresholds (5-10%).

### Self-compare (interleaved)

`zenbench self-compare` builds both versions from git worktrees and
runs them in the same session with interleaved execution. This gives
paired statistics — far more sensitive than cross-run comparison.
Can detect 1-2% differences reliably.

Use `--baseline` for CI gates (fast, simple). Use `self-compare` for
investigating specific regressions (precise, slower).
