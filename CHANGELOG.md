# Changelog

## [Unreleased]

- Resource admission accounts for explicitly registered launcher/worker CPU ownership; foreign work and global RAM/temperature gates remain visible. A private development example reuses engine paired statistics for external-worker rounds (cca46093fe78).

### QUEUED BREAKING CHANGES
<!-- Breaking changes that will ship together in the next minor release.
     Add items here as you discover them. Do NOT ship these piecemeal — batch them. -->

### Changed
- **Nothing goes to the temp dir any more.** Auto-saved results move from `/tmp/zenbench/` to `<target>/zenbench/results/`, `--best-of-processes` exchange files to `<target>/zenbench/proc/`, and `self-compare`'s git worktree and builds to `<target>/zenbench/self-compare/`; the engine lock (`zenbench.lock`) and `exclusive::Lock::default_path()` move to the per-user cache dir (`~/.cache/zenbench/`, `%LOCALAPPDATA%\zenbench\`), so the rendezvous no longer follows `TMPDIR`. Versions up to 0.1.10 still lock in the temp dir, so mixed old/new runs on one box don't wait for each other until both sides upgrade (c62baca).
- `self-compare` prunes stale git worktree registrations before creating its worktree, so a `cargo clean` can't make `worktree add` refuse the path (c62baca).
- Lockfiles refreshed within requirements: `arbitrary` 1.5.0, `either` 1.19.0. `sysinfo` 0.39 needs Rust 1.95 (0.37+ needs 1.88) and stays at 0.36 under the 1.85 MSRV (a50173c).
- README: throughput is per group; a second `throughput()` call replaces the first (b4e7f4f).

## [0.1.10] - 2026-10-06

### Fixed

- Exclude explicitly registered workers’ Linux tasks from rival counts without double-subtracting CPU usage.

- Propagate strict resource-gate failures into streamed and saved `SuiteResult::unreliable`; the engine previously always left that field false. Retained rounds now record `gate_clean` as checked-clean, flagged, or disabled/unknown. A forced insufficient-RAM regression fails before the correction and passes afterward. Multi-run aggregation preserves any input unreliability. Strictness permits a configured number of noisy checks, so the suite flag alone is not proof that every round was clean (5394102).
- Exclude the current process's Linux task IDs from the concurrent-benchmark scan. The exclusive-lock heartbeat's thread name matched the benchmark filter, making the harness wait for itself for 30 seconds per round. A live named-thread regression reproduced 29 waits before the fix and zero afterward; other processes and their tasks remain eligible (1bf8a65).
- **`ResourceGate`'s CPU-load check never fired — `max_cpu_load` was dead config for the life of the gate.** `sysinfo` computes `cpu_usage()` as a delta between two refreshes, so a refresh landing inside `MINIMUM_CPU_UPDATE_INTERVAL` (200 ms) of the previous one reports **0.0% on every core regardless of actual load**. `ResourceGate::new(cfg).check()` constructs a `SystemMonitor` and snapshots immediately, so `cpu_load` was always exactly 0.0 and the threshold could never trip. Measured on a box at loadavg 2.65: first snapshot 0.000, second 0.059. Caught when a five-box zensysbench comparison gated **0 of 205 cells** while the dev box sat at load 17 — a gate that certifies a contaminated box as clean is worse than no gate. `SystemMonitor::snapshot()` now tracks the last CPU refresh and sleeps the remainder of the minimum interval before re-reading; the ≤200 ms cost is confined to gate checks (`gate.rs` holds the only `snapshot()` callers). Regression test `tests/gate_sees_load.rs` saturates every core and asserts both that the first snapshot sees it and that the gate refuses the box. RAM and heavy-process checks were unaffected and did work (c80801b).
- Flaky-test fix: `overhead_compensation_produces_lower_times` compared the noop mean against 2× the loop overhead sampled at startup, which failed on contended macOS CI runners when VM speed drifted between the two (2026-09-14 macOS Intel; both macOS ARM64 attempts for the 0.1.10 release commit). It now checks compensation exactly from the retained rounds: compensated = max(raw − overhead × iterations, 1), strictly below raw, with the reported mean built from the compensated values. A mutant engine without the subtraction fails it (6084c0a).

