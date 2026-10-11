//! A tiny scoped-affinity benchmark with an OS mask check.
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use zenbench::platform::{current_thread_affinity, pin_current_thread};
use zenbench::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let before = current_thread_affinity()?;
    let core = before[0]; // Works under taskset, containers, and restricted CPU sets.
    println!("before: {before:?}");
    {
        let _pin = pin_current_thread(&[core])?;
        println!("guard: {:?}", current_thread_affinity()?);
    }
    assert_eq!(current_thread_affinity()?, before);

    zenbench::run_gated(GateConfig::disabled(), |suite| {
        suite.group("pinned", |g| {
            g.affinity(&[core]);
            g.config()
                .max_rounds(5)
                .auto_rounds(false)
                .warmup_time(Duration::from_millis(2))
                .sample_target_ns(10_000)
                .min_sample_ns(0);
            let printed = AtomicBool::new(false);
            g.bench("multiply", move |b| {
                if !printed.swap(true, Ordering::Relaxed) {
                    let applied = current_thread_affinity().expect("query benchmark affinity");
                    println!("benchmark: {applied:?}");
                    assert_eq!(applied, [core]);
                }
                b.iter(|| black_box(42u64).wrapping_mul(black_box(7)));
            });
        });
    });
    let after = current_thread_affinity()?;
    println!("restored: {after:?}");
    assert_eq!(after, before);
    Ok(())
}
