# Native Windows repeatability: windows_repeatability_qpc_fixed_seed

Observed 2026-10-11. Source commit `fc75703794b1125ae507a9e42739924716938d80`.
Executable SHA-256: `4aeb6d7301bf5e96d6d0fce5cec8cf5fefdee4973b676abc92280fa702db3668`.

Full rounds, execution orders, native RSS, and benchmark stderr are in
`/mnt/v/output/zenbench/windows-verification/2026-10-11/windows_repeatability_qpc_fixed_seed_2c404a9db32fa9b6312bf147a20d43db2e7889d64ac8ceef7eadc5279d99381a.json.gz` on `dev`, with a verified copy at
`/mnt/tower/output/zenbench/windows-verification/2026-10-11/windows_repeatability_qpc_fixed_seed_2c404a9db32fa9b6312bf147a20d43db2e7889d64ac8ceef7eadc5279d99381a.json.gz` on the same host. Both paths were checked during archival.
Compressed bytes: 503006. SHA-256: `2c404a9db32fa9b6312bf147a20d43db2e7889d64ac8ceef7eadc5279d99381a`.

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
- `src/engine.rs`: `84570c5b9a5d41a5d62da0032319114ba4411789999d618696d512bc73293ceb`
- `src/stats.rs`: `45475ab42ba5a42df124dc7a0f0cf7fbee3e0f871088d658c46c666a9d59282d`
- `Cargo.lock`: `6620a21520dc067395a203269dfab07f40e0c70492943bb20d409f5e0fad1b1f`