### Added
- `--no-busy-gate` / `ZENBENCH_NO_BUSY_GATE=1` disables the resource gate and rival-benchmark wait, overriding any harness `GateConfig` — for benchmarks whose own worker threads trip the gate (8f7bdb4, bf4a0e3).
- Retain completed interleaved rounds in `ComparisonResult::samples`, including actual iteration counts, randomized execution order, and raw/overhead-compensated durations. `RoundSample` lets the Zensim speed benchmark audit individual-call latency from its existing JSON export. Historical results deserialize with empty samples; multi-run aggregation clears samples because its arms no longer represent one paired execution (16fb8fd).
- Export `ResourceGate` and `GateReason` at the crate root alongside `GateConfig`, so external harnesses (first consumer: zensysbench) can run the busyness gate around their own child-process measurements. The methods were already annotated "Public API for external gate users"; this makes them reachable. Additive, no behavior change (645002a).

### Changed
- Regenerated API snapshots for round retention; this also records the already exported `ResourceGate` / `GateReason` surface that was missing from the prior snapshot (16fb8fd, 5394102).
- Refreshed `Cargo.lock` within the existing requirements (`cargo update`). Entirely third-party: zenbench's lock contains no zen-family crate, so nothing here touches the in-flight ecosystem requirement work. Notable movers `cc` 1.2.64 → 1.4.4, `regex` 1.12.4 → 1.13.1, `tokio` 1.52.3 → 1.53.1, `wasm-bindgen` 0.2.123 → 0.2.127, `serde` 1.0.228 → 1.0.229, `libc` 0.2.186 → 0.2.189 (28e28ec).
- Bumped `charts-rs` 0.3.28 → 1.0.0 (optional `charts` feature). The `BarChart`/`HorizontalBarChart`/`Series`/`Align` surface `src/charts.rs` uses is unchanged across the major, so no code change was needed (baeeb05).
- Bumped `wasmtime`/`wasmtime-wasi` 48 → 49.0.2 (optional `wasm` feature); no source change needed. Added `tests/wasm_smoke.rs`, the first runtime test of `zenbench::wasm`: WAT compile, plain and WASI p1 instances, simd128 lanes, and the unresolved-import error path. It passes on x86_64 and on i686 via `cross`, where wasmtime has no Cranelift backend and uses the Pulley interpreter (611c90f).
- Bumped `charts-rs` 1.3 → 2.0.0 (optional `charts` feature). `src/charts.rs` moves six fields to 2.0's grouped option structs (`title_text` → `title.text`, …). Output is byte-identical: 36 charts rendered at charts-rs 1.3.0 and 2.0.0 (3 fixtures × 2 orientations × 3 themes × labels on/off) match byte for byte (bced140).
- Raised requirement floors to the lock-tested versions: `serde` 1.0.229, `serde_json` 1.0.151, `clap` 4.6.7, `tokio` 1.53.2. The lock already resolved to these, and all four build on Rust 1.85 (f738573).
- CI: `actions/checkout` → v7, `codecov/codecov-action` → v7, `actions/upload-pages-artifact` → v5, `actions/deploy-pages` → v5 (b48f08b). The MSRV job now also checks `--all-targets --features async,cpu-time,criterion-compat` on 1.85; `--all-features` cannot resolve on 1.85 because `charts` needs 1.88 and `wasm` 1.96 (c9b414b).

### Documentation
- README: new Cargo features table with each feature's default state and measured build MSRV (dcffa1d). Moved the "Auditable latency samples" section above the crosslink footer, and regenerated `README.crates.md`, which had drifted and lacked both `--no-busy-gate` and that section (eb6b06c).

### Compatibility
- **`wasm` feature users: the re-exported `wasmtime` moves from major 43 (0.1.9) to 49.** Code that names wasmtime types through `zenbench::wasm::wasmtime`, `WasmBench`/`WasmInstance` signatures, or `WasmInstance::store` and mixes them with its own `wasmtime = "43"` dependency must move to wasmtime 49. Shipped as a patch release by owner decision (2026-10-06): 43 sits under the wasmtime advisories listed in Security, and the change is confined to the optional, non-default feature. `cargo semver-checks` passes (196 checks) because it does not track re-exported crate majors.
- `wasm` feature: the wasmtime/wasmtime-wasi 43 → 44 → 48 → 49 bumps change the re-exported `wasmtime` major (`pub use wasmtime` at `zenbench::wasm::wasmtime`, plus the wasmtime types in `WasmBench`/`WasmInstance` signatures and the public `WasmInstance::store` field) and raise the `wasm`-feature build MSRV to Rust 1.96. The crate's default-build MSRV is unchanged (1.85), since wasmtime is only pulled in by the optional `wasm` feature (e521d9d, baeeb05, 611c90f).
- `charts` feature: `charts-rs` 0.3 → 1.0 → 2.0 raises the `charts`-feature build MSRV to Rust 1.88. `charts_rs` types are not re-exported, so this is a build-requirement change only, not an API break. Default-build MSRV unchanged (baeeb05, bced140).

