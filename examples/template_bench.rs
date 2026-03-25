//! Benchmark template — copy this to your crate's benches/ directory.
//!
//! In Cargo.toml:
//! ```toml
//! [dev-dependencies]
//! zenbench = { version = "0.1" }
//!
//! [[bench]]
//! name = "my_bench"
//! harness = false
//! ```
//!
//! Run: cargo bench --bench my_bench
//! Save baseline: cargo bench --bench my_bench -- --save-baseline=main
//! Check regression: cargo bench --bench my_bench -- --baseline=main

use zenbench::prelude::*;

fn bench_basics(suite: &mut Suite) {
    suite.group("example", |g| {
        g.throughput(Throughput::Elements(1000));

        g.bench("fast_path", |b| {
            b.iter(|| {
                let mut v = 0u64;
                for i in 0..1000 {
                    v = v.wrapping_add(black_box(i));
                }
                black_box(v)
            })
        });

        g.bench("slow_path", |b| {
            b.iter(|| {
                let mut v = 0u64;
                for i in 0..1000 {
                    v = v.wrapping_add(black_box(i * i));
                }
                black_box(v)
            })
        });
    });
}

fn bench_with_setup(suite: &mut Suite) {
    suite.group("sort", |g| {
        g.throughput(Throughput::Elements(10_000));

        for &size in &[100, 1000, 10_000] {
            g.bench(format!("sort_{size}"), move |b| {
                b.with_input(|| (0..size).rev().collect::<Vec<u32>>())
                    .run(|mut v| { v.sort_unstable(); v })
            });
        }
    });
}

zenbench::main!(bench_basics, bench_with_setup);
