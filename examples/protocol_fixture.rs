//! Known-work controls for the fixed-process protocol; no algorithm speed claim.
use std::hint::black_box;
use zenbench::Suite;

fn groups(suite: &mut Suite) {
    suite.plan().workload_id = Some("fold-1024-positive-control-v1".into());
    suite.group("positive-control", |g| {
        for (name, repetitions) in [("fold_once", 1), ("fold_four_times", 4)] {
            let data: Vec<u64> = (0..1024).map(|x| x * 17).collect();
            let stall = std::env::var_os("PROTOCOL_STALL").is_some();
            g.bench(name, move |b| {
                if stall {
                    std::thread::sleep(std::time::Duration::from_secs(10));
                }
                b.iter(|| {
                    let mut checksum = 0;
                    for _ in 0..repetitions {
                        checksum ^= black_box(&data)
                            .iter()
                            .fold(0u64, |a, v| a.wrapping_add(*v).rotate_left(7));
                    }
                    black_box(checksum)
                })
            });
        }
    });
}

zenbench::main!(groups);
