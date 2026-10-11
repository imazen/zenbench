//! Native repeatability controls. Each invocation is a fresh process.
//! Pass a comma-separated CPU set, or `default` for the refusal control.
#[cfg(target_os = "windows")]
use std::hint::black_box;
#[cfg(target_os = "windows")]
use std::sync::Arc;
#[cfg(target_os = "windows")]
use std::time::{Duration, Instant};
#[cfg(target_os = "windows")]
use zenbench::platform::{CpuPolicy, current_thread_affinity};

#[cfg(target_os = "windows")]
#[inline(never)]
fn integer_work(n: usize) -> u64 {
    let mut x = black_box(0x9e37_79b9_7f4a_7c15_u64);
    for i in 0..n {
        x = x.rotate_left(13).wrapping_mul(0xd6e8_feb8_6659_fd93) ^ i as u64;
    }
    black_box(x)
}

#[cfg(target_os = "windows")]
#[inline(never)]
fn cache_work(data: &[u64], n: usize) -> u64 {
    let mut x = black_box(17_usize);
    for _ in 0..n {
        x = data[x & (data.len() - 1)] as usize;
    }
    black_box(x as u64)
}

#[cfg(target_os = "windows")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arg = std::env::args().nth(1).ok_or("supply CPUs or default")?;
    let policy = match arg.as_str() {
        "performance" => CpuPolicy::Performance,
        "efficiency" => CpuPolicy::Efficiency,
        _ => CpuPolicy::Auto,
    };
    let cpus: Option<Vec<usize>> =
        if matches!(arg.as_str(), "default" | "performance" | "efficiency") {
            None
        } else {
            Some(arg.split(',').map(str::parse).collect::<Result<_, _>>()?)
        };
    let initial_process = affinity::get_process_affinity()?;
    // All workers exist before zenbench establishes its process boundary.
    let pool = Arc::new(rayon::ThreadPoolBuilder::new().num_threads(2).build()?);
    let initial_workers = pool.broadcast(|_| current_thread_affinity().unwrap());
    let data: Arc<Vec<u64>> = Arc::new(
        (0..32768_u64)
            .map(|i| (i.wrapping_mul(1664525).wrapping_add(1013904223)) & 32767)
            .collect(),
    );
    let proof = Arc::new(std::sync::Mutex::new(Vec::new()));
    let process_proof = Arc::new(std::sync::Mutex::new(Vec::new()));
    let reference = Arc::new(std::sync::Mutex::new(Vec::new()));
    let result = zenbench::run_gated(zenbench::GateConfig::disabled(), |suite| {
        suite.cpu_policy(policy);
        if let Some(cpus) = &cpus {
            suite.cpu_set(cpus);
        }
        for kind in ["integer", "cache", "rayon"] {
            suite.group(kind, |g| {
                g.config()
                    .max_rounds(200)
                    .auto_rounds(false)
                    .warmup_time(Duration::from_millis(200))
                    .max_wall_time(Duration::from_secs(20));
                for (name, multiplier) in [
                    ("identical-a", 1000),
                    ("identical-b", 1000),
                    ("ten-percent-more", 1100),
                    ("double-work", 2000),
                ] {
                    let pool = pool.clone();
                    let data = data.clone();
                    let cpus = cpus.clone();
                    let proof = proof.clone();
                    let process_proof = process_proof.clone();
                    let reference = reference.clone();
                    let initial_process = initial_process.clone();
                    let mut initialized = false;
                    g.bench(name, move |b| {
                        if !initialized {
                            let expected = cpus
                                .clone()
                                .unwrap_or_else(|| affinity::get_process_affinity().unwrap());
                            let mutation = affinity::set_process_affinity(&initial_process);
                            let actual = affinity::get_process_affinity().unwrap();
                            assert_eq!(
                                actual, expected,
                                "benchmark code widened the process CPU boundary"
                            );
                            process_proof.lock().unwrap().push(serde_json::json!({
                                "requested_cpus": initial_process,
                                "request_result": mutation.map_err(|e| e.to_string()),
                                "actual_cpus": actual,
                            }));
                            let masks = pool.broadcast(|_| {
                                // Deliberately try widening after process restriction.
                                let _ = affinity::set_thread_affinity(&initial_process);
                                let mask = current_thread_affinity().unwrap();
                                assert!(
                                    !mask.is_empty() && mask.iter().all(|c| expected.contains(c)),
                                    "Rayon worker escaped: {mask:?}"
                                );
                                let start = Instant::now();
                                while start.elapsed() < Duration::from_millis(100) {
                                    black_box(integer_work(4096));
                                }
                                mask
                            });
                            proof.lock().unwrap().push(masks);
                            if kind == "integer" && name == "identical-a" {
                                // Independent Windows QPC via Instant, without
                                // Bencher batching, compensation, or TSC conversion.
                                for _ in 0..16 {
                                    let start = Instant::now();
                                    for _ in 0..1000 {
                                        black_box(integer_work(4096));
                                    }
                                    reference
                                        .lock()
                                        .unwrap()
                                        .push(start.elapsed().as_nanos() as f64 / 1000.0);
                                }
                            }
                            initialized = true;
                        }
                        match kind {
                            "integer" => b.iter(|| integer_work(4096 * multiplier / 1000)),
                            "cache" => b.iter(|| cache_work(&data, 4096 * multiplier / 1000)),
                            "rayon" => b.iter(|| {
                                pool.install(|| {
                                    rayon::join(
                                        || integer_work(4096 * multiplier / 1000),
                                        || integer_work(4096 * multiplier / 1000),
                                    )
                                })
                            }),
                            _ => unreachable!(),
                        }
                    });
                }
            });
        }
    });
    let process_after = affinity::get_process_affinity()?;
    assert_eq!(
        process_after,
        result.cpu_placement.as_ref().unwrap().effective_cpus
    );
    println!(
        "{}",
        serde_json::json!({
            "precise_timing_enabled": cfg!(feature = "precise-timing"),
            "reference_integer_qpc_ns": *reference.lock().unwrap(),
            "initial_workers": initial_workers,
            "worker_masks": *proof.lock().unwrap(),
            "process_widen_attempts": *process_proof.lock().unwrap(),
            "process_after": process_after,
            "result": result,
        })
    );
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    Err("this native repeatability diagnostic requires Windows".into())
}
