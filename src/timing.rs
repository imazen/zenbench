//! Hardware timing and fences for precise benchmarking.
//!
//! This module is only compiled when `feature = "precise-timing"` is enabled.
//! It contains the only `unsafe` code in zenbench — TSC reads on x86_64,
//! counter reads on aarch64, and inline assembly fences.
//!
//! # TSC (Time Stamp Counter)
//!
//! On x86_64, uses `rdtsc` for the start timestamp and `rdtscp` for the end.
//! `rdtsc` needs an explicit `lfence` before it to prevent instruction reordering.
//! `rdtscp` is partially serializing (waits for prior instructions) so only needs
//! an `lfence` after it to prevent subsequent instructions from moving before it.
//!
//! TSC frequency is calibrated against `Instant` at startup. Results are converted
//! to nanoseconds — we never report raw cycles.
//!
//! # Fences
//!
//! `asm_fence()` emits an empty inline assembly block that LLVM cannot see through.
//! This is stronger than `compiler_fence(SeqCst)` for preventing dead code
//! elimination — LLVM knows that `compiler_fence` doesn't access memory, but
//! `asm!("")` is an opaque black box from the optimizer's perspective.

use core::arch::asm;

// ── Fences ──────────────────────────────────────────────────────────────

/// Empty inline assembly fence.
///
/// LLVM cannot reason through inline assembly at all, so this prevents:
/// - Loop hoisting of benchmark code past the fence
/// - Dead code elimination of stores before the fence
/// - Speculative reordering across the fence
///
/// This is strictly stronger than `std::sync::atomic::compiler_fence(SeqCst)`
/// for optimization prevention, because LLVM treats `compiler_fence` as a
/// known intrinsic it can reason about (e.g., it knows it doesn't access
/// memory), while `asm!("")` is genuinely opaque.
#[inline(always)]
#[allow(unsafe_code)]
pub fn asm_fence() {
    // SAFETY: empty assembly block with no side effects.
    // `nomem` is deliberately NOT specified — we want the compiler to assume
    // this might read/write any memory, preventing it from reordering
    // memory operations across the fence.
    unsafe {
        asm!("", options(nostack, preserves_flags));
    }
}

/// Combined compiler + asm fence.
///
/// Both the LLVM-level `asm!` barrier and the Rust-level `compiler_fence`.
/// The `compiler_fence` is technically redundant given the `asm!`, but
/// defense in depth doesn't hurt and makes the intent explicit.
#[inline(always)]
pub fn compiler_fence() {
    asm_fence();
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
}

// ── TSC Timer (x86_64) ─────────────────────────────────────────────────

/// Read the TSC for a timing START point.
///
/// Uses `lfence; rdtsc` — the lfence ensures all prior instructions have
/// retired before we read the counter, preventing earlier work from being
/// measured as part of a later benchmark.
#[cfg(target_arch = "x86_64")]
#[inline(always)]
#[allow(unsafe_code)]
pub fn tsc_start() -> u64 {
    let lo: u32;
    let hi: u32;
    // SAFETY: rdtsc is always available on x86_64. lfence serializes.
    unsafe {
        asm!(
            "lfence",
            "rdtsc",
            out("eax") lo,
            out("edx") hi,
            options(nostack, preserves_flags),
        );
    }
    ((hi as u64) << 32) | (lo as u64)
}

/// Read the TSC for a timing END point.
///
/// Uses `rdtscp; lfence` — rdtscp waits for prior instructions to complete
/// (it's partially serializing), and the lfence prevents subsequent
/// instructions from executing before we've read the counter.
#[cfg(target_arch = "x86_64")]
#[inline(always)]
#[allow(unsafe_code)]
pub fn tsc_end() -> u64 {
    let lo: u32;
    let hi: u32;
    // SAFETY: rdtscp is available on all modern x86_64 (required by our MSRV targets).
    // We discard the TSC_AUX value (ecx) since we don't need the processor ID.
    unsafe {
        asm!(
            "rdtscp",
            "lfence",
            out("eax") lo,
            out("edx") hi,
            out("ecx") _,  // TSC_AUX (processor ID) — unused
            options(nostack, preserves_flags),
        );
    }
    ((hi as u64) << 32) | (lo as u64)
}

/// Read the performance counter for a timing START point (aarch64).
///
/// Reads `cntvct_el0` (virtual counter) with an `isb` barrier before it
/// to ensure instruction serialization.
#[cfg(target_arch = "aarch64")]
#[inline(always)]
#[allow(unsafe_code)]
pub fn tsc_start() -> u64 {
    let val: u64;
    // SAFETY: cntvct_el0 is accessible from EL0 on all AArch64 platforms.
    unsafe {
        asm!(
            "isb",
            "mrs {val}, cntvct_el0",
            val = out(reg) val,
            options(nostack, preserves_flags),
        );
    }
    val
}

