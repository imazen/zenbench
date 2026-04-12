# Zenbench Project Instructions

## What this is

Interleaved microbenchmarking crate for Rust. `#![forbid(unsafe_code)]`, MSRV 1.85, edition 2024, MIT/Apache-2.0. Pure Rust, no C dependencies.

## Architecture

```
src/
  lib.rs       — public API, main! macro, prelude, run_passes, run_processes, aggregate_results, Aggregation
  bench.rs     — Suite, BenchGroup, GroupConfig, Bencher, BenchFn
  engine.rs    — Engine, run_comparison_group, convergence logic
  results.rs   — SuiteResult, ComparisonResult, BenchmarkResult, to_llm/csv/markdown
  report.rs    — print_report terminal renderer (ANSI tables, bar charts, footnotes)
  format.rs    — format_ns, ns_unit helpers
  stats.rs     — Summary, PairedAnalysis, bootstrap_ci, Wilcoxon, Spearman
  gate.rs      — ResourceGate, GateConfig, system health checks, ZENBENCH_LAUNCHER_PIDS
  platform.rs  — SystemMonitor, CI detection, git hash
  timing.rs    — TSC reads, asm fences, frequency calibration (precise-timing feature, only unsafe)
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

### Done (this session)
- `bench_scaling()` — probes 1..=physical_cores, then adds logical_cores as extra point if different
- Gate thread awareness — benchmark_thread_allowance adjusts threshold
- Cold-start mode — `config().cold_start(true)` forces 1 iter + cache firewall
- Markdown output updated to new columns (min, mean, vs base, throughput)
- CSV output includes cold_start_ns, comparison CI, significance fields
- `checks.rs` cleaned — dead functions removed, only public types remain

### Near-term
- Pause/resume timer for mid-benchmark I/O exclusion
- `bench_scaling` efficiency/scaling columns (currently just uses throughput)
- Add scaling/efficiency metrics to the LLM format output

### All statistical/methodology gaps — DONE
- ~~Overhead compensation~~, ~~slope regression~~, ~~noise threshold~~, ~~per-benchmark CIs~~
- ~~TSC timer~~, ~~asm fences~~, ~~stack jitter~~, ~~configurable resamples~~
- ~~Warmup phase~~, ~~deferred drop~~, ~~precision-driven iteration estimation~~

### All CI regression features — DONE
- ~~Baselines~~, ~~exit codes~~, ~~CLI management~~, ~~update-on-pass~~
- ~~Cross-run variance inflation~~, ~~hardware fingerprint~~, ~~testbed guards~~
- ~~Calibration workloads~~

### All framework parity features — DONE
- ~~Criterion config forwarding~~ (sample_size, measurement_time, etc.)
- ~~Async support~~ (iter_async with tokio block_on, feature = "async")
- ~~Allocation profiling~~ (AllocProfiler, feature = "alloc-profiling")

### Remaining (future work)
- Change point detection (E-Divisive) — design documented in METHODOLOGY.md
- Asymptotic complexity analysis (Big O fitting)
- `iter_custom` for externally-timed operations (GPU, etc.)
- Instruction counting mode (iai-callgrind interop)
- Calibration-normalized output columns
- `--compare-ref` from bench binary macros (CLI self-compare works)

### Known bugs / tech debt
- No tests for terminal report rendering (report.rs, 948 lines)
- No tests for daemon.rs (486 lines) or mcp.rs (634 lines)
- Markdown bar chart doesn't sort by speed like terminal bar chart does
- `sysinfo::System::new_all()` in bench_scaling is heavy — consider caching

## Development notes

- Run `cargo bench --bench sorting` for the comprehensive demo
- The sorting benchmark covers: subgroups, throughput, sub-ns, contention, parallel scaling, sort sizes
- `ZENBENCH_NO_SAVE=1` to disable auto-save during development
- Report code is in `report.rs` (~891 lines) — the main complexity center
- Three-pass row construction in report.rs: collect raw data → compute column widths → format with alignment
- Formatting precision: per-value dp based on magnitude, column-wide alignment via max-width padding

### Known migration friction
- criterion_compat requires `'static` closures (criterion doesn't). Users must add `move` and `.clone()` captured data. This is because zenbench stores closures for interleaved execution; criterion runs them immediately. Fixing properly requires lifetime-parameterized BenchFn or scoped thread approach. Filed as tech debt.

### Report format evolution (from divan comparison)
Divan's tree-style output is more compact than our box-drawing tables:
- No top/bottom borders (saves 2 lines per group)
- Tree nesting for subgroups (├─/│/╰─ instead of full-width separators)
- Single header across all groups (not repeated per group)
- Right-aligned values without explicit column borders

Consider adopting a lighter format in a future pass. Current format is
correct and complete — this is about density and aesthetics, not data.
Key constraint: our CI column and throughput column are differentiators
that divan doesn't have — any format change must preserve them.

### Windows verification (2026-03-25)
Tested via WSL→PowerShell bridge (Windows cargo 1.92 nightly):
- cargo check: ✓ builds clean
- cargo clippy -- -D warnings: ✓ zero warnings
- cargo test --lib: 89/89 pass
- cargo test (full suite including criterion-compat): 170/170 pass
- cargo bench --bench sorting: ✓ full report with tree output, bar chart, TSC timer
- fs4 file locking: ✓ works on Windows NTFS
- sysinfo process detection: ✓ works for benchmark detection
- terminal_size: ✓ returns correct width
- ANSI colors: ✓ work in Windows Terminal (PowerShell 7)
- PathBuf paths: ✓ baselines save/load correctly
- Git commands: ✓ git available in PATH, worktree works
