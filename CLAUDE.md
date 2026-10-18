# Zenbench Project Instructions

### Known bugs: defaults audit (2026-10-11)

Review-stack CI also observed timing-dependent failures in
`baseline_detects_regression_with_different_workload` on macOS ARM64 and
`iter_deferred_drop_runs` on macOS Intel. Full failed-job logs were retained;
the failed jobs were rerun with unchanged assertions. These live performance
assertions are distinct from the deterministic workflow-help checks.

Workflow audit: macro benchmark binaries rejected `--help` before registration;
help now returns before plan/topology validation. CLI baseline-list guidance now
includes required confirmation/workload flags. `zenbench run --wait` still waits
for its initial Cargo child before applying the metadata-wait timeout and does
not preserve confirmation's 0/1/2 exit meanings; direct foreground invocation is
the documented confirmation path. Runtime Git HEAD is retained, but clean/dirty
state and a source-to-binary build manifest are not automatically captured.


LLM output for retained multi-process aggregates labels call counts and timer
ticks as `per_process`. The aggregate's zero placeholders are not measured
zero calls or evidence of limited timer resolution; inspect retained children
for those values. A two-process regression covers the labels.

Evidence command metadata uses an explicit `<INPUT>` placeholder for filesystem
paths that cannot be represented in UTF-8, avoiding serialization panics. The
Windows readability fixture uses native joins for each path component; its
exact command-path assertion remains unchanged.

Executable hashing made debug tests under ARMv7 emulation expensive. The SHA-256
dependency now uses optimization level 3 in the development/test profile, leaving
the test and benchmark code unoptimized. A prebuilt dry-run command measured
7 seconds before and 1 second after; both executable hashes matched Python's
independent SHA-256. See `benchmarks/armv7_hash_2026-10-11.json`; these are emulated
debug startup observations, not release benchmark timings.

Windows x64 CI exposed an ancestor walk that could append its starting PID
when the observed parent links formed a cycle. The walk now stops before that
append, even when its output was initially empty. Deterministic self-cycle and
two-process-cycle regressions supplement the unchanged live parent assertion.

Known bugs identified by the defaults audit: multi-process aggregation retains
the first run's paired analyses after changing benchmark summaries; a synthetic
-20%/+20% pair averages to equal means but still reports -20%, significant.
Aggregate duration and gate counters also describe the first run. Child exchange
JSON is deleted and aggregate round samples are cleared. The per-round CPU
monitor can sleep for its 200 ms refresh interval without charging that time to
the reported gate-wait counter. See `docs/defaults-audit.md` for the source audit,
measurement procedure, proposed modes, and durable storage/diff contract. These
findings are not fixed by the affinity implementation.

