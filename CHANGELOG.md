# Changelog

## [Unreleased]

### Added
- Export `ResourceGate` and `GateReason` at the crate root alongside `GateConfig`, so external harnesses (first consumer: zensysbench) can run the busyness gate around their own child-process measurements. The methods were already annotated "Public API for external gate users"; this makes them reachable. Additive, no behavior change.

### QUEUED BREAKING CHANGES
<!-- Breaking changes that will ship together in the next minor release.
     Add items here as you discover them. Do NOT ship these piecemeal — batch them. -->
- `wasm` feature: the wasmtime/wasmtime-wasi 43 → 44 bump changes the re-exported `wasmtime` major (`pub use wasmtime` at `zenbench::wasm::wasmtime`) and raises the `wasm`-feature build MSRV to Rust 1.92. The crate's default-build MSRV is unchanged (1.85), since wasmtime is only pulled in by the optional `wasm` feature (e521d9d).

### Security
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

[Unreleased]: https://github.com/imazen/zenbench/compare/v0.1.9...HEAD
[0.1.9]: https://github.com/imazen/zenbench/compare/v0.1.8...v0.1.9
[0.1.8]: https://github.com/imazen/zenbench/compare/v0.1.7...v0.1.8
[0.1.7]: https://github.com/imazen/zenbench/compare/v0.1.6...v0.1.7
