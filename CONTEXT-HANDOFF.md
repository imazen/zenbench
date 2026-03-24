# Context Handoff — Zenbench Session 2026-03-23/24

## What happened

Massive feature development session: ~55 commits, grew from a basic
interleaved benchmark harness to a near-0.1-ready crate with CI,
criterion compat, and 112 tests.

## Current state

- **All tests pass**: 112 total (66 unit + 15 integration + 28 edge case + 3 criterion compat)
- **Zero clippy warnings** with `-D warnings`
- **Zero doc warnings** with `-D warnings`
- **CI**: 6 OS targets, lint/test/coverage/MSRV jobs
- **Code**: ~7800 lines across 14 source files

## What was built this session

### Statistical methodology overhaul
- Significance = 95% bootstrap CI excludes zero (no hardcoded thresholds)
- Bootstrap returns (lower, median, upper) triple — all percentiles of same distribution
- MAD instead of stddev for noise display (robust to outliers)
- Auto-rounds convergence: stops when paired-diff CI is resolved AND effect size is stable
- Timer resolution detection at startup
- Cold-start mode: `config().cold_start(true)` → 1 call + cache firewall

### Report formatting
- Box-drawing table with columns: benchmark | min | mean | 95% CI vs base | throughput
- [lo mid hi] three-value ranges with shared unit inside brackets
- Baseline row shows absolute [ns] range, comparison rows show [%] range
- Bar chart: fastest-first, terminal-width-aware, throughput labels
- Footnotes [1][2] for issues (CI crosses zero, tiny effect, drift, high CV)
- Subgroup headers as mid-table separator rows
- Color auto-detection: TTY → color, pipe → no ANSI codes, NO_COLOR respected

### Threading APIs
- `bench_contended(name, threads, setup, work)` — barrier-synchronized
- `bench_parallel(name, threads, work)` — independent scaling
- `bench_scaling(name, work)` — probes 1..physical_cores automatically
- Gate disabled for threaded groups (our own threads ARE the load)

### Output formats
- `--format=llm` — key-value lines with `|` section separators, greppable
- `--format=csv|md|json` — all built-in
- Auto-save to `/tmp/zenbench/zenbench-{id}.txt` in LLM format
- Path printed at startup so tools can re-read without re-running
- Killed runs clearly marked INCOMPLETE

### Criterion compatibility
- `criterion_compat` module: zero-cost migration
- `criterion_group!` / `criterion_main!` macros
- `Criterion`, `BenchmarkGroup`, `BenchmarkId`, `Bencher` shims
- `iter`, `iter_batched`, `iter_batched_ref`, `BatchSize`
- Tested with real criterion-style benchmark code

### API safety
- `#[non_exhaustive]` on all result/config structs
- Removed unnecessary public exports (BenchFn, Benchmark, Engine, ResourceGate)
- Consolidated Throughput API (removed `compute_named`/`format_named`)
- Fresh gate per group (no shared mutable state)

## Architecture (updated)

```
src/
  lib.rs             — public API, main! macro, criterion macros
  bench.rs           — Suite, BenchGroup, GroupConfig, Bencher, Throughput
  criterion_compat.rs — drop-in criterion shim
  engine.rs          — Engine, measurement loop, convergence, auto-save
  results.rs         — SuiteResult, to_llm/csv/markdown, save/load
  report.rs          — terminal renderer (tables, bar charts, footnotes)
  format.rs          — format_ns, ns_unit, format_ns_range
  stats.rs           — Summary, PairedAnalysis, bootstrap_ci, Wilcoxon
  gate.rs            — ResourceGate, GateConfig, system health checks
  platform.rs        — SystemMonitor, timer_resolution_ns, CI detection
  checks.rs          — warning types (mostly unused, footnotes replaced)
  daemon.rs          — fire-and-forget subprocess mode
  mcp.rs             — MCP JSON-RPC server
  ci.rs              — CI environment detection
```

## What's next (priority order)

### Before 0.1.0
- [ ] Cross-run baseline comparison (`--save-baseline`, `--baseline`)
- [ ] Per-benchmark filter (not just per-group)
- [ ] Review daemon.rs and mcp.rs (0 tests, 1100 lines)
- [ ] Publish to crates.io (verify README renders, verify docs.rs)

### Near-term post-0.1
- [ ] Async benchmarking (`b.to_async(runtime)`)
- [ ] `iter_custom` for externally-timed operations (GPU, etc.)
- [ ] Configurable confidence level and resamples
- [ ] `Throughput::BytesDecimal` (KB=1000)
- [ ] Profiler hooks
- [ ] Pause/resume timer for mid-benchmark I/O exclusion

### Medium-term
- [ ] Asymptotic complexity analysis (Big O fitting)
- [ ] HTML/SVG plot generation
- [ ] Custom `Measurement` trait
- [ ] Process-level CPU time for threading efficiency
- [ ] `bench_scaling` efficiency/scaling columns in output

## Key design decisions to remember

1. **Significance = CI excludes zero.** No p-value thresholds. No hardcoded percentages. The CI is the source of truth.

2. **MAD not stddev.** One context-switch spike destroys stddev. MAD is robust.

3. **Cache firewall off by default.** Most benchmarks measure hot-path code. Enable for cold-cache comparison.

4. **Auto-rounds convergence requires TWO things:** direction resolved (CI excludes zero) AND effect size stable (CI half-width < 10% of difference for large effects).

5. **Fresh gate per group.** No shared mutable state between groups. Threaded groups get `GateConfig::disabled()`.

6. **Lower median for iteration estimate.** Not min (starves fast benchmarks) or max (bloats slow ones).

7. **Color = `should_color(&stream)`.** Per-stream, not cached. Respects NO_COLOR.

## Files to read first

1. `CLAUDE.md` — project instructions and architecture
2. `METHODOLOGY.md` — research cross-reference and design rationale
3. `README.md` — user guide with migration instructions
4. `src/engine.rs` — the measurement loop and convergence logic
5. `src/report.rs` — the terminal renderer (most complex file)
