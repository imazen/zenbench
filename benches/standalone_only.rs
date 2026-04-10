use zenbench::prelude::*;

fn bench_basic(suite: &mut Suite) {
    suite.bench("overhead", |b| b.iter(|| black_box(1)));
    suite.bench("overhead2", |b| b.iter(|| black_box(2)));
}

zenbench::main!(bench_basic);
