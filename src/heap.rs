//! Process-wide requested heap accounting for dedicated memory runs.
//!
//! Install [`HeapProfiler`] as the global allocator. It serializes allocation
//! operations across every Rust thread, including pools created before a phase.
//! This changes execution scheduling and allocation cost: use a separate binary
//! for latency. Direct mappings, native-library allocators, allocator metadata,
//! fragmentation and device memory are outside its coverage. Realloc records the
//! successful old-to-new size transition, not hidden transient storage inside
//! the system allocator. No phase resets the process live count.
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// System allocator with serialized, process-wide requested-byte accounting.
///
/// Enable `heap-profiling` and install exactly one global allocator:
/// ```
/// #[global_allocator]
/// static ALLOC: zenbench::HeapProfiler = zenbench::HeapProfiler::system();
/// let phase = zenbench::HeapPhase::begin().unwrap();
/// let data = vec![1u8; 4096];
/// std::hint::black_box(&data);
/// let memory = phase.finish().unwrap();
/// assert!(memory.peak_live_bytes >= memory.entry_live_bytes + 4096);
/// ```
/// Unrelated allocations in any Rust thread are included. Quiesce background
/// work and join workers before finishing a phase. This allocator cannot be
/// composed with the thread-local timing profiler.
pub struct HeapProfiler {
    _private: (),
}
impl HeapProfiler {
    /// Construct the profiler around the system allocator.
    pub const fn system() -> Self {
        Self { _private: () }
    }
}

/// A coherent process-wide snapshot, including allocations before a phase.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub struct HeapSnapshot {
    /// Currently live requested bytes.
    pub live_bytes: u64,
    /// Maximum live bytes since the allocator's first call.
    pub lifetime_peak_bytes: u64,
    /// Sum of successful allocation sizes and successful realloc new sizes.
    pub requested_bytes: u64,
    /// Successful alloc/alloc_zeroed operations (not realloc).
    pub allocations: u64,
    /// Successful realloc operations.
    pub reallocations: u64,
    /// Requests for which the system allocator returned null.
    pub failed_requests: u64,
}
impl HeapSnapshot {
    /// Read the ledger under the same lock used by allocator operations.
    pub fn get() -> Result<Self, HeapError> {
        initialize();
        let guard = LEDGER.lock();
        guard.snapshot()
    }
}
/// Exact requested live bytes at a phase's entry, maximum and exit.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub struct HeapMeasurement {
    pub entry_live_bytes: u64,
    pub peak_live_bytes: u64,
    pub exit_live_bytes: u64,
    /// Successful requested-byte traffic during this phase; never a heap peak.
    pub requested_bytes: u64,
    pub allocations: u64,
    pub reallocations: u64,
    pub failed_requests: u64,
}
/// Failures which prevent a valid heap measurement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum HeapError {
    NotInstalled,
    PhaseAlreadyActive,
    CounterOverflow,
}
impl std::fmt::Display for HeapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NotInstalled => "HeapProfiler is not installed",
            Self::PhaseAlreadyActive => "another heap phase is active",
            Self::CounterOverflow => "heap ledger overflow or inconsistent live count",
        })
    }
}
impl std::error::Error for HeapError {}

