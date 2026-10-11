# Native Windows repeatability: windows_repeatability_tsc_independent

Observed 2026-10-11. Source commit `b40ac4a389313308b0ae88db6888c4dec9a5be23`.
Executable SHA-256: `d480e07ca37a4c406c4ccb998ddae15e413187f46da9b2c7a5431a8dd48b5a89`.

Full rounds, execution orders, native RSS, and benchmark stderr are in
`/mnt/v/output/zenbench/windows-verification/2026-10-11/windows_repeatability_tsc_independent_9e5f1e5de8b8ef1a04ab62f1412fc78ab847a1deb681c3c03ae287d17e430fa9.json.gz` on `dev`, with a verified copy at
`/mnt/tower/output/zenbench/windows-verification/2026-10-11/windows_repeatability_tsc_independent_9e5f1e5de8b8ef1a04ab62f1412fc78ab847a1deb681c3c03ae287d17e430fa9.json.gz` on the same host. Both paths were checked during archival.
Compressed bytes: 563118. SHA-256: `9e5f1e5de8b8ef1a04ab62f1412fc78ab847a1deb681c3c03ae287d17e430fa9`.

Build: `just windows-build-example windows_repeatability` (add
`--no-default-features` for the QPC-only executable). Run:
`just windows-repeatability-remote <trusted-windows-alias> <CPU-sets> <repeats>`.
Exact CPU sets/repetitions and timing mode are in the packet. The native
runner uses a 2 GiB job memory cap and idle priority. Resource admission
is disabled explicitly; these observations do not certify quietness or
maximum frequency.

Source hashes at the recorded commit:

- `examples/windows_repeatability.rs`: `53431225f9f69a8e2e22208ac9b8552a2b4b51edd0d5d0a4ab694b71e69d5a67`
- `examples/windows_repeatability.ps1`: `2f7babd7abf65ac5e8360269f830f38a80a13ef9478a66170e05e5ba3d7fbde8`
- `src/engine.rs`: `41a32d6a16852d24170bd33e7dedd01ae69eb265d3a12935013c655764c8436e`
- `src/stats.rs`: `45475ab42ba5a42df124dc7a0f0cf7fbee3e0f871088d658c46c666a9d59282d`
- `Cargo.lock`: `6620a21520dc067395a203269dfab07f40e0c70492943bb20d409f5e0fad1b1f`