Aggregation correction: descriptive aggregates now clear within-run analyses,
retain original runs in `replicates`, sum duration/gate/round accounting, and
reject mismatched benchmark inventories. The opposed-effect regression verifies
that the first run cannot supply a false aggregate significance claim.
Process-aware confirmation inference is a separate implementation step.

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
  paths.rs     — where results, exchange files and locks live (target dir / user cache)
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
- **Within-process significance is exploratory**: the paired bootstrap interval must clear the configured noise margin (default 1%). Confirm/Evidence decisions use the recorded process-level policy and practical margin.
- **Bootstrap percentile CI** (10K resamples, 2.5th/50th/97.5th). Non-parametric, captures asymmetry.
- **MAD not stddev** in display. Stddev is destroyed by one context-switch spike.
- **Cache firewall off by default**. Most benchmarks measure hot-path code; firewall penalizes pointer-chasing unrealistically.
- **Auto-rounds convergence** based on paired-difference CI + effect-size stability, not individual benchmark precision or p-value targets.
- **Durable gzip evidence** lives under ignored `.zenbench/data/` or `ZENBENCH_DATA_DIR`, with immutable hash objects, process attempts and small references. Explicit selected CSV snapshots belong in Git. See `docs/result-storage.md`. Self-compare build workspaces stay under target; cross-process locks stay in the user cache.
- **Modes:** Iterate is exploratory (30 rounds, 100 ms warmup per arm by default). Confirm/Evidence use fixed fresh processes and the process-level inference in `src/inference.rs`; see `docs/benchmark-modes.md`. Legacy within-process significance and aggregate descriptive means cannot authorize promotion.

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
- Iterate convergence can resolve direction or a sufficiently narrow interval; larger effects also require stable effect size. Confirmation disables adaptive stopping and requires the full fixed plan.

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
- Auto-saved as compressed raw evidence plus a hash-verified reference in `.zenbench/data/`

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
- `gate::tests::own_benchmark_named_thread_does_not_block_gate` asserts zero rival waits, so it fails whenever another process named like a benchmark (`criterion`, `divan`, `zenbench`, `cargo-bench`, `bench-`) runs on the box. Seen 2026-10-06 on r5900xt at load ~35: 6 waits, then 133/133 and 5/5 reruns passed once the box was quiet. Not a gate bug; rerun on a quiet box before treating it as a regression.
- The `daemon` feature gates nothing: `pub mod daemon` compiles unconditionally.

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

### Known Bugs (SPEEDQ)

Windows CI observed identical clock readings in concurrent fleet CLI tests,
colliding their scratch directories before any assertion ran. Fixture names
now include a process-local atomic sequence as well as PID and timestamp.

2026-10-12: Windows x64 and ARM64 integration runs failed admission with
`Access is denied` while an earlier reservation completed. Cleanup deleted
ticket/guard files outside the queue mutex while waiters could open them. Cleanup
now takes the same mutex, with a regression requiring the record and guard to
remain live until a scanner releases it. Cleanup failures retain the running
record and fail closed; no admission error is reinterpreted as permission.

The external-pair subprocess fixture must start its line protocol with a newline.
Single-threaded libtest (including the ARMv7 cross runner) prints an unterminated
`test ... ` prefix; without that newline the first metric/check row is not a
protocol row. Retained child stdout reproduced the missing-cardinality failure.
The fixture now frames its output; extractor expectations and deadlines stay intact.

2026-10-11: macOS ARM CI at fdb4a0f4 failed the unchanged no-default-feature
`iter_deferred_drop_runs` timing assertion: deferred mean 656937 ns versus
regular mean 170814 ns. The test excludes deferred deallocation but retains
all allocated outputs during its batch, unlike regular iteration. The cause
of this particular failure has not been established; assertions remain unchanged.

The affinity environment regression test originally relaunched its ARMv7
executable directly under cross's QEMU user-mode runner. QEMU does not intercept
the guest's execve, so the child failed with an executable-format shell error.
Cross.toml now explicitly supplies ZENBENCH_AFFINITY_TEST_RUNNER=qemu-arm for
that target; every fresh-process environment case still executes and asserts
the requested mask and restoration. Native callers use direct execution.

Foreign Linux process leaders and their user-thread entries were counted as
separate heavy processes. A live sleeping-thread regression reproduced a count
of two for one foreign owner before the correction. The gate now counts each
leader once and keeps foreign thread CPU in global CPU admission. Two distinct
foreign owners still count as two; admission thresholds remain unchanged.

Registered external benchmark workers previously counted as foreign CPU/heavy
processes in SystemMonitor even though the concurrent-benchmark scan excluded
those same launcher PIDs. Both scans now honor that explicit inventory;
foreign processes, RAM and temperature remain checked. Pure admission tests
cover MT percentage units and a foreign negative control. No burn test used.

Linux resource admission must also exclude tasks belonging to explicitly registered worker leaders from heavy-process counts. Subtract CPU using leaders only; their CPU usage aggregates tasks, so subtracting task counters again would hide foreign load. The negative control keeps an unregistered heavy process visible.

