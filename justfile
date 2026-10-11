# zenbench development recipes

wasm_target := "wasm32-wasip1"
wasm_simd_flags := "-C target-feature=+simd128"
zen := env("HOME") / "work/zen"

# ── Standard development ─────────────────────────────────────

# Run all CI checks (fmt, clippy, test, wasm)
ci: fmt clippy test wasm-check

# Check default + wasm features
check:
    cargo check
    cargo check --features wasm

# Run clippy on all feature combos
clippy:
    cargo clippy --all-targets -- -D warnings
    cargo clippy --all-targets --features wasm -- -D warnings
    cargo clippy --all-targets --features criterion-compat -- -D warnings

# Check formatting + regenerate the public-API surface snapshots (docs/public-api/).
# The snapshot runner lives in the standalone apidoc/ package, so it is never
# built or run by plain `cargo test` or any CI job.
fmt:
    cargo fmt -- --check
    cargo test --manifest-path apidoc/Cargo.toml

# Regenerate the public-API surface snapshots only
api-doc:
    cargo test --manifest-path apidoc/Cargo.toml

# Verify the committed snapshots are current
api-doc-check:
    ZEN_API_DOC=check cargo test --manifest-path apidoc/Cargo.toml

# Fix formatting
fmt-fix:
    cargo fmt

# Run library tests
test:
    ZENBENCH_CPU_POLICY=unrestricted cargo test --lib

# Run all tests including integration
test-all:
    ZENBENCH_CPU_POLICY=unrestricted cargo test
    ZENBENCH_CPU_POLICY=unrestricted cargo test --features criterion-compat

# ── Native benchmarks ────────────────────────────────────────

# Run the sorting demo benchmark
bench:
    cargo bench --bench sorting

# Run all benchmarks
bench-all:
    cargo bench

# Run a specific benchmark group
bench-group group:
    cargo bench --bench sorting -- --group={{group}}

# ── WASM benchmarking ────────────────────────────────────────

# Verify wasm feature compiles (lib + example)
wasm-check:
    cargo check --features wasm
    cargo check --example wasm_simd_bench --features wasm

# Run the built-in WAT scalar-vs-simd128 benchmark
wasm-bench:
    ZENBENCH_NO_SAVE=1 cargo run --release --example wasm_simd_bench --features wasm

# Verify a zen crate compiles for wasm32-wasip1 with simd128
# Usage: just wasm-check-crate zenflate
wasm-check-crate crate:
    RUSTFLAGS="{{wasm_simd_flags}}" cargo check \
        --manifest-path {{zen}}/{{crate}}/Cargo.toml \
        --target {{wasm_target}} --lib

# Verify a zen crate compiles for wasm32-unknown-unknown (no_std) with simd128
# Usage: just wasm-check-bare linear-srgb
wasm-check-bare crate:
    RUSTFLAGS="{{wasm_simd_flags}}" cargo check \
        --manifest-path {{zen}}/{{crate}}/Cargo.toml \
        --target wasm32-unknown-unknown --lib --no-default-features

# Run tests for a zen crate under wasmtime with simd128
# Pass extra cargo args after crate name (e.g., feature overrides).
# Usage: just wasm-test linear-srgb
# Usage: just wasm-test zenflate --no-default-features --features std
wasm-test crate *args:
    RUSTFLAGS="{{wasm_simd_flags}}" \
    CARGO_TARGET_WASM32_WASIP1_RUNNER="wasmtime" \
        cargo test \
        --manifest-path {{zen}}/{{crate}}/Cargo.toml \
        --target {{wasm_target}} --lib {{args}}

# Run tests for a zen crate under wasmtime WITHOUT simd128 (scalar fallback)
# Usage: just wasm-test-scalar linear-srgb
wasm-test-scalar crate *args:
    CARGO_TARGET_WASM32_WASIP1_RUNNER="wasmtime" \
        cargo test \
        --manifest-path {{zen}}/{{crate}}/Cargo.toml \
        --target {{wasm_target}} --lib {{args}}

# Run simd128 + scalar tests for a zen crate (both paths)
# Usage: just wasm-test-both linear-srgb
wasm-test-both crate:
    @echo "── simd128 ──"
    just wasm-test {{crate}}
    @echo "── scalar ──"
    just wasm-test-scalar {{crate}}

