# Changelog

## [Unreleased]

### QUEUED BREAKING CHANGES
<!-- Breaking changes that will ship together in the next minor release.
     Add items here as you discover them. Do NOT ship these piecemeal — batch them. -->

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

[Unreleased]: https://github.com/imazen/zenbench/compare/v0.1.8...HEAD
[0.1.8]: https://github.com/imazen/zenbench/compare/v0.1.7...v0.1.8
[0.1.7]: https://github.com/imazen/zenbench/compare/v0.1.6...v0.1.7
