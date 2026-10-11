//! Observe the actual workers of a private Rayon pool, without certifying affinity.
use zenbench::platform::{CpuProbeConfig, CpuProbeReport, pin_current_thread, probe_cpu_thread};

fn main() {
    // Optional positional CPU IDs apply scoped hints/masks to these workers.
    // Inspect cpu-info first; CPU IDs do not encode a portable P/E class.
    let cpus = std::env::args()
        .skip(1)
        .map(|v| v.parse::<usize>().unwrap())
        .collect::<Vec<_>>();
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(if cpus.is_empty() { 2 } else { cpus.len() })
        .build()
        .unwrap();
    let observations = pool
        .broadcast(|context| {
            let _guard = cpus
                .get(context.index())
                .map(|&cpu| pin_current_thread(&[cpu]).map_err(|e| e.to_string()))
                .transpose()?;
            probe_cpu_thread(CpuProbeConfig::default())
        })
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let report = CpuProbeReport::from_workers(observations).unwrap();
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
}
