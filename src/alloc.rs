//! Allocation profiler for tracking heap usage per benchmark.
//!
//! Wraps any [`GlobalAlloc`] to count allocations, deallocations, reallocations,
//! and bytes transferred. Install as the global allocator in your benchmark binary:
//!
//! ```rust,ignore
//! #[global_allocator]
//! static ALLOC: zenbench::AllocProfiler = zenbench::AllocProfiler::system();
//! ```
//!
//! When installed, zenbench automatically tracks allocation counts and bytes
//! per iteration for every benchmark. Results appear in the report alongside
//! timing data:
//!
//! ```text
//! │ benchmark   │  mean  │ allocs │  bytes │
//! │ vec_push    │ 245ns  │      3 │    112 │
//! │ smallvec    │  89ns  │      0 │      0 │
//! ```
//!
//! ## How it works
//!
//! Each `alloc`/`dealloc`/`realloc` call increments thread-local counters via
//! `Cell<u64>` — no atomics, no contention. The engine snapshots these counters
//! before and after each benchmark sample to compute per-iteration stats.
//!
//! ## Limitations
//!
//! - Allocations in threads not managed by zenbench (e.g., rayon worker threads)
//!   are not counted. Only the thread calling `Bencher::iter()` is tracked.
//! - The profiler adds ~1-2ns per allocation. This is included in timing and
//!   is NOT subtracted (unlike loop overhead). For allocation-heavy benchmarks,
//!   this is negligible; for allocation-free hot loops, the profiler has zero
//!   overhead.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

// ── Thread-local counters ───────────────────────────────────────────────

/// Whether any allocation has been routed through the profiler.
/// Used to distinguish "profiler not installed" from "zero allocations."
static PROFILER_ACTIVE: AtomicBool = AtomicBool::new(false);

thread_local! {
    static ALLOC_COUNT: Cell<u64> = const { Cell::new(0) };
    static DEALLOC_COUNT: Cell<u64> = const { Cell::new(0) };
    static REALLOC_COUNT: Cell<u64> = const { Cell::new(0) };
    static BYTES_ALLOCATED: Cell<u64> = const { Cell::new(0) };
    static BYTES_DEALLOCATED: Cell<u64> = const { Cell::new(0) };
}

// ── AllocProfiler ───────────────────────────────────────────────────────

/// Allocation profiler that wraps a [`GlobalAlloc`] to track heap usage.
///
/// # Usage
///
/// Wrap the system allocator:
/// ```rust,ignore
/// #[global_allocator]
/// static ALLOC: zenbench::AllocProfiler = zenbench::AllocProfiler::system();
/// ```
///
/// Or wrap a custom allocator (e.g., mimalloc):
/// ```rust,ignore
/// #[global_allocator]
/// static ALLOC: zenbench::AllocProfiler<mimalloc::MiMalloc> =
///     zenbench::AllocProfiler::new(mimalloc::MiMalloc);
/// ```
pub struct AllocProfiler<A = System> {
    alloc: A,
}

impl AllocProfiler {
    /// Profile the system allocator.
    #[inline]
    pub const fn system() -> Self {
        Self::new(System)
    }
}

impl<A> AllocProfiler<A> {
    /// Profile any [`GlobalAlloc`] implementation.
    #[inline]
    pub const fn new(alloc: A) -> Self {
        Self { alloc }
    }
}

#[allow(unsafe_code)]
unsafe impl<A: GlobalAlloc> GlobalAlloc for AllocProfiler<A> {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: Delegating to the wrapped allocator with the same layout.
        let ptr = unsafe { self.alloc.alloc(layout) };
        if !ptr.is_null() {
            PROFILER_ACTIVE.store(true, Ordering::Relaxed);
            ALLOC_COUNT.with(|c| c.set(c.get() + 1));
            BYTES_ALLOCATED.with(|c| c.set(c.get() + layout.size() as u64));
        }
        ptr
    }

    #[inline]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: Delegating to the wrapped allocator with the same layout.
        let ptr = unsafe { self.alloc.alloc_zeroed(layout) };
        if !ptr.is_null() {
            PROFILER_ACTIVE.store(true, Ordering::Relaxed);
            ALLOC_COUNT.with(|c| c.set(c.get() + 1));
            BYTES_ALLOCATED.with(|c| c.set(c.get() + layout.size() as u64));
        }
        ptr
    }

    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: Delegating to the wrapped allocator with the same ptr and layout.
        unsafe { self.alloc.dealloc(ptr, layout) };
        DEALLOC_COUNT.with(|c| c.set(c.get() + 1));
        BYTES_DEALLOCATED.with(|c| c.set(c.get() + layout.size() as u64));
    }

    #[inline]
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: Delegating to the wrapped allocator with the same arguments.
        let new_ptr = unsafe { self.alloc.realloc(ptr, layout, new_size) };
        if !new_ptr.is_null() {
            REALLOC_COUNT.with(|c| c.set(c.get() + 1));
            // Track the net change: old size deallocated, new size allocated
            BYTES_DEALLOCATED.with(|c| c.set(c.get() + layout.size() as u64));
            BYTES_ALLOCATED.with(|c| c.set(c.get() + new_size as u64));
        }
        new_ptr
    }
}

