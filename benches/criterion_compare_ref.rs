//! Criterion reference benchmark — identical workloads to divan_compare.rs.
//!
//! Run: `cargo bench --bench criterion_compare_ref`
//!
//! Compare output against:
//!   `cargo bench --bench divan_compare`        (zenbench)
//!   `cargo bench --bench divan_compare_ref`    (divan)

use criterion::{Criterion, criterion_group, criterion_main};
use std::collections::HashMap;

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

fn bench_all(c: &mut Criterion) {
    c.bench_function("noop", |b| b.iter(work_noop));
    c.bench_function("sum_100", |b| b.iter(work_sum_100));
    c.bench_function("sum_1000", |b| b.iter(work_sum_1000));
    c.bench_function("sort_100", |b| b.iter(work_sort_100));
    c.bench_function("sort_10000", |b| b.iter(work_sort_10000));
    c.bench_function("hashmap_insert_100", |b| b.iter(work_hashmap_100));
}

criterion_group!(benches, bench_all);
criterion_main!(benches);
