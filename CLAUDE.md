# Zenbench Project Instructions

## What this is

Interleaved microbenchmarking crate for Rust. `#![forbid(unsafe_code)]`, MSRV 1.89, edition 2024, MIT/Apache-2.0. Pure Rust, no C dependencies.

## Architecture

```
src/
  lib.rs       — public API, main! macro, prelude
  bench.rs     — Suite, BenchGroup, GroupConfig, Bencher, BenchFn
  engine.rs    — Engine, run_comparison_group, run_standalone, convergence logic
  results.rs   — SuiteResult, ComparisonResult, BenchmarkResult, to_llm/csv/markdown
  report.rs    — print_report terminal renderer (ANSI tables, bar charts, footnotes)
  format.rs    — format_ns, ns_unit, format_ns_range helpers
  stats.rs     — Summary, PairedAnalysis, bootstrap_ci, Wilcoxon, Spearman
  gate.rs      — ResourceGate, GateConfig, system health checks
  platform.rs  — SystemMonitor, CI detection, git hash
  checks.rs    — BenchWarning, WarningKind (mostly superseded by footnotes in report.rs)
  daemon.rs    — fire-and-forget subprocess mode
  mcp.rs       — MCP JSON-RPC server
  ci.rs        — CI environment detection
  bin/         — zenbench CLI, zenbench-mcp
benches/
  sorting.rs   — comprehensive demo: sort, sub-ns, contention, parallel, throughput
```

## Key design decisions

- **Interleaving is the core design**, not opt-in. Every round shuffles benchmark order.
- **Significance = 95% CI excludes zero**. No p-value thresholds, no hardcoded percentage cutoffs.
- **Bootstrap percentile CI** (10K resamples, 2.5th/50th/97.5th). Non-parametric, captures asymmetry.
- **MAD not stddev** in display. Stddev is destroyed by one context-switch spike.
- **Cache firewall off by default**. Most benchmarks measure hot-path code; firewall penalizes pointer-chasing unrealistically.
- **Auto-rounds convergence** based on paired-difference CI + effect-size stability, not individual benchmark precision or p-value targets.
- **Auto-save to /tmp/zenbench/** in LLM format. Path printed at start so tools can re-read without re-running.

## Report output structure

The terminal report is the primary output. Columns:
- `benchmark` — name (green = fastest in group)
- `min` — fastest observed run (real floor, per Chen & Revels)
- `mean` — typical performance
- `mad` — median absolute deviation (robust noise metric, hidden when throughput present)
- `95% CI vs base` — `[lo%  mid%  hi%]` for comparisons, `[lo  mean  hi]ns` for baseline
- `throughput` — when Throughput is set

Footnotes `[1]` fire for: CI crosses zero, tiny effect (d<0.2), drift (Spearman r>0.5), high CV (>20%).

Bar chart: always fastest-first, terminal-width-aware. Throughput labels when throughput set.

## Statistical methodology

Read METHODOLOGY.md for the full cross-reference with academic papers. Key points:
- Mytkowicz (ASPLOS 2009): layout bias is real but we can't fix it
- STABILIZER (ASPLOS 2013): 30+ rounds minimum for stable distributions
- Chen & Revels (2016): min as floor estimator, noise is additive
- Kalibera & Jones (ISMM 2013): always report uncertainty
- Convergence: paired-diff CI must exclude zero AND effect size must be stable (CI half-width < 10% of difference magnitude)

## Threading APIs

Three patterns, three APIs:
- `bench_contended(name, threads, setup, work)` — shared state under lock pressure
- `bench_parallel(name, threads, work)` — independent work scaling
- `bench()` — for rayon/tokio code that manages its own threads

Don't mix bench_parallel/bench_contended with rayon — competing thread pools.

## Output formats

- Terminal: ANSI tables with color (stderr)
- `--format=llm` or `ZENBENCH_FORMAT=llm`: key-value lines with `|` section separators (stdout)
- `--format=csv|md|json`: other formats (stdout)
- Auto-saved to `/tmp/zenbench/zenbench-{run_id}.txt` in LLM format

## Remaining work (from METHODOLOGY.md)

### Near-term
- `bench_scaling()` — automatic thread count probing with scaling/efficiency table
- Gate thread awareness — don't flag benchmark threads as heavy processes
- Pause/resume timer for mid-benchmark I/O exclusion
- Cold-start mode — `config().cold_start(true)` with cache firewall + 1 iter/sample

### Medium-term
- Asymptotic complexity analysis (Big O fitting, like Google Benchmark)
- Manual timing mode for GPU/custom hardware
- Process-level CPU time for threading efficiency analysis
- Custom counters (user-defined per-iteration metrics)

### Known bugs / tech debt
- `checks.rs` is mostly dead code — footnote system in `report.rs` replaced it
- `BelowTimerResolution` warning kind is unused
- Markdown output (`to_markdown`) doesn't use the new column layout
- CSV output doesn't include cold_start_ns or comparison CI percentages
- No tests for the terminal report, LLM format, or bar chart output

## Development notes

- Run `cargo bench --bench sorting` for the comprehensive demo
- The sorting benchmark covers: subgroups, throughput, sub-ns, contention, parallel scaling, sort sizes
- `ZENBENCH_NO_SAVE=1` to disable auto-save during development
- Report code is in `report.rs` (~891 lines) — the main complexity center
- Three-pass row construction in report.rs: collect raw data → compute column widths → format with alignment
- Formatting precision: per-value dp based on magnitude, column-wide alignment via max-width padding