/// Exclusive process-wide phase. Dropping it ends the phase without a result.
///
/// First use probes the global allocator before entering the phase. Subsequent
/// boundaries never allocate and never discard existing live bytes.
/// Only one phase can be active, even across threads. Nested phases return an
/// error. Memory retained from preparation remains in every subsequent watermark.
#[must_use]
pub struct HeapPhase {
    entry: HeapSnapshot,
    active: bool,
}
impl HeapPhase {
    pub fn begin() -> Result<Self, HeapError> {
        initialize();
        let guard = LEDGER.lock();
        let entry = guard.snapshot()?;
        if LEDGER.phase_active.load(Ordering::Relaxed) {
            return Err(HeapError::PhaseAlreadyActive);
        }
        LEDGER.phase_peak.store(entry.live_bytes, Ordering::Relaxed);
        LEDGER.phase_active.store(true, Ordering::Relaxed);
        Ok(Self {
            entry,
            active: true,
        })
    }
    /// End the phase after all participating work has joined.
    pub fn finish(mut self) -> Result<HeapMeasurement, HeapError> {
        let guard = LEDGER.lock();
        self.active = false;
        LEDGER.phase_active.store(false, Ordering::Relaxed);
        let exit = guard.snapshot()?;
        Ok(HeapMeasurement {
            entry_live_bytes: self.entry.live_bytes,
            peak_live_bytes: LEDGER.phase_peak.load(Ordering::Relaxed),
            exit_live_bytes: exit.live_bytes,
            requested_bytes: exit.requested_bytes - self.entry.requested_bytes,
            allocations: exit.allocations - self.entry.allocations,
            reallocations: exit.reallocations - self.entry.reallocations,
            failed_requests: exit.failed_requests - self.entry.failed_requests,
        })
    }
}
impl Drop for HeapPhase {
    fn drop(&mut self) {
        if self.active {
            let _guard = LEDGER.lock();
            LEDGER.phase_active.store(false, Ordering::Relaxed);
        }
    }
}

// A spin lock avoids lazy OS-mutex initialization allocating through the very
// allocator being measured. Atomic fields keep the ledger entirely safe Rust.
// All ledger accesses hold Guard; release/acquire orders both the system
// allocation and its accounting against every phase/snapshot operation.
struct Ledger {
    locked: AtomicBool,
    installed: AtomicBool,
    valid: AtomicBool,
    phase_active: AtomicBool,
    live: AtomicU64,
    peak: AtomicU64,
    phase_peak: AtomicU64,
    requested: AtomicU64,
    allocations: AtomicU64,
    reallocations: AtomicU64,
    failed: AtomicU64,
}
impl Ledger {
    const fn new() -> Self {
        Self {
            locked: AtomicBool::new(false),
            installed: AtomicBool::new(false),
            valid: AtomicBool::new(true),
            phase_active: AtomicBool::new(false),
            live: AtomicU64::new(0),
            peak: AtomicU64::new(0),
            phase_peak: AtomicU64::new(0),
            requested: AtomicU64::new(0),
            allocations: AtomicU64::new(0),
            reallocations: AtomicU64::new(0),
            failed: AtomicU64::new(0),
        }
    }
    fn lock(&self) -> Guard<'_> {
        while self
            .locked
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            std::hint::spin_loop();
        }
        Guard(self)
    }
}
static LEDGER: Ledger = Ledger::new();
// Some runtimes enter main without calling the Rust allocator. Probe before
// taking the ledger lock: the allocator itself needs that same lock.
fn initialize() {
    if !LEDGER.installed.load(Ordering::Acquire) {
        let probe = Box::new(0u8);
        std::hint::black_box(&probe);
        drop(probe);
    }
}
struct Guard<'a>(&'a Ledger);
impl Drop for Guard<'_> {
    fn drop(&mut self) {
        self.0.locked.store(false, Ordering::Release);
    }
}
impl Guard<'_> {
    fn snapshot(&self) -> Result<HeapSnapshot, HeapError> {
        let l = self.0;
        if !l.installed.load(Ordering::Relaxed) {
            return Err(HeapError::NotInstalled);
        }
        if !l.valid.load(Ordering::Relaxed) {
            return Err(HeapError::CounterOverflow);
        }
        Ok(HeapSnapshot {
            live_bytes: l.live.load(Ordering::Relaxed),
            lifetime_peak_bytes: l.peak.load(Ordering::Relaxed),
            requested_bytes: l.requested.load(Ordering::Relaxed),
            allocations: l.allocations.load(Ordering::Relaxed),
            reallocations: l.reallocations.load(Ordering::Relaxed),
            failed_requests: l.failed.load(Ordering::Relaxed),
        })
    }
    fn add(&self, field: &AtomicU64, n: u64) {
        match field.load(Ordering::Relaxed).checked_add(n) {
            Some(v) => field.store(v, Ordering::Relaxed),
            None => self.0.valid.store(false, Ordering::Relaxed),
        }
    }
    fn transition(&self, old: u64, new: u64) {
        let l = self.0;
        let next = l
            .live
            .load(Ordering::Relaxed)
            .checked_sub(old)
            .and_then(|v| v.checked_add(new));
        if let Some(next) = next {
            l.live.store(next, Ordering::Relaxed);
            l.peak
                .store(l.peak.load(Ordering::Relaxed).max(next), Ordering::Relaxed);
            if l.phase_active.load(Ordering::Relaxed) {
                l.phase_peak.store(
                    l.phase_peak.load(Ordering::Relaxed).max(next),
                    Ordering::Relaxed,
                );
            }
        } else {
            l.valid.store(false, Ordering::Relaxed);
        }
    }
    fn allocated(&self, size: usize, success: bool) {
        self.0.installed.store(true, Ordering::Relaxed);
        if success {
            self.transition(0, size as u64);
            self.add(&self.0.requested, size as u64);
            self.add(&self.0.allocations, 1);
        } else {
            self.add(&self.0.failed, 1);
        }
    }
    fn reallocated(&self, old: usize, new: usize, success: bool) {
        if success {
            self.transition(old as u64, new as u64);
            self.add(&self.0.requested, new as u64);
            self.add(&self.0.reallocations, 1);
        } else {
            self.add(&self.0.failed, 1);
        }
    }
}

