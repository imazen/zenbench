//! Example: comparing sorting algorithms with interleaved execution.
//!
//! Run with: cargo run --example sorting --release

fn main() {
    let result = zenbench::run(|suite| {
        // Compare different sorting approaches on the same data
        suite.compare("sort_1000_reversed", |group| {
            group.config().max_rounds(100);

            group.bench("std_sort", |b| {
                b.with_input(|| (0..1000).rev().collect::<Vec<i32>>())
                    .run(|mut v| {
                        v.sort();
                        v
                    })
            });

            group.bench("sort_unstable", |b| {
                b.with_input(|| (0..1000).rev().collect::<Vec<i32>>())
                    .run(|mut v| {
                        v.sort_unstable();
                        v
                    })
            });
        });

        // Compare on random-ish data
        suite.compare("sort_1000_shuffled", |group| {
            group.config().max_rounds(100);

            group.bench("std_sort", |b| {
                b.with_input(|| {
                    // Simple deterministic pseudo-random permutation
                    let mut v: Vec<i32> = (0..1000).collect();
                    let n = v.len();
                    for i in (1..n).rev() {
                        let j = (i * 2654435761) % (i + 1); // Knuth multiplicative hash
                        v.swap(i, j);
                    }
                    v
                })
                .run(|mut v| {
                    v.sort();
                    v
                })
            });

            group.bench("sort_unstable", |b| {
                b.with_input(|| {
                    let mut v: Vec<i32> = (0..1000).collect();
                    let n = v.len();
                    for i in (1..n).rev() {
                        let j = (i * 2654435761) % (i + 1);
                        v.swap(i, j);
                    }
                    v
                })
                .run(|mut v| {
                    v.sort_unstable();
                    v
                })
            });
        });

        // Standalone benchmark
        suite.bench("vec_allocation", |b| {
            b.iter(|| {
                let v: Vec<u8> = vec![0u8; 4096];
                zenbench::black_box(v)
            });
        });
    });

    // Save results
    if let Err(e) = result.save("sorting_results.json") {
        eprintln!("Failed to save results: {e}");
    }
}
