//! Observe the actual workers of a private Rayon pool, without certifying affinity.
use zenbench::platform::{CpuProbeConfig, CpuProbeReport, probe_cpu_thread};

fn main() {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(2)
        .build()
        .unwrap();
    let observations = pool
        .broadcast(|_| probe_cpu_thread(CpuProbeConfig::default()))
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let report = CpuProbeReport::from_workers(observations).unwrap();
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
}
