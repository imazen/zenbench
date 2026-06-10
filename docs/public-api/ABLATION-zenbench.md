# ABLATION REPORT — zenbench

**Date:** 2026-06-11  
**Snapshot commit:** 585299a (main; "feat: versioned public-API surface snapshots")  
**Snapshot file:** `docs/public-api/zenbench.txt`  
**Total public items (default features):** 1,080  
**Total public items (all features):** 1,279  
**Grep template:**
```
ugrep -rn "TERM" /home/lilith/work/zen/ \
  --exclude-dir=target --exclude-dir=.jj --exclude-dir=retired \
  --exclude-dir=perm-corpus --include="*.rs" \
  | grep -v "^/home/lilith/work/zen/zenbench/"
```

**Confirmed external consumers:** linear-srgb, zenresize (multiple benches), jxl-encoder (multiple benches), rav1d-safe (multiple benches), zenavif (multiple benches), zenbitmaps, zencodec, zenflate, fast-ssim2, whereat, zenanalyze/zenpredict-bake, zenmetrics/cpu_wall, zenmetrics/butteraugli-gpu, BRAG, cpu-backend-bench, zenjpeg/zenyuv, zensim-regress.

---

## Summary

| Verdict | Count | Notes |
|---------|-------|-------|
| KEEP (confirmed external consumers) | ~7 items / modules | `prelude::*`, `criterion_compat`, `main!`, `run()`, `black_box`, `Throughput`, `postprocess_result`, `SuiteResult`, `run_gated`, `GateConfig`, `Suite` (as fn param type) |
| FLAG B — demote to `pub(crate)` | 7 modules | `daemon`, `mcp`, `charts`, `quickchart`, `exclusive`, `calibration`, `platform` |
| Observe / minor | 2 items | `engine_new` pub fn; `run_and_save`, `run_passes`, `parse_pass_args`, `parse_process_args`, `run_processes`, `aggregate_results`, `Aggregation` |
| Already internal | 0 | (no misidentified pub(crate) found) |

**Flag rate (conservative):** 7 modules out of ~12 pub modules = ~58% of module-level exposure is CLI-internal. Item-level flag rate is much lower (~100/1080 items ≈ 9%). See breakdown.

---

## The Core External API (confirmed consumers, KEEP)

These items have multiple confirmed external consumers across the org:

| Item | External consumers |
|------|-------------------|
| `prelude::*` (`Suite`, `BenchGroup`, `Bencher`, `GroupConfig`, `Throughput`) | jxl-encoder/benches, rav1d-safe/benches, BRAG/benches, zenanalyze/zenpredict-bake, zenmetrics/cpu_wall, zensim-regress |
| `criterion_compat::*` | linear-srgb/benches (3 files), zenresize/benches (3 files), zenavif/benches (3 files), fast-ssim2/benches, whereat/benches (4 files), zenflate/benches |
| `criterion_group!`, `criterion_main!` | Same as criterion_compat consumers |
| `main!(f)` macro | jxl-encoder/benches (3 files), rav1d-safe/benches, BRAG/benches, zenresize/benches, zenbitmaps, zencodec, cpu-backend-bench, zenanalyze/zenpredict-bake |
| `black_box(x)` | jxl-encoder/benches, zenmetrics/cpu_wall, zenmetrics/butteraugli-gpu, cpu-backend-bench, zencodec |
| `Throughput` | jxl-encoder/benches, zenresize/benches, zenbitmaps, zenmetrics/butteraugli-gpu |
| `run(f)` | cpu-backend-bench, zenmetrics/butteraugli-gpu (multiple examples) |
| `run_gated(gate, f)` | zenmetrics/cpu_wall (`GateConfig::disabled()`) |
| `GateConfig` | zenmetrics/cpu_wall — `GateConfig::disabled()` |
| `postprocess_result(&result)` | cpu-backend-bench |
| `SuiteResult` | cpu-backend-bench, zenmetrics/cpu_wall, zenmetrics/butteraugli-gpu |
| `SuiteResult::load` / `SuiteResult::save` | Used by internal CLI — external callers access it via `run()` return |
| `Summary::mean`, `Summary::min` | zenmetrics/cpu_wall accesses `bm.summary.mean` from `SuiteResult` |

