# Native Windows repeatability: windows_repeatability_job_tsc

Observed 2026-10-11. Source commit `95e4042c9e58bc53070b352d36a4a65f93293885`.
Executable SHA-256: `5149c66b25d1c8df3eb1634761de7be613e7a3574995adb95447bad5947d6977`.

Full rounds, execution orders, native RSS, and benchmark stderr are in
`/mnt/v/output/zenbench/windows-verification/2026-10-11/windows_repeatability_job_tsc_b3c69b82b7b978584cf1b699e41c8c3ea553e6a1bc446e764850193c322dc61b.json.gz` on `dev`, with a verified copy at
`/mnt/tower/output/zenbench/windows-verification/2026-10-11/windows_repeatability_job_tsc_b3c69b82b7b978584cf1b699e41c8c3ea553e6a1bc446e764850193c322dc61b.json.gz` on the same host. Both paths were checked during archival.
Compressed bytes: 564535. SHA-256: `b3c69b82b7b978584cf1b699e41c8c3ea553e6a1bc446e764850193c322dc61b`.

Build: `just windows-build-example windows_repeatability` (add
`--no-default-features` for the QPC-only executable). Run:
`just windows-repeatability-remote <trusted-windows-alias> <CPU-sets> <repeats>`.
Exact CPU sets/repetitions and timing mode are in the packet. The native
runner uses a 2 GiB job memory cap and idle priority. Resource admission
is disabled explicitly; these observations do not certify quietness or
maximum frequency.

Source hashes at the recorded commit:

- `examples/windows_repeatability.rs`: `21e28aa520910d694872a143d7b127ac091fe35ca435111e760def5895216874`
- `examples/windows_repeatability.ps1`: `6568bb4b3be2d49dbade81e45f8b93d2f30ae87125a9966293fc96783d8c8c05`
- `src/engine.rs`: `41a32d6a16852d24170bd33e7dedd01ae69eb265d3a12935013c655764c8436e`
- `src/stats.rs`: `45475ab42ba5a42df124dc7a0f0cf7fbee3e0f871088d658c46c666a9d59282d`
- `Cargo.lock`: `8633f119c25be010747553c45cd37c9cfa747cf2de3103c1e8fc4837fdb2acc3`
