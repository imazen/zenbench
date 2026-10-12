+++
title = "Read example output"
description = "Interpret descriptive rows and keep acceptance decisions separate."
weight = 12
+++

The terminal and JSON excerpts below are historical examples of report shape,
not predictions for your machine or a current default run. Default Iterate now
uses up to 30 rounds unless a group overrides it.

## Live HTML report

[Open example HTML report](/zenbench/example-report.html)

Generated with `cargo bench --bench sorting -- --format=html`. Self-contained, no JavaScript. Click any benchmark name to expand full statistics.

## Terminal output (tree mode, default)

```text
═══════════════════════════════════════════════════════════════
  zenbench  1774477892-21174f
  git: 97e317bc99c789a99e50f9fad734b4897753e20b
═══════════════════════════════════════════════════════════════

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

  total: 27.0s
═══════════════════════════════════════════════════════════════
```

## Terminal output (table mode)

```text
  sort_1000 ───────────────────────────────────────────────────
  200 rounds × 3K calls
  ┌─────────────────┬───────┬───────┬────────────────────────┬─────────┐
  │ benchmark       │   min │  mean │         95% CI vs base │ items/s │
  ├─────────────────┼───────┼───────┼────────────────────────┼─────────┤
  ├─ reversed ─────────────────────────────────────────────────────────┤
  │ std_sort        │ 247ns │ 258ns │ [ 255ns  258ns  262ns] │   3.87G │
  │ sort_unstable   │ 242ns │ 246ns │ [ -5.5%  -4.8%  -4.2%] │   4.06G │
  ├─ already sorted ───────────────────────────────────────────────────┤
  │ std_sort_sorted │ 204ns │ 207ns │ [-18.7% -18.2% -17.8%] │   4.84G │
  │ unstable_sorted │ 198ns │ 202ns │ [-20.7% -19.4% -18.1%] │   4.94G │
  └─────────────────┴───────┴───────┴────────────────────────┴─────────┘
```

## Baseline decisions

Confirmation reports process-level intervals and explicit improvement, regression,
equivalence, non-inferiority or inconclusive verdicts. Descriptive percentage
changes alone cannot pass CI. See [confirmation and CI](@/ci-regression.md).

## Thread scaling output

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

## JSON output (excerpt)

```bash
cargo bench --bench my_bench -- --format=json > result.json
```

```json
{
  "run_id": "1774477892-21174f",
  "git_hash": "97e317b",
  "comparisons": [{
    "group_name": "compress_64k",
    "benchmarks": [{
      "name": "level_1",
      "summary": {
        "mean": 16200.0,
        "min": 15800.0,
        "mad": 500.0,
        "median": 16100.0
      },
      "mean_ci": { "lower": 15800.0, "upper": 16600.0 }
    }]
  }]
}
```

## What to review next

Mean, minimum, MAD and throughput describe observations. Positive latency change
means slower. The comparison interval in Iterate remains exploratory; inspect
resource flags, drift and [A/A controls](@/external-programs.md#review-stop-or-resume).
Accept a candidate only under the applicable [confirmation contract](@/ci-regression.md).
[Read or export retained data](@/results.md) to change views without measuring again.
