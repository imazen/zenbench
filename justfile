# zenbench development recipes

evidence-check:
    cargo fmt -p zenbench
    ZENBENCH_CPU_POLICY=unrestricted cargo test --lib storage::
    ZENBENCH_CPU_POLICY=unrestricted cargo test --test aggregation_evidence --test round_samples
    cargo clippy --all-targets -- -D warnings
    just api-doc

# Audit harness runtime/storage; invoke heavy recipes through run-heavy.
defaults-audit-build:
    cargo fmt -p zenbench -- --check
    cargo clippy --example defaults_audit -- -D warnings
    cargo build --release --example defaults_audit

defaults-audit-run output plan="full" cpus="0":
    docker run --rm --user $(id -u):$(id -g) --memory=2g --memory-swap=2g --cpuset-cpus={{cpus}} --mount type=bind,src={{justfile_directory()}},dst=/work --workdir /work --env HOME=/work/.git/defaults-audit-home --env TMPDIR=/work/.git/defaults-audit-home/tmp --entrypoint /usr/bin/nice zenbench-audit -n19 python3 -B examples/audit_defaults.py /work/target/release/examples/defaults_audit {{output}} --plan {{plan}}

defaults-audit-image:
    docker build --memory=2g --memory-swap=2g --cpu-shares=256 -t zenbench-audit -f tools/defaults-audit.Dockerfile tools

defaults-audit-summary directory output:
    python3 -B examples/summarize_defaults_audit.py {{directory}} {{output}}

defaults-audit-fixture:
    target/release/examples/defaults_audit --fixture

defaults-audit-archive manifest host date *directories:
    python3 -B examples/archive_defaults_audit.py {{manifest}} target/release/examples/defaults_audit {{directories}} --archive-host {{host}} --date {{date}}

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
windows-topology-compile image="mcr.microsoft.com/powershell:latest" script="windows_topology_probe.ps1":
    docker run --rm --memory=2g --memory-swap=2g --cpus=1 --mount type=bind,src={{justfile_directory()}},dst=/work,readonly --entrypoint /usr/bin/nice {{image}} -n19 pwsh -NoProfile -File /work/examples/{{script}} -CompileOnly

# Additional arguments select existing relays, network jumps, and trusted pins.
windows-topology-remote host *options:
    python3 -B examples/windows_remote_probe.py {{host}} {{options}}

# Local cross-linker container; no Windows compiler installation is required.
windows-build-image:
    docker build -t zenbench-windows-build -f tools/windows-build.Dockerfile tools

windows-cargo *options:
    docker run --rm --user $(id -u):$(id -g) --memory=12g --memory-swap=12g --cpus=8 --mount type=bind,src={{justfile_directory()}},dst={{justfile_directory()}} --mount type=bind,src={{env("HOME")}}/.cargo,dst={{env("HOME")}}/.cargo --mount type=bind,src={{env("HOME")}}/.rustup,dst={{env("HOME")}}/.rustup,readonly --mount type=bind,src={{env("HOME")}}/tmp,dst={{env("HOME")}}/tmp --env HOME={{env("HOME")}} --env TMPDIR={{env("HOME")}}/tmp --env CARGO_BUILD_JOBS=8 --env CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc-posix --workdir {{justfile_directory()}} --entrypoint /usr/bin/nice zenbench-windows-build -n19 {{env("HOME")}}/.cargo/bin/cargo {{options}}

windows-build-example example *options:
    just windows-cargo build --release --target x86_64-pc-windows-gnu --example {{example}} {{options}}

windows-repeatability-check:
    cargo fmt --check
    cargo clippy --all-targets --features criterion-compat -- -D warnings
    just windows-cargo clippy --target x86_64-pc-windows-gnu --example windows_repeatability -- -D warnings
    just windows-topology-compile mcr.microsoft.com/powershell:latest windows_repeatability.ps1

# Binary transfer is hash checked; each child inherits a native 2 GiB memory cap.
windows-repeatability-remote host sets="0;2" repeats="2" *options:
    python3 -B examples/windows_remote_probe.py {{host}} --script examples/windows_repeatability.ps1 --script-arg `sha256sum target/x86_64-pc-windows-gnu/release/examples/windows_repeatability.exe | cut -d' ' -f1` --script-arg '{{sets}}' --script-arg {{repeats}} --timeout 1200 {{options}}

windows-repeatability-upload host *options:
    python3 -B examples/windows_remote_probe.py {{host}} --upload-file target/x86_64-pc-windows-gnu/release/examples/windows_repeatability.exe {{options}}

windows-repeatability-analyze log output *options:
    python3 -B examples/analyze_windows_repeatability.py {{log}} {{output}} {{options}}