### Deferred
- `sysinfo` stays at 0.36.1 (latest 0.39.6). Every 0.37+ release declares a `rust-version` above zenbench's 1.85 — 0.37.x and 0.38.x want 1.88, 0.39.x wants 1.95 — and `sysinfo` is a default, non-optional dependency that the MSRV (1.85) CI job compiles, so taking it would break that job. Owner decision (2026-10-06): keep MSRV 1.85 for 0.1.10.
- `criterion` resolves to 0.7.0 (latest 0.8.2) with no manifest change needed: the dev-dependency requirement is already `>=0.7.0, <0.9.0`, and the MSRV-aware resolver holds 0.7.0 because 0.8.x declares `rust-version` 1.86 > 1.85. It moves on its own once the declared MSRV rises.

### Security
- Moved `wasmtime`/`wasmtime-wasi` off 48.0.1, which the 2026-09-24 and 2026-10-02 wasmtime advisory batches cover, among them GHSA-32h6-97mm-8q3c (critical: unvalidated async-lifted callback result count in the component model). A lock refresh took 48.0.5 (da8e002), then the requirement moved to 49.0.2 (611c90f); both carry the fixes. Only the optional `wasm` feature pulls wasmtime. zenbench runs trusted benchmark modules, not untrusted guests, and uses core modules rather than components, so practical exposure was low.
- Bumped `wasmtime`/`wasmtime-wasi` 43.0.2 → 44.0.3, clearing the `wasmtime-wasi` advisory (WASI `path_open(TRUNCATE)` bypassed the host `FilePerms::WRITE` restriction; dependabot high). Only the optional `wasm` feature pulls wasmtime, and zenbench runs trusted benchmark wasm rather than sandboxing untrusted guests, so there was no practical exposure — the bump keeps the dependency current regardless (e521d9d).

## [0.1.9] - 2026-07-14

### Added
- Versioned public-API surface snapshots under `docs/public-api/` (`zenbench.txt` full surface, `zenbench.features.txt`, `zenbench.internal.txt`), produced by a self-contained `apidoc/` package (`zenbench-apidoc`, depending on `zenutils-apidoc` 0.1.0). The package carries its own empty `[workspace]`, so plain `cargo test` and every CI job never compile it or invoke rustdoc; regenerate with `just api-doc` and verify the committed snapshots with `just api-doc-check` (`ZEN_API_DOC=check`) (585299a, d15efb8, 0b3977d).

### Changed
- Extended `exclude` in `Cargo.toml` to drop `.gitignore`, `benches/`, `tests/`, `docs/`, `site/`, and `CHART-GALLERY.md` from the published tarball; declarations and local builds are unaffected (e1d928c).
- Refreshed the dependency lockfile via `cargo update` (cca28b3).

### Fixed
- Resource gate no longer flags its own launcher chain as a concurrent benchmark. `cargo bench --bench <name>_zenbench` carries the harness name in its command line, so the gate matched its own parent `cargo` process and stalled `max_wait` on every round — leaving ~4 surviving rounds per group, deterministically, on every machine. The concurrent-benchmark scan now excludes the entire ancestor PID chain (ancestors are blocked waiting on the harness, so they cannot be concurrently *running* benchmarks); genuine sibling benchmarks under the same shell are still detected (67ed629).
- Resource gate matches the harness patterns against a process's NAME and argv[0] basename only, never its full joined command line. The old full-cmdline scan false-positived on any unrelated process that merely *named* a benchmark directory in its arguments — most painfully a periodic backup `rsync --exclude=criterion/ --exclude=cargo-timing-*.html …`, which a non-ancestor so `collect_ancestors` couldn't exclude, stalling the gate the full 30 s every round. A real concurrent bench binary carries the pattern in its own invoked path (`…/deps/foo_zenbench-<hash>`), so this preserves genuine sibling detection (55692bf).
- MSRV fix: the `collect_ancestors` test used a `let`-chain (`if let … && let …`, stabilized in Rust 1.88) that failed to compile on the declared MSRV of 1.85; rewritten as nested `if let` (55692bf).
- Flaky-test fix: `exclusive::tests::heartbeat_advances` asserted the heartbeat advanced after a single fixed 1.2 s sleep, which failed intermittently on loaded ARM64 CI when the heartbeat thread was starved past that window. It now polls (up to a 10 s timeout) for the timestamp to advance — same guarantee, no timing flake (9895be5).
- `run_calibration()` now honors its ~50 ms contract on slow/emulated hardware. The fixed 10M-iter / 1M-step workloads took >5 s under qemu (armv7 cross CI) and failed `calibration_is_fast`; each workload now probes per-iter cost and scales its rep count to a ~25 ms budget, capped at the previous constants. Native hardware runs the identical counts; slow/emulated targets get proportionally fewer iterations (b767c63).
- Fixed the non-compiling `sort_by_speed` README example (`g.sort_by_speed()` → `g.config().sort_by_speed(true)`; the no-arg form is criterion-compat only), and documented the significance rule (95% CI excludes zero **and** exceeds the timer-resolution floor), the regression gate's significance-gating (`--max-regression=N` fails only on a `>N%` slowdown that is also t-test significant), DCE/`black_box` guidance (returning the value defeats DCE), and throughput per-call semantics — gaps found by an external-developer usability test (332b136).