/// Read the performance counter for a timing END point (aarch64).
///
/// Reads `cntvct_el0` with an `isb` barrier after it.
#[cfg(target_arch = "aarch64")]
#[inline(always)]
#[allow(unsafe_code)]
pub fn tsc_end() -> u64 {
    let val: u64;
    // SAFETY: cntvct_el0 is accessible from EL0 on all AArch64 platforms.
    unsafe {
        asm!(
            "isb",
            "mrs {val}, cntvct_el0",
            "isb",
            val = out(reg) val,
            options(nostack, preserves_flags),
        );
    }
    val
}

// ── TSC Frequency Calibration ───────────────────────────────────────────

/// Calibrate the TSC/counter frequency by measuring against `Instant`.
///
/// Returns ticks per nanosecond (as f64). On x86_64 this is typically
/// ~3.0-5.0 (GHz-class CPUs). On aarch64, the counter frequency varies
/// but is usually in the 1-100 MHz range (0.001-0.1 ticks/ns).
///
/// Calibration strategy: sleep for increasing durations, take the ratio
/// of TSC delta to Instant delta. Accept when two consecutive measurements
/// agree within 0.1%.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
pub fn calibrate_tsc_frequency() -> f64 {
    let mut prev_ratio = 0.0_f64;

    for power in 0..9 {
        let sleep_ms = 1u64 << power; // 1, 2, 4, 8, ..., 256 ms
        let sleep_dur = std::time::Duration::from_millis(sleep_ms);

        let tsc_before = tsc_start();
        let instant_before = std::time::Instant::now();
        std::thread::sleep(sleep_dur);
        let tsc_after = tsc_end();
        let instant_after = std::time::Instant::now();

        let tsc_delta = tsc_after.wrapping_sub(tsc_before) as f64;
        let ns_delta = instant_after.duration_since(instant_before).as_nanos() as f64;

        if ns_delta < 1.0 {
            continue;
        }
        let ratio = tsc_delta / ns_delta;

        if prev_ratio > 0.0 {
            let relative_diff = ((ratio - prev_ratio) / prev_ratio).abs();
            if relative_diff < 0.001 {
                return ratio;
            }
        }
        prev_ratio = ratio;
    }

    // Fallback: use the last measurement
    prev_ratio
}

/// Check whether the TSC is invariant (constant frequency regardless of
/// CPU power state). Non-invariant TSCs are useless for timing.
#[cfg(target_arch = "x86_64")]
#[allow(unsafe_code)]
pub fn tsc_is_invariant() -> bool {
    // CPUID function 0x80000007, EDX bit 8 = "Invariant TSC"
    // rbx is reserved by LLVM, so we must save/restore it manually.
    let max_extended: u32;
    unsafe {
        asm!(
            "push rbx",
            "mov eax, 0x80000000",
            "cpuid",
            "pop rbx",
            out("eax") max_extended,
            out("ecx") _,
            out("edx") _,
            options(preserves_flags),
        );
    }
    if max_extended < 0x80000007 {
        return false;
    }
    let edx: u32;
    unsafe {
        asm!(
            "push rbx",
            "mov eax, 0x80000007",
            "cpuid",
            "pop rbx",
            out("eax") _,
            out("ecx") _,
            out("edx") edx,
            options(preserves_flags),
        );
    }
    (edx & (1 << 8)) != 0
}

#[cfg(target_arch = "aarch64")]
pub fn tsc_is_invariant() -> bool {
    // AArch64's cntvct_el0 runs at a fixed frequency by spec.
    true
}

// ── Ticks-to-Nanoseconds Conversion ────────────────────────────────────

/// Convert a tick delta to nanoseconds using the calibrated frequency.
#[inline(always)]
pub fn ticks_to_ns(ticks: u64, ticks_per_ns: f64) -> u64 {
    if ticks_per_ns > 0.0 {
        (ticks as f64 / ticks_per_ns) as u64
    } else {
        0
    }
}

// ── TscTimer ────────────────────────────────────────────────────────────

/// Calibrated TSC timer that converts hardware ticks to nanoseconds.
///
/// Created once at engine startup. If the TSC is not invariant or
/// calibration fails, falls back to `None` (use `Instant` instead).
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
pub struct TscTimer {
    ticks_per_ns: f64,
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
impl TscTimer {
    /// Try to create a calibrated TSC timer.
    ///
    /// Returns `None` if the TSC is not invariant or calibration produces
    /// an implausible frequency.
    pub fn new() -> Option<Self> {
        if !tsc_is_invariant() {
            return None;
        }
        let freq = calibrate_tsc_frequency();
        // Sane range: 0.001 (1 MHz) to 10.0 (10 GHz)
        if freq > 0.001 && freq < 10.0 {
            Some(Self { ticks_per_ns: freq })
        } else {
            None
        }
    }

    /// Measure elapsed nanoseconds for a closure using TSC.
    #[inline(always)]
    #[allow(dead_code)] // Public API for direct use
    pub fn measure<R>(&self, f: impl FnOnce() -> R) -> (u64, R) {
        compiler_fence();
        let start = tsc_start();
        let result = f();
        let end = tsc_end();
        compiler_fence();
        let ticks = end.wrapping_sub(start);
        (ticks_to_ns(ticks, self.ticks_per_ns), result)
    }