---

## Flagged Items — CLI-Internal Modules (FLAG B)

The following modules are exclusively used by the `zenbench` and `zenbench-mcp` **internal CLI binaries** in `src/bin/`. Zero external callers found in any org crate.

### B: `daemon` module

**Contents:** `RunStatus`, `RunState`, `runs_dir`, `lock_path`, `list_runs`, `save_run_state`, `load_run_state`, `kill_stale_runs`, `kill_run`, `is_process_alive`, `cleanup_old_runs`, `spawn_detached`, `spawn_fire_and_forget`, `wait_for_run`, `find_latest_with_results`, `result_path_from_env`

**Evidence:** 0 external consumers. Only `src/bin/zenbench.rs` imports `zenbench::daemon`. This is fire-and-forget subprocess management for the CLI's multi-process benchmark orchestration.

**Recommendation:** `pub(crate) mod daemon`.

---

### B: `mcp` module

**Contents:** `run_server(PathBuf)` function

**Evidence:** 0 external consumers. Only `src/bin/zenbench-mcp.rs` calls `zenbench::mcp::run_server(project)`. This is the JSON-RPC MCP server for IDE integration — an internal binary feature.

**Recommendation:** `pub(crate) mod mcp`.

---

### B: `charts` module

**Contents:** `ChartOrientation`, `ChartConfig`, `comparison_to_svg`, `save_charts`

**Evidence:** 0 external consumers. Only `src/bin/zenbench.rs` imports `zenbench::charts::ChartConfig` and `ChartOrientation` for the `--charts` CLI flag.

**Recommendation:** `pub(crate) mod charts`.

---

### B: `quickchart` module

**Contents:** `QuickChartUrl`, `ColorScheme`, `QuickChartConfig`

**Evidence:** 0 external consumers. Only used internally by the charts and report rendering machinery.

**Recommendation:** `pub(crate) mod quickchart`.

---

### B: `exclusive` module

**Contents:** `AcquireConfig`, `HolderInfo`, `Lock`

**Evidence:** 0 external consumers. Used internally for file-based locking in the daemon/baseline management path. The `exclusive::Exclusive` hit found (`naga-metal-msl-repro/vendor/wgpu/`) is a different, unrelated `exclusive::Exclusive` from wgpu's internal module — not `zenbench::exclusive`.

**Recommendation:** `pub(crate) mod exclusive`.

---

### B: `calibration` module

**Contents:** `Calibration` struct, `run_calibration()` fn

**Evidence:** 0 external consumers. `Calibration` appears as `Option<crate::calibration::Calibration>` in `SuiteResult::calibration` — a struct field. External callers receive `SuiteResult` from `run()` and access `.comparisons`, `.benchmarks`, `.summary.mean` etc., but nothing has been seen accessing `.calibration` from external code. The field is populated by the internal engine.

**Recommendation:** `pub(crate) mod calibration`. The field on `SuiteResult` (`pub calibration: Option<Calibration>`) would need to change type to hide it — see Dependency note below.

**Dependency note:** `SuiteResult::calibration` is typed as `Option<Calibration>`. Demoting the `calibration` module to `pub(crate)` would require either:
1. Changing `SuiteResult::calibration` field type to `Option<()>` or removing it (breaking change for 0.2.x), OR
2. Keeping the field but using `#[doc(hidden)]` on the module while the field's type remains accessible.

**Conservative recommendation:** Add `#[doc(hidden)]` to `pub mod calibration` rather than demoting to `pub(crate)`, since the field type reference forces the module to stay reachable. File as a "hidden until externally needed" status.

---

### B: `platform` module

