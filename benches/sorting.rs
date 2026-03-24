use std::collections::HashMap;
use std::sync::Mutex;
use zenbench::Throughput;
use zenbench::black_box;

zenbench::main!(|suite| {
    // Group 1: sort algorithms with subgroups by input pattern
    suite.compare("sort_1000", |group| {
        group.throughput(Throughput::Elements(1000));
        group.throughput_unit("items");

        group.subgroup("reversed");
        group.bench("std_sort", |b| {
            b.with_input(|| (0..1000).rev().collect::<Vec<i32>>())
                .run(|mut v| {
                    v.sort();
                    black_box(v)
                })
        });
        group.bench("sort_unstable", |b| {
            b.with_input(|| (0..1000).rev().collect::<Vec<i32>>())
                .run(|mut v| {
                    v.sort_unstable();
                    black_box(v)
                })
        });

        group.subgroup("already sorted");
        group.bench("std_sort_sorted", |b| {
            b.with_input(|| (0..1000).collect::<Vec<i32>>())
                .run(|mut v| {
                    v.sort();
                    black_box(v)
                })
        });
        group.bench("unstable_sorted", |b| {
            b.with_input(|| (0..1000).collect::<Vec<i32>>())
                .run(|mut v| {
                    v.sort_unstable();
                    black_box(v)
                })
        });
    });

    // Group 2: sub-ns operations
    suite.compare("sub_ns_ops", |group| {
        group.config().expect_sub_ns(true);
        group.bench("black_box_unit", |b| b.iter(|| black_box(())));
        group.bench("black_box_add", |b| {
            let x = black_box(41u64);
            b.iter(|| black_box(x + 1))
        });
        group.bench("black_box_bool", |b| {
            let x = black_box(42u64);
            b.iter(|| black_box(x > 0))
        });
    });

    // Group 3: contended data structures
    suite.compare("contention", |group| {
        for &threads in &[1, 2, 4] {
            let label = format!("mutex_map_{threads}t");
            group.bench_contended(
                label,
                threads,
                || Mutex::new(HashMap::<u64, u64>::new()),
                |b, shared, tid| {
                    b.iter(|| {
                        shared.lock().unwrap().insert(tid as u64, black_box(42));
                    })
                },
            );
        }
    });

    // Group 4: element processing — throughput with contention
    suite.compare("process_elements", |group| {
        group.throughput(Throughput::Elements(10_000));
        group.throughput_unit("items");

        // Single-threaded baseline: sum 10K integers
        group.subgroup("single-threaded");
        group.bench("sum_sequential", |b| {
            b.with_input(|| (0..10_000i64).collect::<Vec<_>>())
                .run(|v| black_box(v.iter().sum::<i64>()))
        });

        // Chunked map: apply a transform to each element
        group.bench("map_sqrt", |b| {
            b.with_input(|| (1..=10_000).map(|i| i as f64).collect::<Vec<_>>())
                .run(|v| {
                    let out: Vec<f64> = v.iter().map(|x| x.sqrt()).collect();
                    black_box(out)
                })
        });

        // Contended: 4 threads processing a shared work queue
        group.subgroup("contended (4 threads)");
        group.bench_contended(
            "shared_vec_push",
            4,
            || Mutex::new(Vec::<i64>::with_capacity(10_000)),
            |b, shared, tid| {
                let start = (tid * 2500) as i64;
                b.iter(|| {
                    for i in start..start + 2500 {
                        shared.lock().unwrap().push(black_box(i));
                    }
                })
            },
        );
    });

    // Group 5: sort sizes
    suite.compare("sort_sizes", |group| {
        for &size in &[10, 100, 1000, 10_000] {
            let label = format!("unstable_{size}");
            group.bench(label, move |b| {
                b.with_input(move || (0..size).rev().collect::<Vec<i32>>())
                    .run(|mut v| {
                        v.sort_unstable();
                        black_box(v)
                    })
            });
        }
    });
});
