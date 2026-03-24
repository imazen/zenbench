//! Allocation profiling benchmark — verifies AllocProfiler tracks heap usage.
//!
//! This binary installs AllocProfiler as the global allocator, then runs
//! benchmarks that allocate known amounts. The alloc_stats in results should
//! reflect the expected allocation patterns.
//!
//! Run: `cargo bench --bench alloc_profiling`

#[global_allocator]
static ALLOC: zenbench::AllocProfiler = zenbench::AllocProfiler::system();

zenbench::main!(|suite| {
    suite.compare("alloc_tracking", |group| {
        group.config().max_rounds(30).auto_rounds(false);

        // No allocations — should show 0 allocs/iter
        group.bench("no_alloc", |b| {
            b.iter(|| {
                let mut v = 0u64;
                for i in 0..100 {
                    v = v.wrapping_add(zenbench::black_box(i));
                }
                zenbench::black_box(v)
            })
        });

        // Known allocation: Vec::with_capacity(100) = 1 alloc of 400 bytes (100 × 4)
        group.bench("vec_100", |b| {
            b.iter_deferred_drop(|| {
                let mut v = Vec::with_capacity(100);
                for i in 0..100u32 {
                    v.push(zenbench::black_box(i));
                }
                v
            })
        });

        // Larger allocation
        group.bench("vec_10000", |b| {
            b.iter_deferred_drop(|| {
                let v: Vec<u32> = (0..10000).collect();
                zenbench::black_box(v)
            })
        });
    });

    // Verify alloc stats are present
    // (printed via terminal report and available in JSON output)
});
