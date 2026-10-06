//! Smoke test for the `wasm` feature: compile WAT, instantiate with and
//! without WASI imports, and call exports through the wasmtime re-export.
//!
//! Nothing else exercises `zenbench::wasm` at runtime; the example only
//! compiles in CI. These tests catch a wasmtime / wasmtime-wasi bump that
//! still builds but breaks instantiation, WASI p1 linking, or simd128.
//! On hosts without a Cranelift backend (i686, armv7) wasmtime falls back to
//! the Pulley interpreter, so the same assertions run there too.

#![cfg(feature = "wasm")]

use zenbench::wasm::WasmBench;

const ADD_SIMD_WAT: &str = r#"(module
  (func (export "add") (param i32 i32) (result i32)
    local.get 0 local.get 1 i32.add)

  ;; Splat both operands, add as i32x4, sum all four lanes.
  (func (export "simd_add4") (param i32 i32) (result i32)
    (local $v v128)
    (local.set $v (i32x4.add (i32x4.splat (local.get 0))
                             (i32x4.splat (local.get 1))))
    (i32.add
      (i32.add (i32x4.extract_lane 0 (local.get $v))
               (i32x4.extract_lane 1 (local.get $v)))
      (i32.add (i32x4.extract_lane 2 (local.get $v))
               (i32x4.extract_lane 3 (local.get $v))))))"#;

/// Imports one WASI p1 function; WASI reads guest memory via the
/// `memory` export, so the module must export it.
const WASI_WAT: &str = r#"(module
  (import "wasi_snapshot_preview1" "random_get"
    (func $random_get (param i32 i32) (result i32)))
  (memory (export "memory") 1)
  ;; Returns the WASI errno (0 = success) after filling 16 bytes.
  (func (export "fill") (result i32)
    (call $random_get (i32.const 0) (i32.const 16))))"#;

#[test]
fn plain_instance_calls_scalar_and_simd_exports() {
    let wasm = WasmBench::from_wat(ADD_SIMD_WAT).expect("compile WAT");
    let mut inst = wasm.instance().expect("instantiate");

    assert_eq!(inst.call::<(i32, i32), i32>("add", (2, 40)).unwrap(), 42);

    let f = inst.typed_func::<(i32, i32), i32>("simd_add4").unwrap();
    assert_eq!(f.call(&mut inst.store, (3, 4)).unwrap(), 28);
}

#[test]
fn clones_share_the_module_but_not_instances() {
    let wasm = WasmBench::from_wat(ADD_SIMD_WAT).expect("compile WAT");
    let other = wasm.clone();
    let mut a = wasm.instance().unwrap();
    let mut b = other.instance().unwrap();
    assert_eq!(a.call::<(i32, i32), i32>("add", (1, 1)).unwrap(), 2);
    assert_eq!(b.call::<(i32, i32), i32>("add", (5, 5)).unwrap(), 10);
    assert!(wasm.module().exports().any(|e| e.name() == "simd_add4"));
}

#[test]
fn wasi_instance_links_preview1_imports() {
    let wasm = WasmBench::from_wat(WASI_WAT).expect("compile WAT");
    let mut inst = wasm.wasi_instance().expect("instantiate with WASI");
    assert_eq!(inst.call::<(), i32>("fill", ()).unwrap(), 0, "WASI errno");
}

#[test]
fn plain_instance_rejects_unresolved_wasi_imports() {
    let wasm = WasmBench::from_wat(WASI_WAT).expect("compile WAT");
    assert!(wasm.instance().is_err());
}
