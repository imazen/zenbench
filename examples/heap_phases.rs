//! Real allocator controls, including pools created before measurement.
#![forbid(unsafe_code)]
use std::sync::{Arc, Barrier};
use zenbench::{HeapPhase, HeapProfiler, HeapSnapshot};
#[global_allocator]
static ALLOC: HeapProfiler = HeapProfiler::system();
fn main() {
    // Preparation persists across phase boundaries. Dropping older allocations
    // must remain visible; resetting a phase must not zero the live ledger.
    let retained = vec![3u8; 4096];
    std::hint::black_box(&retained);
    let phase = HeapPhase::begin().unwrap();
    assert!(HeapPhase::begin().is_err());
    let spike = vec![1u8; 65536];
    std::hint::black_box(&spike);
    drop(spike);
    let transient = phase.finish().unwrap();
    assert_eq!(
        transient.peak_live_bytes,
        transient.entry_live_bytes + 65536
    );
    assert_eq!(transient.exit_live_bytes, transient.entry_live_bytes);
    let phase = HeapPhase::begin().unwrap();
    drop(retained);
    let retained_free = phase.finish().unwrap();
    assert_eq!(
        retained_free.entry_live_bytes - retained_free.exit_live_bytes,
        4096
    );
    assert_eq!(
        retained_free.peak_live_bytes,
        retained_free.entry_live_bytes
    );

    // Keep pre-existing ordinary workers alive until the phase is closed, so
    // runtime thread teardown does not obscure the exact allocation oracle.
    let ready = Arc::new(Barrier::new(3));
    let allocated = Arc::new(Barrier::new(3));
    let release = Arc::new(Barrier::new(3));
    let freed = Arc::new(Barrier::new(3));
    let finish = Arc::new(Barrier::new(3));
    let threads: Vec<_> = (0..2)
        .map(|_| {
            let (ready, allocated, release, freed, finish) = (
                ready.clone(),
                allocated.clone(),
                release.clone(),
                freed.clone(),
                finish.clone(),
            );
            std::thread::spawn(move || {
                ready.wait();
                allocated.wait();
                release.wait();
                freed.wait();
                finish.wait();
                ready.wait();
                let bytes = vec![2u8; 1024 * 1024];
                std::hint::black_box(&bytes);
                allocated.wait();
                release.wait();
                drop(bytes);
                freed.wait();
                finish.wait();
            })
        })
        .collect();
    ready.wait();
    // macOS initializes barrier synchronization storage on first contention.
    // Include that storage in preparation, before the allocation-only phase.
    allocated.wait();
    release.wait();
    freed.wait();
    finish.wait();
    let phase = HeapPhase::begin().unwrap();
    ready.wait();
    allocated.wait();
    release.wait();
    freed.wait();
    let overlapping = phase.finish().unwrap();
    assert_eq!(
        overlapping.peak_live_bytes,
        overlapping.entry_live_bytes + 2 * 1024 * 1024
    );
    assert_eq!(overlapping.exit_live_bytes, overlapping.entry_live_bytes);
    finish.wait();
    for thread in threads {
        thread.join().unwrap();
    }

    let sequential = sequential_workers();
    let transferred = cross_thread_free();
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(2)
        .build()
        .unwrap();
    pool.broadcast(|_| {}); // Existing pool, warmed before the phase.
    let barrier = Barrier::new(2);
    let phase = HeapPhase::begin().unwrap();
    pool.broadcast(|_| {
        let bytes = vec![7u8; 1024 * 1024];
        std::hint::black_box(&bytes);
        barrier.wait();
        barrier.wait();
        drop(bytes);
    });
    let rayon = phase.finish().unwrap();
    assert!(rayon.peak_live_bytes >= rayon.entry_live_bytes + 2 * 1024 * 1024);

    // Successful growth/shrink and fallible requests preserve the live ledger.
    let mut data = vec![0u8; 128];
    let phase = HeapPhase::begin().unwrap();
    data.try_reserve_exact(128).unwrap();
    data.truncate(32);
    data.shrink_to_fit();
    let resize = phase.finish().unwrap();
    assert_eq!(resize.peak_live_bytes, resize.entry_live_bytes + 128);
    assert_eq!(resize.exit_live_bytes + 96, resize.entry_live_bytes);
    // Explicit opt-in: a 64-bit impossible request. On a 32-bit process a
    // similar size may be representable and available, so the caller chooses.
    if std::env::args().any(|s| s == "--exercise-oom") {
        if usize::BITS != 64 {
            panic!("OOM oracle requires a 64-bit process");
        }
        let before = HeapSnapshot::get().unwrap();
        assert!(data.try_reserve_exact(isize::MAX as usize - 4096).is_err());
        let after = HeapSnapshot::get().unwrap();
        assert_eq!(before.live_bytes, after.live_bytes);
        assert_eq!(before.lifetime_peak_bytes, after.lifetime_peak_bytes);
        assert_eq!(after.failed_requests, before.failed_requests + 1);
        let mut fresh = Vec::<u8>::new();
        let before = HeapSnapshot::get().unwrap();
        assert!(fresh.try_reserve_exact(isize::MAX as usize - 4096).is_err());
        let after = HeapSnapshot::get().unwrap();
        assert_eq!(before.live_bytes, after.live_bytes);
        assert_eq!(after.failed_requests, before.failed_requests + 1);
    }
    drop(data);
    println!(
        "{}",
        serde_json::json!({"transient":transient,"retained_free":retained_free,"overlapping":overlapping,"preexisting_rayon":rayon,"resize":resize,"sequential_workers":sequential,"cross_thread_free":transferred,"lifetime":HeapSnapshot::get().unwrap()})
    );
}

fn sequential_workers() -> zenbench::HeapMeasurement {
    let gates: Vec<_> = (0..2).map(|_| Arc::new(Barrier::new(2))).collect();
    let threads: Vec<_> = gates
        .iter()
        .map(|gate| {
            let gate = gate.clone();
            std::thread::spawn(move || {
                gate.wait();
                gate.wait();
                let bytes = vec![5u8; 1024 * 1024];
                std::hint::black_box(&bytes);
                gate.wait();
                gate.wait();
                drop(bytes);
                gate.wait();
                gate.wait();
            })
        })
        .collect();
    for gate in &gates {
        gate.wait();
    }
    let phase = HeapPhase::begin().unwrap();
    for gate in &gates {
        gate.wait();
        gate.wait();
        gate.wait();
        gate.wait();
    }
    let result = phase.finish().unwrap();
    assert_eq!(
        result.peak_live_bytes,
        result.entry_live_bytes + 1024 * 1024
    );
    assert_eq!(result.exit_live_bytes, result.entry_live_bytes);
    for gate in &gates {
        gate.wait();
    }
    for thread in threads {
        thread.join().unwrap();
    }
    result
}
fn cross_thread_free() -> zenbench::HeapMeasurement {
    let bytes = vec![4u8; 32768];
    std::hint::black_box(&bytes);
    let gate = Arc::new(Barrier::new(2));
    let worker_gate = gate.clone();
    let worker = std::thread::spawn(move || {
        worker_gate.wait();
        worker_gate.wait();
        drop(bytes);
        worker_gate.wait();
        worker_gate.wait();
    });
    gate.wait();
    let phase = HeapPhase::begin().unwrap();
    gate.wait();
    gate.wait();
    let result = phase.finish().unwrap();
    assert_eq!(result.entry_live_bytes - result.exit_live_bytes, 32768);
    assert_eq!(result.peak_live_bytes, result.entry_live_bytes);
    gate.wait();
    worker.join().unwrap();
    result
}