**Contents:** `SystemState`, `SystemMonitor`, `Testbed`, `detect_testbed()`, `timer_resolution_ns()`, `detect_ci()`, `git_commit_hash()`, `git_short_hash()`

**Evidence:** 0 external consumers of the module itself. `Testbed` is re-exported at `zenbench::Testbed` (confirmed in lib.rs:150: `pub use platform::Testbed`). The re-export means the type is accessible without going through `zenbench::platform::Testbed`. External callers that wanted `Testbed` would use `zenbench::Testbed` not `zenbench::platform::Testbed`.

`SystemState` and `SystemMonitor` are implementation details of the gate/resource monitoring subsystem. Only the CLI binary uses `platform::git_commit_hash()` and `platform::git_short_hash()` directly.

**Recommendation:** `pub(crate) mod platform`. Keep `pub use platform::Testbed` at the crate root (via re-export, it stays accessible as `zenbench::Testbed`).

---

## Minor Observations (not flagged)

### `engine_new` pub fn (lib.rs:145)

```rust
pub fn engine_new(suite: Suite) -> engine::Engine;
```

**Evidence:** 0 external consumers. This is a low-level escape hatch into the internal engine type. No caller needs to drive `engine::Engine` directly; `run()` and `run_gated()` are the intended entry points.

**Recommendation:** Demote to `pub(crate)`. Low priority since `engine::Engine` itself is not a `pub` type (it comes from `pub(crate) mod engine`), so callers can't do much with the return value even now.

---

### `run_and_save`, `run_passes`, `parse_pass_args`, `parse_process_args`, `run_processes`, `aggregate_results`, `Aggregation`

**Evidence:** 0 external consumers found. These are multi-process orchestration primitives for the daemon's process-parallel benchmark mode. The `main!` macro internally calls `parse_pass_args` / `run_processes` for the `--process` CLI mode; the external consumer is always just `main!()`.

**Recommendation:** Consider demoting these to `pub(crate)` and making `main!()` the only supported external multi-process interface. However, since these names are documented in README/METHODOLOGY as extension points, and the zenbench 0.1.x series has multiple external consumers, defer any changes to a version bump with proper deprecation notice.

---

### `MeanCi`, `PairedAnalysis`, `Summary`, `BenchmarkResult`, `ComparisonResult`, `RunId`

**Evidence:** These are accessed indirectly through `SuiteResult`. `zenmetrics/cpu_wall.rs` accesses `.benchmarks[].summary.mean` on `ComparisonResult`'s `benchmarks: Vec<BenchmarkResult>` field. The external callers need these types to be pub in order to dereference the `SuiteResult` struct.

**Status:** KEEP as public — they are load-bearing types for external SuiteResult introspection.

---

## Confirmed-KEEP Modules

| Module | Status |
|--------|--------|
| `prelude` | KEEP — the primary external interface |
| `criterion_compat` | KEEP — widely used |
| `baseline` | PARTIAL — external callers never call `baseline::*` directly, only via CLI. But `BaselineComparison`, `BenchmarkDelta` may be pub-necessary if any external code reads SuiteResult baselines. Low priority to investigate further. |
| `wasm` | Likely CLI/internal — no external consumers found. Leave for a follow-up pass. |

---

## Recommended Change Set (0.2.x-safe)

All changes below are `pub → pub(crate)` on modules with zero external consumers. These are semantically breaking (removes public path), but since no downstream crate uses these paths, the change is safe under the zenbench 0.1.x series with a minor bump:

```
pub(crate) mod daemon;
pub(crate) mod mcp;
pub(crate) mod charts;
pub(crate) mod quickchart;
pub(crate) mod exclusive;
pub(crate) mod platform;  // with pub use platform::Testbed remaining
```

For `calibration`: use `#[doc(hidden)] pub mod calibration` since `SuiteResult::calibration` references the type.

For `engine_new`: demote to `pub(crate) fn engine_new`.

Batch these into one release so the minor version bumps once.