# Verify all WASM-ready zen crates compile for wasm32-wasip1 with simd128
wasm-check-all:
    @echo "Checking zen crates for {{wasm_target}} + simd128..."
    just wasm-check-crate zenflate
    just wasm-check-crate linear-srgb
    just wasm-check-crate zenresize

# Run WASM tests for all verified crates
# zenflate: --no-default-features --features std excludes threads feature
# (std::thread::scope unavailable in wasm32-wasip1 without shared memory)
wasm-test-all:
    just wasm-test-both linear-srgb
    just wasm-test zenflate --no-default-features --features std

# SPEEDQ external-worker measurements reuse the engine's statistics owner.
paired-rounds-build:
    cargo build --release --example paired_rounds --no-default-features

paired-rounds-check:
    ZENBENCH_CPU_POLICY=unrestricted cargo test --lib --no-default-features
    cargo test --example paired_rounds --no-default-features
    cargo clippy --lib --example paired_rounds --no-default-features -- -D warnings

# Scoped-affinity verification (invoke through run-heavy on shared hosts).
affinity-check:
    cargo fmt -- --check
    cargo clippy --all-targets -- -D warnings
    cargo clippy --lib --tests --examples --no-default-features -- -D warnings
    ZENBENCH_CPU_POLICY=unrestricted cargo test

# Print the calling thread's OS mask before, during, and after a tiny benchmark.
affinity-example:
    ZENBENCH_CPU_POLICY=unrestricted ZENBENCH_NO_SAVE=1 ZENBENCH_NO_CALIBRATE=1 cargo run --example pinned

# Type-check native backends and the unsupported-target fallback, without linking.
affinity-platform-check:
    cargo check --lib --tests --no-default-features --target x86_64-apple-darwin
    cargo check --lib --tests --no-default-features --target aarch64-apple-darwin
    cargo check --lib --features criterion-compat --target aarch64-apple-darwin
    cargo check --lib --tests --no-default-features --target x86_64-pc-windows-gnu
    cargo check --lib --no-default-features --target wasm32-unknown-unknown

# Update API snapshots and verify platform test compilation plus the example.
affinity-review-check: affinity-platform-check api-doc affinity-example

# Exercise native and explicitly wrapped launches without changing assertions.
affinity-subprocess-check:
    cargo fmt -- --check
    cargo clippy --test affinity_env -- -D warnings
    ZENBENCH_CPU_POLICY=unrestricted cargo test --test affinity_env
    ZENBENCH_CPU_POLICY=unrestricted ZENBENCH_AFFINITY_TEST_RUNNER=/usr/bin/env cargo test --test affinity_env

# Cross.toml supplies qemu-arm to children on this emulated target.
affinity-arm-check:
    ZENBENCH_CPU_POLICY=unrestricted cross test --target armv7-unknown-linux-gnueabihf --all-features --test affinity_env
    ZENBENCH_CPU_POLICY=unrestricted cross test --target armv7-unknown-linux-gnueabihf --no-default-features --test affinity_env

# Functional fixtures opt out explicitly; policy cases clear that opt-out in fresh children.
cpu-policy-check:
    cargo fmt --check
    cargo clippy --all-targets --features criterion-compat -- -D warnings
    ZENBENCH_CPU_POLICY=unrestricted ZENBENCH_NO_SAVE=1 ZENBENCH_NO_CALIBRATE=1 cargo test --features criterion-compat
    ZENBENCH_CPU_POLICY=unrestricted ZENBENCH_NO_SAVE=1 ZENBENCH_NO_CALIBRATE=1 cargo test --no-default-features

cpu-policy-build:
    cargo build --bin zenbench --example cpu_policy

# Each class needs its own fresh process. These probes use one CPU, no sweep.
cpu-policy-container image cpus:
    docker run --rm --memory=12g --memory-swap=12g --cpus=1 --cpuset-cpus={{cpus}} --mount type=bind,src={{justfile_directory()}},dst=/work,readonly --env ZENBENCH_NO_SAVE=1 --env ZENBENCH_NO_CALIBRATE=1 --entrypoint /usr/bin/nice {{image}} -n19 /work/target/debug/examples/cpu_policy

# This gate test requires no foreign benchmark processes on the host.
cpu-policy-gate-check:
    ZENBENCH_CPU_POLICY=unrestricted cargo test --lib gate::tests::own_benchmark_named_thread_does_not_block_gate -- --exact --nocapture