    /// The calibrated frequency in ticks per nanosecond.
    pub fn ticks_per_ns(&self) -> f64 {
        self.ticks_per_ns
    }
}

// Stub for unsupported architectures
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
pub struct TscTimer;

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
impl TscTimer {
    pub fn new() -> Option<Self> {
        None
    }
}

// ── Stack alignment jitter ───────────────────────────────────────────────

/// Call a benchmark function with stack alignment jitter.
///
/// Burns `depth` stack frames (each ~64 bytes) via recursion before calling
/// the benchmark. This shifts the stack pointer by approximately `depth × 64`
/// bytes, varying cache-line alignment of stack variables across samples.
/// Defeats systematic bias from lucky/unlucky alignment (Mytkowicz et al.,
/// ASPLOS 2009).
///
/// Fully safe — uses recursive calls with padded frames. The `black_box`
/// on the pad array prevents the optimizer from collapsing frames or
/// applying tail-call optimization.
///
/// Overhead: ~1-2ns per recursion level. With typical depths of 0-64
/// (for 0-4096 byte offsets), overhead is 0-128ns — negligible for
/// samples > 10µs.
#[inline(never)]
pub fn stack_jitter_call(
    func: &mut crate::bench::BenchFn,
    bencher: &mut crate::bench::Bencher,
    depth: usize,
) {
    // Pad: 64 bytes of stack space per frame. black_box prevents
    // the compiler from optimizing away the allocation or merging frames.
    let _pad: [u8; 64] = std::hint::black_box([0u8; 64]);
    if depth == 0 {
        func.call(bencher);
    } else {
        stack_jitter_call(func, bencher, depth - 1);
    }
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asm_fence_does_not_panic() {
        asm_fence();
        compiler_fence();
    }

    #[test]
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    fn tsc_reads_increase() {
        let a = tsc_start();
        // Spin enough to guarantee the counter advances even on low-freq
        // counters like aarch64 cntvct_el0 (~24 MHz on Apple Silicon).
        let mut x = 0u64;
        for i in 0..1000 {
            x = x.wrapping_add(std::hint::black_box(i));
        }
        std::hint::black_box(x);
        let b = tsc_end();
        assert!(b > a, "TSC should advance: {a} -> {b}");
    }

    #[test]
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    fn tsc_calibration_is_sane() {
        let freq = calibrate_tsc_frequency();
        // Should be between 0.001 (1 MHz counter) and 10.0 (10 GHz CPU)
        assert!(
            freq > 0.001 && freq < 10.0,
            "TSC frequency {freq} ticks/ns is outside sane range"
        );
    }

    #[test]
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    fn tsc_is_invariant_check() {
        // On modern hardware this should be true. If it's false,
        // that's not a test failure — just informational.
        let invariant = tsc_is_invariant();
        eprintln!("TSC invariant: {invariant}");
    }

    #[test]
    fn stack_jitter_call_works() {
        use std::sync::atomic::{AtomicBool, Ordering};

        static CALLED: AtomicBool = AtomicBool::new(false);

        let mut func = crate::bench::BenchFn::new(|b: &mut crate::bench::Bencher| {
            CALLED.store(true, Ordering::Relaxed);
            b.iter(|| std::hint::black_box(42u64));
        });
        let mut bencher = crate::bench::Bencher::new(10);

        // Depth 0 — direct call
        CALLED.store(false, Ordering::Relaxed);
        stack_jitter_call(&mut func, &mut bencher, 0);
        assert!(CALLED.load(Ordering::Relaxed), "depth=0 should call func");

        // Depth 10 — ~640 bytes of stack shift
        CALLED.store(false, Ordering::Relaxed);
        stack_jitter_call(&mut func, &mut bencher, 10);
        assert!(CALLED.load(Ordering::Relaxed), "depth=10 should call func");

        // Depth 50 — ~3200 bytes of stack shift
        CALLED.store(false, Ordering::Relaxed);
        stack_jitter_call(&mut func, &mut bencher, 50);
        assert!(CALLED.load(Ordering::Relaxed), "depth=50 should call func");
    }

    #[test]
    fn stack_jitter_different_depths_produce_results() {
        // Run a simple benchmark at different jitter depths and verify
        // all produce valid (positive) elapsed times
        for depth in [0, 5, 20, 50] {
            let mut func = crate::bench::BenchFn::new(|b: &mut crate::bench::Bencher| {
                b.iter(|| std::hint::black_box(42u64));
            });
            let mut bencher = crate::bench::Bencher::new(100);
            stack_jitter_call(&mut func, &mut bencher, depth);
            assert!(
                bencher.elapsed_ns > 0,
                "depth={depth} should produce positive elapsed_ns"
            );
        }
    }

    #[test]
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    fn ticks_to_ns_basic() {
        // 3 GHz = 3.0 ticks/ns
        let ns = ticks_to_ns(3000, 3.0);
        assert_eq!(ns, 1000);
    }
}
