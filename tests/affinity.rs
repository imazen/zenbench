//! Affinity applies to all benchmark phases and zenbench-owned worker threads.
#![cfg(any(target_os = "linux", target_os = "windows"))]

use std::sync::{Arc, Mutex};
use std::time::Duration;
use zenbench::platform::current_thread_affinity;
use zenbench::{GateConfig, run_gated};

#[test]
fn group_pins_workers_and_restores_between_groups() {
    let before = current_thread_affinity().unwrap();
    let core = before[0];
    let masks = Arc::new(Mutex::new(Vec::new()));
    let records = masks.clone();
    let result = run_gated(GateConfig::disabled(), |suite| {
        suite.group("pinned", |g| {
            g.affinity(&[core]);
            g.config()
                .max_rounds(5)
                .auto_rounds(false)
                .warmup_time(Duration::from_millis(2))
                .sample_target_ns(10_000)
                .min_sample_ns(0);
            let records = records.clone();
            g.bench("single", move |b| {
                records
                    .lock()
                    .unwrap()
                    .push(("single", current_thread_affinity().unwrap()));
                b.iter(|| std::hint::black_box(42));
            });
            let records = masks.clone();
            g.bench_parallel("parallel", 2, move |b, _| {
                records
                    .lock()
                    .unwrap()
                    .push(("parallel", current_thread_affinity().unwrap()));
                b.iter(|| std::hint::black_box(42));
            });
            let records = masks.clone();
            g.bench_contended(
                "contended",
                2,
                || Mutex::new(0),
                move |b, shared, _| {
                    records
                        .lock()
                        .unwrap()
                        .push(("contended", current_thread_affinity().unwrap()));
                    b.iter(|| {
                        *shared.lock().unwrap() += 1;
                    });
                },
            );
        });
        let before = before.clone();
        suite.group("restored", |g| {
            g.affinity(&before);
            g.config()
                .max_rounds(5)
                .auto_rounds(false)
                .warmup_time(Duration::ZERO)
                .sample_target_ns(10_000)
                .min_sample_ns(0);
            g.bench("next", move |b| {
                assert_eq!(current_thread_affinity().unwrap(), before);
                b.iter(|| std::hint::black_box(7));
            });
        });
    });
    assert_eq!(result.comparisons.len(), 2);
    assert_eq!(current_thread_affinity().unwrap(), before);
    let masks = masks.lock().unwrap();
    for kind in ["single", "parallel", "contended"] {
        // Warmup + estimation + measured rounds all invoke the same pinned scope.
        let observations: Vec<_> = masks.iter().filter(|(name, _)| *name == kind).collect();
        assert!(
            observations.len() > 5,
            "missing warmup/estimation calls for {kind}"
        );
        assert!(
            observations.iter().all(|(_, mask)| mask == &[core]),
            "{kind} escaped affinity: {observations:?}"
        );
    }
}

#[test]
fn invalid_group_aborts_before_closure_and_restores() {
    let before = current_thread_affinity().unwrap();
    let result = std::panic::catch_unwind(|| {
        run_gated(GateConfig::disabled(), |suite| {
            suite.group("invalid", |g| {
                g.affinity(&[usize::MAX]);
                g.bench("never", |_| {
                    panic!("benchmark ran despite invalid affinity")
                });
            });
        });
    });
    let panic = result.unwrap_err();
    let message = panic.downcast_ref::<String>().unwrap();
    assert!(message.contains("affinity"));
    assert!(!message.contains("benchmark ran"));
    assert_eq!(current_thread_affinity().unwrap(), before);
}