// SAFETY: AllocProfiler delegates to the wrapped allocator which must be Sync.
// The thread-local counters are inherently thread-safe (Cell is single-threaded).
#[allow(unsafe_code)]
unsafe impl<A: Sync> Sync for AllocProfiler<A> {}

// ── Snapshot ────────────────────────────────────────────────────────────

/// A point-in-time snapshot of allocation counters for the current thread.
#[derive(Debug, Clone, Copy, Default)]
pub struct AllocSnapshot {
    pub allocs: u64,
    pub deallocs: u64,
    pub reallocs: u64,
    pub bytes_allocated: u64,
    pub bytes_deallocated: u64,
}

impl AllocSnapshot {
    /// Capture the current thread's allocation counters.
    #[inline]
    pub fn now() -> Self {
        Self {
            allocs: ALLOC_COUNT.with(|c| c.get()),
            deallocs: DEALLOC_COUNT.with(|c| c.get()),
            reallocs: REALLOC_COUNT.with(|c| c.get()),
            bytes_allocated: BYTES_ALLOCATED.with(|c| c.get()),
            bytes_deallocated: BYTES_DEALLOCATED.with(|c| c.get()),
        }
    }

    /// Compute the difference between two snapshots (self - before).
    #[inline]
    pub fn delta(self, before: Self) -> Self {
        Self {
            allocs: self.allocs.saturating_sub(before.allocs),
            deallocs: self.deallocs.saturating_sub(before.deallocs),
            reallocs: self.reallocs.saturating_sub(before.reallocs),
            bytes_allocated: self.bytes_allocated.saturating_sub(before.bytes_allocated),
            bytes_deallocated: self
                .bytes_deallocated
                .saturating_sub(before.bytes_deallocated),
        }
    }
}

/// Whether the `AllocProfiler` is installed and has seen at least one allocation.
#[inline]
pub fn is_active() -> bool {
    PROFILER_ACTIVE.load(Ordering::Relaxed)
}

// ── Per-benchmark stats ─────────────────────────────────────────────────

/// Allocation statistics for a benchmark, averaged per iteration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct AllocStats {
    /// Average number of allocations per iteration.
    pub allocs_per_iter: f64,
    /// Average number of deallocations per iteration.
    pub deallocs_per_iter: f64,
    /// Average number of reallocations per iteration.
    pub reallocs_per_iter: f64,
    /// Average bytes allocated per iteration.
    pub bytes_per_iter: f64,
    /// Average bytes deallocated per iteration.
    pub bytes_dealloc_per_iter: f64,
}

impl AllocStats {
    /// Compute per-iteration stats from accumulated totals.
    pub fn from_totals(
        total_allocs: u64,
        total_deallocs: u64,
        total_reallocs: u64,
        total_bytes_alloc: u64,
        total_bytes_dealloc: u64,
        total_iterations: u64,
    ) -> Self {
        let n = total_iterations.max(1) as f64;
        Self {
            allocs_per_iter: total_allocs as f64 / n,
            deallocs_per_iter: total_deallocs as f64 / n,
            reallocs_per_iter: total_reallocs as f64 / n,
            bytes_per_iter: total_bytes_alloc as f64 / n,
            bytes_dealloc_per_iter: total_bytes_dealloc as f64 / n,
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_delta() {
        let before = AllocSnapshot {
            allocs: 10,
            deallocs: 5,
            reallocs: 2,
            bytes_allocated: 1000,
            bytes_deallocated: 500,
        };
        let after = AllocSnapshot {
            allocs: 15,
            deallocs: 8,
            reallocs: 3,
            bytes_allocated: 2000,
            bytes_deallocated: 900,
        };
        let delta = after.delta(before);
        assert_eq!(delta.allocs, 5);
        assert_eq!(delta.deallocs, 3);
        assert_eq!(delta.reallocs, 1);
        assert_eq!(delta.bytes_allocated, 1000);
        assert_eq!(delta.bytes_deallocated, 400);
    }

    #[test]
    fn alloc_stats_from_totals() {
        let stats = AllocStats::from_totals(100, 100, 10, 8000, 8000, 50);
        assert!((stats.allocs_per_iter - 2.0).abs() < f64::EPSILON);
        assert!((stats.bytes_per_iter - 160.0).abs() < f64::EPSILON);
    }

    #[test]
    fn snapshot_now_returns_something() {
        // Just verify it doesn't panic — actual alloc tracking requires
        // the AllocProfiler to be the global allocator.
        let snap = AllocSnapshot::now();
        // Just verify it returns something (counters are u64, always >= 0)
        let _ = snap.allocs;
    }
}