Without an explicit nonempty worker PID inventory, SystemMonitor retains global CPU-load behavior, including its caller. The gate_sees_load integration test depends on this contract; ownership subtraction is opt-in through the launcher.

CPU-policy immediate-mode comparisons must attach the coordinator mask before
checking compatibility with prior samples. Comparing a completed placement
record against a process-only record incorrectly rejected the second benchmark
in a Criterion-compatible group; the existing integration test caught it.

### Native repeatability runner regression (2026-10-11)

The development Windows runner initially used the unhandled-exception flag where
it intended a per-process memory limit. Its total job memory cap remained active.
Native limit readback caught the missing per-process cap. The runner now sets the
SDK per-process flag as well and verifies all limits before starting children.

### Repeated execution schedules (2026-10-11)

Twenty native Windows processes repeated the same 200-round permutation sequence
in each group because the engine used a constant shuffle seed. A fresh-process
regression failed with that seed and passed after each group received an
independent RandomState seed. Bootstrap resampling stays deterministic; actual
round permutations remain in results for inspection and replay.

### Windows process-mask mutation (2026-10-11)

A process-mask boundary can be widened by code that calls SetProcessAffinityMask;
the native restoration controls exercise that behavior. Strict Windows admission
now holds an affinity-limited Job Object for the process lifetime. The native Rust
fixture checked existing Rayon/ordinary workers and an inherited child process.
Windows can return success for a widen request while preserving the job mask;
an unchanged actual mask is the confinement evidence.

### Durable result storage (2026-10-11)

New automatic evidence lives under ignored `.zenbench/data/`, configurable with
`ZENBENCH_DATA_DIR`, as compact gzip objects and hash-verified references.
`zenbench snapshot` exports a selected small CSV; do not commit every raw run.
Partial checkpoints and failed process outputs remain available. See
`docs/result-storage.md`. Exact JSON float parsing prevents the one-bit drift
found by the independent gzip/Python round-trip check.

### Background resource observations (2026-10-11)

The engine now starts an owner-aware resource sampler before group warmup. Its
first valid CPU delta is accounted as gate wait; subsequent rounds only read a
cached observation and retain its age. Observations older than one second are
unknown and flag the run. Thread-tagged groups keep monitoring enabled. Rival
benchmark admission is checked before warmup; a timeout flags the result, and
exclusive-lock failure also flags it. The public standalone SystemMonitor keeps
its blocking first-snapshot/global-load contract. Monitoring still observes host
CPU averages, not exclusive use of a selected CPU/cache domain.

Cross CI now supplies `XDG_CACHE_HOME=/target/zenbench-ci-cache`: its container
HOME was read-only, so exclusive-lock acquisition failed with EACCES. Once lock
failure correctly flagged results, the clean-gate integration assertion exposed
that environment defect. The target mount is writable and a native container
control acquired an exclusive OS lock there; assertions remain unchanged.

### Confirmation controls (2026-10-11)

`examples/check_protocol.py` retains every control attempt, including invalid runs.
One local Evidence control recorded 30 flagged rounds in one child and correctly
refused a verdict. Cached observation diagnostics are now retained with rounds
so future refusals include the resource values/reason. Never weaken admission to
make a known-work control pass. Approved deterministic shuffle tests replace the
old probabilistic requirement to change order within ten random two-arm rounds.

Extreme confidence/family settings can make a process interval unrepresentable.
Confirmation validates that every required interval was produced; missing numerical
results cannot become a successful exit or baseline promotion. A deterministic
large-dynamic-range control exercises that refusal.

Baseline/candidate comparisons must use disjoint child run identities. Reusing the
same saved measurement on both sides cannot establish an independent confirmation.

The CLI evidence integration test must honor `ZENBENCH_AFFINITY_TEST_RUNNER`
like the existing CPU-policy subprocess tests. Directly launching its ARMv7
executable under cross returned a launcher error rather than exercising the CLI.
The expected CLI status and diagnostic remain unchanged.

