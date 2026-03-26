//! Test the criterion compatibility layer.
//!
//! These tests verify that criterion-style API patterns work unchanged
//! through the zenbench compat layer.
#![cfg(feature = "criterion-compat")]

use zenbench::criterion_compat::*;
use zenbench::criterion_group;

fn bench_simple(c: &mut Criterion) {
    c.bench_function("fib_add", |b| b.iter(|| black_box(42u64 + 1)));
}

fn bench_group(c: &mut Criterion) {
    let mut group = c.benchmark_group("sorting");
    group.throughput(Throughput::Elements(100));
    group.bench_function("std_sort", |b| {
        b.iter(|| {
            let mut v: Vec<i32> = (0..100).rev().collect();
            v.sort();
            black_box(v)
        })
    });
    group.bench_function("sort_unstable", |b| {
        b.iter(|| {
            let mut v: Vec<i32> = (0..100).rev().collect();
            v.sort_unstable();
            black_box(v)
        })
    });
    group.finish();
}

fn bench_with_input(c: &mut Criterion) {
    let data: Vec<i32> = (0..1000).collect();
    c.bench_with_input(BenchmarkId::new("sum", 1000), &data, |b, input| {
        b.iter(|| black_box(input.iter().sum::<i32>()))
    });
}

fn bench_iter_batched(c: &mut Criterion) {
    let mut group = c.benchmark_group("batched");
    group.bench_function("with_setup", |b| {
        b.iter_batched(
            || (0..100).rev().collect::<Vec<i32>>(),
            |mut v| {
                v.sort();
                black_box(v)
            },
            BatchSize::SmallInput,
        )
    });
    group.bench_function("with_setup_ref", |b| {
        b.iter_batched_ref(
            || (0..100).rev().collect::<Vec<i32>>(),
            |v| {
                v.sort();
                black_box(v.len())
            },
            BatchSize::LargeInput,
        )
    });
    group.finish();
}

// Use the criterion macros
criterion_group!(
    benches,
    bench_simple,
    bench_group,
    bench_with_input,
    bench_iter_batched
);

// Can't use criterion_main! in a test (it generates main()),
// so test the group function directly.
#[test]
fn criterion_compat_runs_without_panic() {
    let criterion = benches();
    let suite = criterion.into_suite();
    let result = zenbench::run_gated(zenbench::GateConfig::disabled(), |s| {
        s.merge(suite);
        // Set low rounds for fast test
        // (can't set per-group config through compat layer easily)
    });
    // Should have run all groups
    assert!(
        result.comparisons.len() >= 3,
        "expected >= 3 groups, got {}",
        result.comparisons.len(),
    );
}

#[test]
fn benchmark_id_formatting() {
    let id = BenchmarkId::new("sort", 1000);
    assert_eq!(format!("{id}"), "sort/1000");

    let id2 = BenchmarkId::from_parameter("large");
    assert_eq!(format!("{id2}"), "large");
}

#[test]
fn batch_size_variants_exist() {
    // Just verify the enum variants compile
    let _ = BatchSize::SmallInput;
    let _ = BatchSize::LargeInput;
    let _ = BatchSize::PerIteration;
    let _ = BatchSize::NumBatches(10);
    let _ = BatchSize::NumIterations(1000);
}
