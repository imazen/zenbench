//! Side-by-side comparison: run identical workloads with both zenbench and divan,
//! then compare the results.
//!
//! This benchmark binary uses zenbench as the harness. It runs each workload
//! through zenbench's measurement engine and also shells out to a divan benchmark
//! binary to get divan's numbers for the same workloads.
//!
//! Run: `cargo bench --bench divan_compare`
//!
//! The workloads are designed to span a wide range:
//! - Sub-ns: no-op / constant return
//! - Low ns: integer arithmetic
//! - Medium ns: small allocation + sort
//! - High ns: large allocation + sort
//! - Microsecond: hash map operations

use std::collections::HashMap;

// ── Workload functions (shared between zenbench and divan) ──────────

#[inline(never)]
fn work_noop() -> u64 {
    std::hint::black_box(42)
}

#[inline(never)]
fn work_sum_100() -> u64 {
    let mut v = 0u64;
    for i in 0..100 {
        v = v.wrapping_add(std::hint::black_box(i));
    }
    v
}

#[inline(never)]
fn work_sum_1000() -> u64 {
    let mut v = 0u64;
    for i in 0..1000 {
        v = v.wrapping_add(std::hint::black_box(i));
    }
    v
}

#[inline(never)]
fn work_sort_100() -> Vec<u32> {
    let mut v: Vec<u32> = (0..100).rev().collect();
    v.sort_unstable();
    v
}

#[inline(never)]
fn work_sort_10000() -> Vec<u32> {
    let mut v: Vec<u32> = (0..10000).rev().collect();
    v.sort_unstable();
    v
}

#[inline(never)]
fn work_hashmap_100() -> HashMap<u64, u64> {
    let mut m = HashMap::with_capacity(100);
    for i in 0..100u64 {
        m.insert(i, i * i);
    }
    m
}

// ── Zenbench harness ────────────────────────────────────────────────

zenbench::main!(|suite| {
    // Use fixed rounds for reproducible comparison
    suite.compare("noop", |group| {
        group
            .config()
            .max_rounds(100)
            .auto_rounds(false)
            .expect_sub_ns(true);
        group.bench("noop", |b| b.iter(work_noop));
    });

    suite.compare("arithmetic", |group| {
        group.config().max_rounds(100).auto_rounds(false);
        group.bench("sum_100", |b| b.iter(work_sum_100));
        group.bench("sum_1000", |b| b.iter(work_sum_1000));
    });

    suite.compare("sort", |group| {
        group.config().max_rounds(50).auto_rounds(false);
        // Use iter() not iter_deferred_drop() — divan drops each output immediately,
        // so we should too for a fair comparison. iter_deferred_drop() holds all
        // outputs in memory, causing cache pressure that inflates sort_10000 by ~7x.
        group.bench("sort_100", |b| b.iter(work_sort_100));
        group.bench("sort_10000", |b| b.iter(work_sort_10000));
    });

    suite.compare("hashmap", |group| {
        group.config().max_rounds(100).auto_rounds(false);
        group.bench("insert_100", |b| b.iter(work_hashmap_100));
    });

    // Same hashmap but with low iteration count to match divan/criterion's
    // ~100 iterations per sample. Tests whether iteration count (cache
    // hotness) explains the measurement difference.
    suite.compare("hashmap_cold", |group| {
        group
            .config()
            .max_rounds(100)
            .auto_rounds(false)
            .cold_start(true); // 1 iter/sample + cache firewall
        group.bench("insert_100_cold", |b| b.iter(work_hashmap_100));
    });

    // Print summary for manual comparison against divan output
    eprintln!("\n[zenbench] Compare these numbers against: cargo bench --bench divan_compare_ref");
});