### Documentation
- README overhaul + crates.io README split: standardized the badge row (CI `&label=CI`, MSRV 1.85, `#license` anchor), documented the `--format=html` self-contained SVG report and `charts`/`quickchart` output modes, moved License above the crosslink footer and refreshed it to repo links, and generated `README.crates.md` (`readme = "README.crates.md"`) so crates.io shows a trimmed CI-badge-only page while docs.rs keeps the full README (5e37355, 8849e10).

## [0.1.8] - 2026-04-29

### Added
- `zenbench::exclusive::Lock` — public cross-process bench mutex with heartbeat sentinel file. Acquiring writes a small text record (`pid`, `hostname`, `project`, `binary`, `benchmark`, `start`, `heartbeat`, `eta`) to a sibling `.info` file; a background thread refreshes the heartbeat every 5 s. Other processes waiting on the lock read it via `Lock::peek()` and print accurate "waiting on …, ETA in 4 m" messages every 15 s. Cross-platform (Linux/macOS/Windows); compiles to no-op stubs on `wasm32` (66f069e, 4e37d8f).
- `tests/crash_stale_lock.rs` integration test — verifies that the kernel releases the fs4 advisory lock the instant the holder process dies (SIGKILL on Unix, `TerminateProcess` on Windows), confirming the load-bearing assumption that no stale-lock recovery code is needed (d1098e8).

### Changed
- Engine now uses `exclusive::Lock` for cross-process coordination, automatically populating the holder file with the project name (`CARGO_PKG_NAME`), bench binary (`CARGO_BIN_NAME`), current bench group, and an extrapolated suite-completion ETA refined after each group (66f069e).
- Migrated `fs4` 0.13.1 → 1.1.0 internally (`lock_exclusive` → `lock`, `try_lock_exclusive` returns `Result<(), TryLockError>`). No public API change (d1098e8).
- `cargo update` across 26 transitive deps (d1098e8).

### Fixed
- Module-level doc comment containing `<project>/<bench>` was parsed as unclosed HTML tags by rustdoc under `-D warnings`; backticked the placeholder (1f96c5d).
- Windows `LockFileEx` mandatory locking made the lock file unreadable to peekers; split into a fs4-locked file and a sibling `.info` file replaced via atomic temp + rename. Verified across all five test platforms (4e37d8f).

## [0.1.7] - 2026-04-12

### Changed
- `zenbench-driver` binary replaced by self-trampolining: the bench binary re-execs itself for multi-process aggregation, eliminating the separate driver dependency (5035c6f).
- Renamed `run_processes` → `run_passes` to reflect that the function actually controls in-process pass count, not OS process count (007e9f7).
- Dropped never-published deprecated aliases left over from internal refactors (3182780).

### Documentation
- Added multi-process / multi-pass section to README; updated framework comparison table (4f3ea7b).

[Unreleased]: https://github.com/imazen/zenbench/compare/v0.1.10...HEAD
[0.1.10]: https://github.com/imazen/zenbench/compare/v0.1.9...v0.1.10
[0.1.9]: https://github.com/imazen/zenbench/compare/v0.1.8...v0.1.9
[0.1.8]: https://github.com/imazen/zenbench/compare/v0.1.7...v0.1.8
[0.1.7]: https://github.com/imazen/zenbench/compare/v0.1.6...v0.1.7
