//! WASM benchmarking via wasmtime with simd128 support.
//!
//! Load a `.wasm` or `.wat` module, create per-thread instances, and
//! benchmark exported functions — including v128 SIMD operations.
//!
//! # Quick start
//!
//! ```rust,ignore
//! use zenbench::prelude::*;
//! use zenbench::wasm::WasmBench;
//!
//! let wasm = WasmBench::from_wat(r#"(module
//!     (func (export "add") (param i32 i32) (result i32)
//!         local.get 0 local.get 1 i32.add)
//! )"#).unwrap();
//!
//! zenbench::run(|suite| {
//!     suite.group("wasm", |g| {
//!         g.bench("add", {
//!             let wasm = wasm.clone();
//!             move |b| {
//!                 let mut inst = wasm.instance().unwrap();
//!                 let f = inst.typed_func::<(i32, i32), i32>("add").unwrap();
//!                 b.iter(|| f.call(&mut inst.store, (1, 2)).unwrap())
//!             }
//!         });
//!     });
//! });
//! ```
//!
//! # Parallel benchmarking
//!
//! [`WasmBench`] is `Clone` (Arc-shared). Each thread creates its own
//! [`WasmInstance`] with independent linear memory — no locks needed.
//!
//! ```rust,ignore
//! g.bench_parallel("add_4t", 4, {
//!     let wasm = wasm.clone();
//!     move |b, _tid| {
//!         let mut inst = wasm.instance().unwrap();
//!         let f = inst.typed_func::<(i32, i32), i32>("add").unwrap();
//!         b.iter(|| f.call(&mut inst.store, (1, 2)).unwrap())
//!     }
//! });
//! ```
//!
//! # WASI modules (Rust compiled to `wasm32-wasip1`)
//!
//! Use [`WasmBench::wasi_instance`] for modules that import WASI functions:
//!
//! ```rust,ignore
//! let wasm = WasmBench::from_file("target/wasm32-wasip1/release/my_lib.wasm").unwrap();
//! let mut inst = wasm.wasi_instance().unwrap();
//! ```

use std::path::Path;
use std::sync::Arc;

/// Re-export wasmtime for advanced use (custom Engine config, Linker, etc.).
pub use wasmtime;

/// Compiled WASM module ready for benchmarking.
///
/// Cheap to clone (Arc-shared). Thread-safe (`Send + Sync`).
/// Each benchmark thread creates its own [`WasmInstance`] via
/// [`instance()`](WasmBench::instance) or
/// [`wasi_instance()`](WasmBench::wasi_instance).
#[derive(Clone)]
pub struct WasmBench {
    inner: Arc<Inner>,
}

struct Inner {
    engine: wasmtime::Engine,
    module: wasmtime::Module,
}

/// Per-thread WASM execution context.
///
/// Contains a wasmtime [`Store`](wasmtime::Store) and
/// [`Instance`](wasmtime::Instance). Create one per benchmark thread.
///
/// For hot-path benchmarking, use [`typed_func`](WasmInstance::typed_func)
/// once, then call via the returned [`TypedFunc`](wasmtime::TypedFunc)
/// with `&mut inst.store`:
///
/// ```rust,ignore
/// let f = inst.typed_func::<(i32,), i32>("work").unwrap();
/// b.iter(|| f.call(&mut inst.store, (1024,)).unwrap());
/// ```
pub struct WasmInstance {
    /// The wasmtime Store. Pass `&mut store` to [`TypedFunc::call`](wasmtime::TypedFunc::call).
    pub store: wasmtime::Store<wasmtime_wasi::p1::WasiP1Ctx>,
    instance: wasmtime::Instance,
}

impl WasmBench {
    /// Create from WAT (WebAssembly Text) source.
    ///
    /// ```rust,ignore
    /// let wasm = WasmBench::from_wat(r#"(module
    ///     (func (export "add") (param i32 i32) (result i32)
    ///         local.get 0 local.get 1 i32.add)
    /// )"#).unwrap();
    /// ```
    pub fn from_wat(wat: &str) -> Result<Self, wasmtime::Error> {
        Self::from_bytes(wat.as_bytes())
    }

