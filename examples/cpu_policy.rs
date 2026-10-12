//! Verify confinement of an existing private Rayon pool and ordinary threads.
//! Run inside an enabled, single-class cpuset; `taskset` alone is insufficient.
#[cfg(any(target_os = "linux", target_os = "windows"))]
use std::time::Duration;
#[cfg(any(target_os = "linux", target_os = "windows"))]
use zenbench::platform::{PlacementStatus, check_cpu_placement, current_thread_affinity};

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // This pool exists before the engine, and explicitly ignores the launcher's
    // suggested Rayon size. Placement must still hold for every worker.
    let pool = rayon::ThreadPoolBuilder::new().num_threads(2).build()?;
    let proof = check_cpu_placement()?;
    assert_eq!(proof.status, PlacementStatus::EnforcedUniform);
    let all_cpus: Vec<_> = proof
        .topology
        .classes
        .iter()
        .flat_map(|c| c.cpus.iter().copied())
        .collect();
    let check = || {
        // Linux filters this request to the cpuset; Windows may reject it.
        let _attempt = affinity::set_thread_affinity(&all_cpus);
        let mask = current_thread_affinity().expect("worker mask");
        assert!(!mask.is_empty());
        assert!(
            mask.iter().all(|cpu| proof.effective_cpus.contains(cpu)),
            "worker escaped verified process set: {mask:?}"
        );
        mask
    };
    let masks = pool.broadcast(|_| check());
    let ordinary = std::thread::scope(|scope| scope.spawn(check).join().unwrap());
    println!("private Rayon masks: {masks:?}; ordinary thread: {ordinary:?}");
    println!("verified: {}", serde_json::to_string(&proof)?);
    let result = zenbench::run_gated(zenbench::GateConfig::disabled(), |suite| {
        suite.group("external-pool", |g| {
            g.config()
                .max_rounds(5)
                .auto_rounds(false)
                .warmup_time(Duration::ZERO)
                .sample_target_ns(10_000)
                .min_sample_ns(0);
            g.bench("private-rayon", move |b| {
                b.iter(|| {
                    pool.install(|| {
                        rayon::join(|| std::hint::black_box(1), || std::hint::black_box(2))
                    })
                })
            });
        });
    });
    print!("{}", result.to_llm());
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    Err("this process-boundary probe requires Linux or Windows".into())
}