Fleet CLI verification must include binary test targets in clippy and Windows
MSRV checks: checking only `--bin zenbench` missed test-module ordering and a
Windows-only overwritten fixture variable. Both were corrected without changing
assertions.

### Known Bugs: Windows ARM64 topology discovery

Native Snapdragon controls at `6a798e09` passed heap phases, fleet CLI and
inherited Job Object confinement, but the capability probe reported unknown
CPU topology because Windows discovery only compiled the x86 CPUID path.
The independent native oracle reports two efficiency classes and three shared
L2 domains. Default placement correctly refuses the unknown result. Evidence:
`benchmarks/winarm_native_controls_2026-10-12.json`.

The approved Windows ARM64 topology backend now acquires the OS inventory and
uses a bounds-checked parser shared with captured-buffer tests. Native CI checks
core/cache discovery against the independent PowerShell oracle and exercises
class selection and existing Rayon confinement. Multiple groups remain refused.

### Known Bugs: release compatibility review (2026-10-12)

At 9d84e8b2, native Linux `cargo semver-checks` against published 0.1.10 reports
added fields on the exhaustively constructible `BaselineComparison` and lost
`UnwindSafe`/`RefUnwindSafe` on `ResourceGate`. Both default and expanded feature
checks reproduced these source-compatibility failures. The cleanup restores the
published result layout, puts confirmation in `BaselineAssessment`, and places
the monitor handle behind a mutex unwind boundary. The same expanded-feature
semver check now passes all 202 applicable checks;
see [the release surface review](docs/public-api/review-0.1.10.md) for scope,
reproduction commands, behavioral migration and CLI consistency findings.

### macOS fixture isolation (2026-10-12)

Concurrent fleet CLI scenarios can saturate the hosted runner while hashing bundles,
causing real resource admission to expire. The scenarios now serialize without
changing admission thresholds or assertions. A live Rayon pool can asynchronously
release scheduler allocations during later exact heap-ledger controls; those
controls now run before pool creation. CI exposed a 48-byte live-ledger difference
on macOS Intel; the exact assertions remain unchanged.

### Known Bugs: site and chart discoverability (2026-10-12)

The guide index's right TOC only offered section anchors, and GitHub was hidden
inside a dropdown. It now offers direct guide destinations and a visible header
repository link at mobile and desktop widths. Browser validation must click guide
links and assert the destination, not only assert link visibility.

SVG export existed on the legacy results command but was absent from the saved
packet reader. `data read` now exports HTML and SVG from native suite JSON, gzip
and evidence references, fails on export I/O errors, and explicitly rejects pair
packets for these suite renderers. Themed chart bars show means; they do not draw
confirmation confidence intervals. Matrix MAD comments in themed SVG source are
not visible error bars. Use the chart guide to keep these quantities distinct.

### Known Bugs: external exit-window admission (2026-10-18)

Confirm's resource sampler counted a reaped worker leader's final-window CPU as
foreign: exited processes cannot be subtracted, so a window refreshed after
reaping showed the worker's own CPU. On r7900x an 8-thread single-process
control child showed 0.30-0.35 whole-host CPU in 13 of 15 windows completing
after its exit (two still assigned the leader), while all 45 live windows read
below 0.03. Only windows completing at or after the supervisor's owned exit
observation (after `try_wait` returns the status, before cleanup) are now
`exit_transition`; windows after the last live probe stay subject to admission,
and missing lifecycle evidence invalidates. Transitions waive only the
`cpu_load` magnitude: heavy foreign processes, RAM, temperature, freshness and
finite CPU stay enforced. The zombie-oracle lifecycle test is Linux-only; the
pair_cli wiring test runs everywhere. Residual: the window whose counter refresh
spans reaping is not admitted by CPU magnitude even when it still assigned the
leader.
