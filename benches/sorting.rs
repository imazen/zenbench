use zenbench::Throughput;
use zenbench::black_box;

zenbench::main!(|suite| {
    // Group 1: realistic sort comparison with throughput
    suite.compare("sort_1000_reversed", |group| {
        group.throughput(Throughput::Elements(1000));
        group.throughput_unit("items");
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
    });

    // Group 2: sub-ns operations — tests auto-scaling and warning heuristics
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

    // Group 3: sort sizes — wider spread, more benchmarks, shows baseline-only auto
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
