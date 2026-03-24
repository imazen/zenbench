//! Divan reference benchmark — identical workloads to divan_compare.rs.
//!
//! Run: `cargo bench --bench divan_compare_ref`
//!
//! Compare output against: `cargo bench --bench divan_compare`

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

fn main() {
    divan::main();
}

#[divan::bench]
fn noop() -> u64 {
    work_noop()
}

#[divan::bench]
fn sum_100() -> u64 {
    work_sum_100()
}

#[divan::bench]
fn sum_1000() -> u64 {
    work_sum_1000()
}

#[divan::bench]
fn sort_100() -> Vec<u32> {
    work_sort_100()
}

#[divan::bench]
fn sort_10000() -> Vec<u32> {
    work_sort_10000()
}

#[divan::bench]
fn hashmap_insert_100() -> HashMap<u64, u64> {
    work_hashmap_100()
}
