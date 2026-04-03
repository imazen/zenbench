//! Benchmark WASM simd128 vs scalar — array sum via wasmtime.
//!
//! Run:  cargo run --example wasm_simd_bench --features wasm
//!
//! This demonstrates:
//! - Loading a WAT module with v128 SIMD operations
//! - Scalar vs simd128 comparison on the same workload
//! - Parallel multi-instance benchmarking (each thread gets its own WASM instance)
//!
//! For benchmarking your own Rust code compiled to WASM:
//!
//! ```sh
//! # Compile with simd128 enabled
//! RUSTFLAGS="-C target-feature=+simd128" \
//!     cargo build --target wasm32-unknown-unknown --release -p my_lib
//!
//! # Then load the .wasm file:
//! let wasm = WasmBench::from_file("target/wasm32-unknown-unknown/release/my_lib.wasm")?;
//! ```
//!
//! For `wasm32-wasip1` modules that need WASI imports, use `wasm.wasi_instance()`.

use zenbench::prelude::*;
use zenbench::wasm::WasmBench;

/// WAT module with scalar and simd128 array-sum implementations.
///
/// Exports:
/// - `init(len: i32)` — fill memory with 0, 1, 2, ... len-1
/// - `scalar_sum(len: i32) -> i32` — sum via scalar i32 loop
/// - `simd_sum(len: i32) -> i32` — sum via v128 i32x4 with scalar tail
const SIMD_WAT: &str = r#"(module
  (memory (export "memory") 1)  ;; 1 page = 64 KiB = 16384 i32s

  ;; Fill memory[0..len*4] with values 0, 1, 2, ... len-1
  (func (export "init") (param $len i32)
    (local $i i32)
    (block $break
      (loop $loop
        (br_if $break (i32.ge_u (local.get $i) (local.get $len)))
        (i32.store
          (i32.shl (local.get $i) (i32.const 2))
          (local.get $i))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $loop))))

  ;; Scalar sum: one i32 load per iteration
  (func (export "scalar_sum") (param $len i32) (result i32)
    (local $i i32)
    (local $sum i32)
    (block $break
      (loop $loop
        (br_if $break (i32.ge_u (local.get $i) (local.get $len)))
        (local.set $sum (i32.add (local.get $sum)
          (i32.load (i32.shl (local.get $i) (i32.const 2)))))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $loop)))
    (local.get $sum))

  ;; SIMD sum: four i32s per iteration via v128
  (func (export "simd_sum") (param $len i32) (result i32)
    (local $i i32)
    (local $vec v128)
    (local $tail i32)
    (local $vec_len i32)
    ;; vec_len = len & ~3  (round down to multiple of 4)
    (local.set $vec_len (i32.and (local.get $len) (i32.const -4)))
    ;; Vector loop
    (block $break
      (loop $loop
        (br_if $break (i32.ge_u (local.get $i) (local.get $vec_len)))
        (local.set $vec (i32x4.add (local.get $vec)
          (v128.load (i32.shl (local.get $i) (i32.const 2)))))
        (local.set $i (i32.add (local.get $i) (i32.const 4)))
        (br $loop)))
    ;; Horizontal reduction: sum 4 lanes
    (local.set $tail (i32.add
      (i32.add (i32x4.extract_lane 0 (local.get $vec))
               (i32x4.extract_lane 1 (local.get $vec)))
      (i32.add (i32x4.extract_lane 2 (local.get $vec))
               (i32x4.extract_lane 3 (local.get $vec)))))
    ;; Scalar tail for remaining 0-3 elements
    (block $break2
      (loop $loop2
        (br_if $break2 (i32.ge_u (local.get $i) (local.get $len)))
        (local.set $tail (i32.add (local.get $tail)
          (i32.load (i32.shl (local.get $i) (i32.const 2)))))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $loop2)))
    (local.get $tail))
)"#;

const LEN: i32 = 4096;

fn bench_wasm_simd(suite: &mut Suite) {
    let wasm = WasmBench::from_wat(SIMD_WAT).expect("failed to compile WAT module");

    // Verify correctness: both should produce the same sum
    {
        let mut inst = wasm.instance().expect("failed to create instance");
        inst.call::<(i32,), ()>("init", (LEN,)).unwrap();
        let scalar: i32 = inst.call::<(i32,), i32>("scalar_sum", (LEN,)).unwrap();
        let simd: i32 = inst.call::<(i32,), i32>("simd_sum", (LEN,)).unwrap();
        assert_eq!(scalar, simd, "scalar and simd sums must match");
        eprintln!("[wasm_simd_bench] correctness check passed: sum({LEN}) = {scalar}");
    }

    // --- Scalar vs SIMD comparison ---
    suite.group("wasm_array_sum", |g| {
        g.throughput(Throughput::Elements(LEN as u64));

        g.bench("scalar", {
            let wasm = wasm.clone();
            move |b| {
                let mut inst = wasm.instance().unwrap();
                inst.call::<(i32,), ()>("init", (LEN,)).unwrap();
                let f = inst.typed_func::<(i32,), i32>("scalar_sum").unwrap();
                b.iter(|| f.call(&mut inst.store, (LEN,)).unwrap())
            }
        });

        g.bench("simd128", {
            let wasm = wasm.clone();
            move |b| {
                let mut inst = wasm.instance().unwrap();
                inst.call::<(i32,), ()>("init", (LEN,)).unwrap();
                let f = inst.typed_func::<(i32,), i32>("simd_sum").unwrap();
                b.iter(|| f.call(&mut inst.store, (LEN,)).unwrap())
            }
        });
    });

    // --- Parallel scaling: each thread gets its own WASM instance ---
    suite.group("wasm_simd_parallel", |g| {
        g.throughput(Throughput::Elements(LEN as u64));

        for threads in [1, 2, 4] {
            g.bench_parallel(format!("simd128_{threads}t"), threads, {
                let wasm = wasm.clone();
                move |b, _tid| {
                    let mut inst = wasm.instance().unwrap();
                    inst.call::<(i32,), ()>("init", (LEN,)).unwrap();
                    let f = inst.typed_func::<(i32,), i32>("simd_sum").unwrap();
                    b.iter(|| f.call(&mut inst.store, (LEN,)).unwrap())
                }
            });
        }
    });
}

zenbench::main!(bench_wasm_simd);