cpu-policy-review-check:
    cargo clippy --all-targets --features criterion-compat -- -D warnings
    cargo test --lib platform::placement
    cargo test --lib results::tests::placement_metadata
    ZENBENCH_CPU_POLICY=unrestricted ZENBENCH_NO_CALIBRATE=1 ZENBENCH_NO_SAVE=1 cargo test --test cpu_policy --test affinity --test affinity_env --test criterion_compat --features criterion-compat
    just affinity-platform-check
    just api-doc
    just cpu-policy-build

cpu-policy-image:
    docker pull ubuntu:26.04

cpu-policy-launch-container image cpus:
    docker run --rm --memory=12g --memory-swap=12g --cpus=1 --cpuset-cpus={{cpus}} --mount type=bind,src={{justfile_directory()}},dst=/work,readonly --entrypoint /usr/bin/nice {{image}} -n19 /work/target/debug/zenbench exec --cpu-policy auto --cpu-set={{cpus}} /usr/bin/printf -- PROGRAM_STARTED

cpu-policy-cli-check:
    cargo clippy --bin zenbench -- -D warnings
    cargo build --bin zenbench
    cargo test --test cpu_policy launcher_helper_cannot_certify_an_opt_out -- --exact

cpu-policy-chart-check:
    cargo clippy --all-targets --features criterion-compat,charts -- -D warnings
    cargo test --lib --features charts charts::tests

cpu-policy-subprocess-check:
    cargo clippy --test cpu_policy -- -D warnings
    ZENBENCH_AFFINITY_TEST_RUNNER=/usr/bin/env ZENBENCH_NO_SAVE=1 ZENBENCH_NO_CALIBRATE=1 cargo test --test cpu_policy

cpu-policy-arm-check:
    ZENBENCH_CPU_POLICY=unrestricted cross test --target armv7-unknown-linux-gnueabihf --no-default-features --features cli --test cpu_policy

# Fixed-work diagnostics: an actual Rayon pool and new CLI diagnostic workers.
cpu-probe-check:
    cargo fmt --check
    cargo clippy --all-targets --features criterion-compat -- -D warnings
    cargo clippy --lib --tests --examples --no-default-features -- -D warnings
    cargo test --lib platform::probe
    cargo test --test cpu_probe
    cargo test --test cpu_probe --no-default-features
    ZENBENCH_AFFINITY_TEST_RUNNER=/usr/bin/env cargo test --test cpu_probe
    cargo build --bin zenbench --example cpu_probe

cpu-probe-example *cpus:
    cargo run --release --example cpu_probe -- {{cpus}}

cpu-probe-info:
    cargo run --bin zenbench -- cpu-info --probe --workers 2

# Native API oracle through an existing trusted SSH alias; no remote file writes.
mac-affinity-probe host:
    ssh -o BatchMode=yes -o StrictHostKeyChecking=yes -o ConnectTimeout=10 {{host}} 'python3 -B -' < examples/macos_affinity_probe.py

# Invoke through run-heavy locally; native interpreter is niced, CPU-time-limited, and RSS-monitored.
mac-qos-probe host:
    ssh -o BatchMode=yes -o StrictHostKeyChecking=yes -o ConnectTimeout=10 {{host}} 'nice -n19 /usr/bin/time -l python3 -B -' < examples/macos_qos_probe.py

# Verify QoS guard compile-fail documentation and normal Criterion startup.
qos-warmup-check:
    cargo test --lib platform::affinity
    ZENBENCH_CPU_POLICY=unrestricted ZENBENCH_NO_SAVE=1 ZENBENCH_NO_CALIBRATE=1 cargo test --doc
    ZENBENCH_CPU_POLICY=unrestricted ZENBENCH_NO_SAVE=1 ZENBENCH_NO_CALIBRATE=1 cargo test --features criterion-compat --test criterion_compat

# Compile the diagnostic's native declarations without invoking Windows APIs.
windows-topology-compile image="mcr.microsoft.com/powershell:latest":
    docker run --rm --memory=2g --memory-swap=2g --cpus=1 --mount type=bind,src={{justfile_directory()}},dst=/work,readonly --entrypoint /usr/bin/nice {{image}} -n19 pwsh -NoProfile -File /work/examples/windows_topology_probe.ps1 -CompileOnly
