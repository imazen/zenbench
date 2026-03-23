use zenbench::Throughput;
use zenbench::black_box;

zenbench::main!(|suite| {
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
});