execution-order-check:
    cargo test --test cpu_policy fresh_processes_use_independent_execution_orders -- --exact --nocapture

windows-repeatability-archive log label commit *options:
    python3 -B examples/archive_windows_repeatability.py {{log}} {{label}} {{commit}} {{options}}

windows-job-test-build:
    just windows-cargo test --release --target x86_64-pc-windows-gnu --test windows_job_boundary --no-run

windows-job-test-remote host binary *options:
    python3 -B examples/windows_remote_probe.py {{host}} --upload-file {{binary}} {{options}}
    python3 -B examples/windows_remote_probe.py {{host}} --script examples/windows_repeatability.ps1 --script-arg `sha256sum {{binary}} | cut -d' ' -f1` --script-arg default --script-arg 1 --script-arg job-test {{options}}

windows-gate-test-build:
    just windows-cargo test --release --target x86_64-pc-windows-gnu --test gate_sees_load --no-run

windows-msrv-check:
    cargo +1.85 check --locked --target x86_64-pc-windows-gnu --all-targets --features async,cpu-time,criterion-compat

# Reusable CI audit; redirect job logs to a retained capture file.
ci-status run:
    gh run view {{run}} --json jobs --jq '.jobs[] | [.name,.status,.conclusion] | @tsv'

ci-job-log job:
    gh api repos/imazen/zenbench/actions/jobs/{{job}}/logs --allow-escape-sequences

windows-gate-test-remote host binary *options:
    python3 -B examples/windows_remote_probe.py {{host}} --upload-file {{binary}} {{options}}
    python3 -B examples/windows_remote_probe.py {{host}} --script examples/windows_repeatability.ps1 --script-arg `sha256sum {{binary}} | cut -d' ' -f1` --script-arg default --script-arg 1 --script-arg gate-test {{options}}

monitor-check:
    cargo fmt -p zenbench
    ZENBENCH_CPU_POLICY=unrestricted cargo test --lib monitor::
    ZENBENCH_CPU_POLICY=unrestricted cargo test --test round_samples
    cargo clippy --all-targets -- -D warnings
    just api-doc

plan-check:
    cargo fmt -p zenbench
    ZENBENCH_CPU_POLICY=unrestricted cargo test --lib
    ZENBENCH_CPU_POLICY=unrestricted cargo test --test aggregation_evidence --test round_samples
    cargo clippy --all-targets -- -D warnings
    just api-doc

ancestor-check:
    cargo fmt -p zenbench -- --check
    cargo test --lib gate::tests::collect_ancestors
    cargo clippy --all-targets --all-features -- -D warnings

protocol-cross-plan cross="cross":
    CROSS_CONTAINER_OPTS="--memory=6g --memory-swap=6g --cpus=2" ZENBENCH_CPU_POLICY=unrestricted {{cross}} run --target armv7-unknown-linux-gnueabihf --example protocol_fixture -- --dry-run

protocol-build:
    cargo fmt -p zenbench
    cargo clippy --all-targets -- -D warnings
    cargo build --release --example protocol_fixture

# Retains all local protocol attempts; output must be a new path under this repo.
protocol-check output cpus="0" *options:
    docker run --rm --user $(id -u):$(id -g) --memory=2g --memory-swap=2g --cpuset-cpus={{cpus}} --mount type=bind,src={{justfile_directory()}},dst=/work --workdir /work --env HOME=/work/.git/defaults-audit-home --env TMPDIR=/work/.git/defaults-audit-home/tmp --entrypoint /usr/bin/nice zenbench-audit -n19 python3 -B examples/check_protocol.py /work/target/release/examples/protocol_fixture {{output}} {{options}}

# Generate independent expectations only into a previously absent fixture.
coverage-oracle:
    python3 -B examples/coverage_oracle.py

# Full local verification for mode/storage changes, with explicit placement opt-out
# for functional fixtures. Native placement subprocess controls test admission.
modes-full-check:
    cargo fmt -p zenbench
    ZENBENCH_CPU_POLICY=unrestricted ZENBENCH_NO_SAVE=1 cargo test --all-features
    ZENBENCH_CPU_POLICY=unrestricted ZENBENCH_NO_SAVE=1 cargo test --no-default-features
    cargo clippy --all-targets --all-features -- -D warnings
    cargo clippy --no-default-features -- -D warnings
    just api-doc
    cargo +1.85 check --locked --all-targets --features async,cpu-time,criterion-compat

protocol-replay directory:
    python3 -B examples/check_retained_protocol.py target/debug/zenbench {{directory}}

protocol-archive manifest binary host date *directories:
    python3 -B examples/archive_defaults_audit.py {{manifest}} {{binary}} {{directories}} --archive-host {{host}} --date {{date}} --label modes