    /// Create from a `.wasm` or `.wat` file.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, wasmtime::Error> {
        let engine = Self::make_engine()?;
        let module = wasmtime::Module::from_file(&engine, path.as_ref())?;
        Ok(Self {
            inner: Arc::new(Inner { engine, module }),
        })
    }

    /// Create from raw WASM bytes (binary or WAT text).
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, wasmtime::Error> {
        let engine = Self::make_engine()?;
        let module = wasmtime::Module::new(&engine, bytes)?;
        Ok(Self {
            inner: Arc::new(Inner { engine, module }),
        })
    }

    /// Create with a custom wasmtime [`Config`](wasmtime::Config).
    ///
    /// Use this to control optimization level, enable/disable features, etc.
    pub fn from_bytes_with_config(
        bytes: &[u8],
        config: wasmtime::Config,
    ) -> Result<Self, wasmtime::Error> {
        let engine = wasmtime::Engine::new(&config)?;
        let module = wasmtime::Module::new(&engine, bytes)?;
        Ok(Self {
            inner: Arc::new(Inner { engine, module }),
        })
    }

    fn make_engine() -> Result<wasmtime::Engine, wasmtime::Error> {
        let mut config = wasmtime::Config::new();
        config.wasm_simd(true);
        config.cranelift_opt_level(wasmtime::OptLevel::Speed);
        wasmtime::Engine::new(&config)
    }

    fn make_p1_ctx() -> wasmtime_wasi::p1::WasiP1Ctx {
        wasmtime_wasi::WasiCtxBuilder::new().build_p1()
    }

    /// Create a new instance without WASI imports.
    ///
    /// Use for WAT modules or `wasm32-unknown-unknown` targets that don't
    /// import WASI functions.
    pub fn instance(&self) -> Result<WasmInstance, wasmtime::Error> {
        let state = Self::make_p1_ctx();
        let mut store = wasmtime::Store::new(&self.inner.engine, state);
        let instance = wasmtime::Instance::new(&mut store, &self.inner.module, &[])?;
        Ok(WasmInstance { store, instance })
    }

    /// Create a new instance with WASI preview1 imports linked.
    ///
    /// Use for `wasm32-wasip1` targets (Rust crates compiled to WASM).
    pub fn wasi_instance(&self) -> Result<WasmInstance, wasmtime::Error> {
        let state = Self::make_p1_ctx();
        let mut store = wasmtime::Store::new(&self.inner.engine, state);
        let mut linker = wasmtime::Linker::new(&self.inner.engine);
        wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |s| s)?;
        let instance = linker.instantiate(&mut store, &self.inner.module)?;
        Ok(WasmInstance { store, instance })
    }

    /// Access the underlying wasmtime [`Engine`](wasmtime::Engine).
    pub fn engine(&self) -> &wasmtime::Engine {
        &self.inner.engine
    }

    /// Access the underlying compiled [`Module`](wasmtime::Module).
    pub fn module(&self) -> &wasmtime::Module {
        &self.inner.module
    }
}

impl WasmInstance {
    /// Look up and call an exported function by name.
    ///
    /// Resolves the function on every call — for hot paths, use
    /// [`typed_func`](WasmInstance::typed_func) + [`TypedFunc::call`](wasmtime::TypedFunc::call)
    /// instead.
    ///
    /// ```rust,ignore
    /// inst.call::<(i32,), ()>("init", (4096,))?;
    /// let sum: i32 = inst.call::<(i32,), i32>("sum", (4096,))?;
    /// ```
    pub fn call<P, R>(&mut self, name: &str, params: P) -> Result<R, wasmtime::Error>
    where
        P: wasmtime::WasmParams,
        R: wasmtime::WasmResults,
    {
        let func = self
            .instance
            .get_typed_func::<P, R>(&mut self.store, name)?;
        func.call(&mut self.store, params)
    }

    /// Get a typed function handle for zero-overhead repeated calls.
    ///
    /// The returned [`TypedFunc`](wasmtime::TypedFunc) is `Copy`. Call it with
    /// `&mut inst.store`:
    ///
    /// ```rust,ignore
    /// let f = inst.typed_func::<(i32,), i32>("work").unwrap();
    /// b.iter(|| f.call(&mut inst.store, (1024,)).unwrap());
    /// ```
    pub fn typed_func<P, R>(
        &mut self,
        name: &str,
    ) -> Result<wasmtime::TypedFunc<P, R>, wasmtime::Error>
    where
        P: wasmtime::WasmParams,
        R: wasmtime::WasmResults,
    {
        self.instance
            .get_typed_func::<P, R>(&mut self.store, name)
    }

    /// Access the underlying wasmtime [`Instance`](wasmtime::Instance).
    pub fn raw_instance(&self) -> &wasmtime::Instance {
        &self.instance
    }
}