// User-approved exception: only unchanged GlobalAlloc delegation is unsafe.
// The guard and ledger are safe Rust, allocate nothing, do not format/panic,
// and cover the entire system call plus accounting in one serialization order.
#[allow(unsafe_code)]
unsafe impl GlobalAlloc for HeapProfiler {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let guard = LEDGER.lock();
        // SAFETY: forward the caller's valid layout unchanged to System.
        let ptr = unsafe { System.alloc(layout) };
        guard.allocated(layout.size(), !ptr.is_null());
        ptr
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let guard = LEDGER.lock();
        // SAFETY: forward the caller's valid layout unchanged to System.
        let ptr = unsafe { System.alloc_zeroed(layout) };
        guard.allocated(layout.size(), !ptr.is_null());
        ptr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let guard = LEDGER.lock();
        // SAFETY: allocation pointers/layouts are System's, forwarded unchanged.
        unsafe { System.dealloc(ptr, layout) };
        guard.transition(layout.size() as u64, 0);
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let guard = LEDGER.lock();
        // SAFETY: preserve caller's valid pointer/layout/new-size contract.
        let result = unsafe { System.realloc(ptr, layout, new_size) };
        guard.reallocated(layout.size(), new_size, !result.is_null());
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failures_preserve_live_bytes_and_original_realloc_allocation() {
        let l = Ledger::new();
        let g = l.lock();
        g.allocated(100, true);
        g.allocated(500, false);
        g.reallocated(100, 1000, false);
        let s = g.snapshot().unwrap();
        assert_eq!(s.live_bytes, 100);
        assert_eq!(s.lifetime_peak_bytes, 100);
        assert_eq!(s.failed_requests, 2);
        g.reallocated(100, 200, true);
        g.reallocated(200, 50, true);
        g.transition(50, 0);
        let s = g.snapshot().unwrap();
        assert_eq!(s.live_bytes, 0);
        assert_eq!(s.lifetime_peak_bytes, 200);
        assert_eq!(s.requested_bytes, 350);
    }
    #[test]
    fn invalid_ledger_fails_instead_of_wrapping() {
        let l = Ledger::new();
        let g = l.lock();
        g.allocated(1, true);
        g.transition(2, 0);
        assert!(matches!(g.snapshot(), Err(HeapError::CounterOverflow)));
    }
}