cargo-bench-plan:
    ZENBENCH_CPU_POLICY=unrestricted cargo test --bench sorting -- --dry-run

cli-evidence-check:
    cargo fmt -p zenbench
    cargo test --test cli_evidence
    ZENBENCH_CPU_POLICY=unrestricted cargo test --lib baseline::
    cargo clippy --all-targets -- -D warnings

# Honor cross's explicit QEMU subprocess runner; keep CLI assertions identical.
cli-evidence-cross cross="cross":
    CROSS_CONTAINER_OPTS="--memory=6g --memory-swap=6g --cpus=2" ZENBENCH_CPU_POLICY=unrestricted {{cross}} test --target armv7-unknown-linux-gnueabihf --all-features --test cli_evidence

# Validate each review-stack boundary; run through run-heavy.
review-check *test_args="--lib":
    cargo fmt -p zenbench -- --check
    ZENBENCH_CPU_POLICY=unrestricted ZENBENCH_NO_SAVE=1 cargo test --all-features {{test_args}}
    cargo clippy --all-targets --all-features -- -D warnings
    just api-doc

# Both main! forms must display help before touching invalid plan/CPU settings.
workflow-check:
    cargo fmt -p zenbench -- --check
    cargo test --test cli_evidence
    cargo test --test cpu_policy startup_guides_precede_refusal_once_and_stay_out_of_child_output
    ZENBENCH_MODE=invalid ZENBENCH_CPU_POLICY=invalid cargo test --bench sorting -- --help
    ZENBENCH_MODE=invalid ZENBENCH_CPU_POLICY=invalid cargo test --bench standalone_only -- -h
    cargo clippy --all-targets --all-features -- -D warnings
    just api-doc-check

# Exercise the documented cargo-show-asm target/profile syntax on a real bench.
simd-asm-check:
    cargo asm --bench standalone_only --profile bench --asm 'standalone_only::main' 0

# Inspect retained executable bytes without rebuilding them (tool needs disasm).
simd-saved-asm binary symbol index="0":
    cargo asm --file '{{binary}}' --disasm '{{symbol}}' {{index}}

# Cooperative scheduling and external-process runner checks (use run-heavy).
process-check:
    cargo fmt -p zenbench
    cargo test --lib coordination::
    cargo test --bin zenbench
    cargo test --test cooperative_cli --test pair_cli
    cargo clippy --all-targets -- -D warnings

# Real Linux cgroup/priority and nested-reservation smoke check.
nice-smoke:
    cargo build --bin zenbench
    mkdir -p target/nice-smoke-admission
    ZENBENCH_COORDINATION_DIR="{{justfile_directory()}}/target/nice-smoke-admission" target/debug/zenbench nice --memory-mib 128 -- target/debug/zenbench nice --memory-mib 64 -- target/debug/zenbench guide

# Check the actual Cargo-installed binary, not just cargo run's build output.
install-check:
    cargo install --path . --root target/install-check --bin zenbench --locked
    target/install-check/bin/zenbench --help
    target/install-check/bin/zenbench guide --topic pair
    target/install-check/bin/zenbench nice --help

pair-cross cross="cross":
    CROSS_CONTAINER_OPTS="--memory=6g --memory-swap=6g --cpus=2" ZENBENCH_CPU_POLICY=unrestricted {{cross}} test --target armv7-unknown-linux-gnueabihf --all-features --test pair_cli

fleet-check:
    cargo fmt -p zenbench --check
    cargo test --bin zenbench support::fleet
    cargo test --test fleet_cli
    cargo clippy --bin zenbench --test fleet_cli -- -D warnings

fleet-memory-check output:
    cargo build --bin zenbench --example fleet_memory_fixture
    python3 -B examples/check_fleet_memory.py {{output}}

heap-check:
    cargo fmt -p zenbench --check
    cargo test --features heap-profiling --lib heap::
    cargo run --features heap-profiling --example heap_phases -- --exercise-oom
    cargo test --features heap-profiling --doc
    cargo clippy --lib --example heap_phases --features heap-profiling -- -D warnings

heap-oracle output:
    cargo build --example heap_phases --features heap-profiling
    python3 -B examples/check_heap_oracle.py {{output}}

fleet-cross cross="cross":
    CROSS_CONTAINER_OPTS="--memory=6g --memory-swap=6g --cpus=2" ZENBENCH_CPU_POLICY=unrestricted {{cross}} test --target armv7-unknown-linux-gnueabihf --test fleet_cli

# Run on Windows from a revision-identified source tree; keeps a transcript.
windows-native-check:
    powershell.exe -NoProfile -ExecutionPolicy Bypass -File examples/windows_native_check.ps1
