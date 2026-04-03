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

# Check formatting
fmt:
    cargo fmt -- --check

# Fix formatting
fmt-fix:
    cargo fmt

# Run library tests
test:
    cargo test --lib

# Run all tests including integration
test-all:
    cargo test
    cargo test --features criterion-compat

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
# Usage: just wasm-test linear-srgb
wasm-test crate:
    RUSTFLAGS="{{wasm_simd_flags}}" \
    CARGO_TARGET_WASM32_WASIP1_RUNNER="wasmtime" \
        cargo test \
        --manifest-path {{zen}}/{{crate}}/Cargo.toml \
        --target {{wasm_target}} --lib

# Run tests for a zen crate under wasmtime WITHOUT simd128 (scalar fallback)
# Usage: just wasm-test-scalar linear-srgb
wasm-test-scalar crate:
    CARGO_TARGET_WASM32_WASIP1_RUNNER="wasmtime" \
        cargo test \
        --manifest-path {{zen}}/{{crate}}/Cargo.toml \
        --target {{wasm_target}} --lib

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
# (zenflate excluded — dev-deps don't compile for wasm32-wasip1)
wasm-test-all:
    just wasm-test-both linear-srgb
