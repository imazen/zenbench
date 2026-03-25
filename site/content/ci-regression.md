+++
title = "CI Regression Testing"
weight = 2
+++

## Save a baseline

After merging to main, save benchmark results:

```bash
cargo bench -- --save-baseline=main
```

This writes `.zenbench/baselines/main.json` — a complete snapshot of all benchmark results.

## Check for regressions on PRs

```bash
cargo bench -- --baseline=main
```

Exit codes:
- **0** — pass, no regressions exceed threshold
- **1** — fail, regressions detected
- **2** — error (baseline not found, etc.)

Output:

```text
  Baseline comparison
  ───────────────────
  ⚠ git hash differs: baseline=abc12345 current=def67890

  compress::level_1     16.2µs →   16.4µs    +1.2%    unchanged
  compress::level_6     15.1µs →   15.3µs    +1.3%    unchanged
  compress::mixed      401.0µs →  425.3µs    +6.1%  ▲ REGRESSION

  Summary: 1 regressions, 0 improvements, 2 unchanged

[zenbench] FAIL: 1 regression(s) exceed 5% threshold
```

## Configure the threshold

```bash
# Fail if any benchmark regresses more than 3%
cargo bench -- --baseline=main --max-regression=3

# Auto-update baseline when the run passes
cargo bench -- --baseline=main --update-on-pass
```

## GitHub Actions workflow

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

      - name: Check for regressions
        run: cargo bench -- --baseline=main --max-regression=5

      - name: Update baseline (main only)
        if: github.ref == 'refs/heads/main'
        run: cargo bench -- --save-baseline=main
```

## How comparison works

When comparing against a baseline, zenbench:

1. Runs the full benchmark suite (interleaved, converging)
2. Loads the baseline JSON
3. Matches benchmarks by `group::name`
4. Computes percentage change for each
5. Applies **both** a percentage threshold AND a statistical t-test
6. Only flags a regression if it exceeds the threshold AND is statistically significant

The t-test prevents false positives from noisy CI runners — if the mean shifted but the variance is high, the test won't flag it.

## Hardware fingerprinting

Baselines include a hardware fingerprint (CPU model, arch, OS, core count). When comparing, zenbench warns if the hardware changed:

```text
  ⚠ CPU changed: baseline='AMD EPYC 7763' current='Intel Xeon E5-2686'
```

## Comparing against git tags

```bash
# Save baseline at release time
cargo bench -- --save-baseline=v0.3.0

# Later, check for regressions vs release
cargo bench -- --baseline=v0.3.0

# Or use worktree-based comparison (builds both versions, interleaves)
zenbench self-compare --bench my_bench --ref v0.3.0
```
