//! Development analyzer for external-worker rounds; same stats owner as engine.
// Module reuse shares the implementation without adding public library API.
#[allow(dead_code)]
#[path = "../src/stats.rs"]
mod stats;
use std::io::Read;

fn main() {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).unwrap();
    let packets: Vec<serde_json::Value> = serde_json::from_str(&input).unwrap();
    let result: Vec<_> = packets
        .iter()
        .map(|p| {
            let baseline: Vec<f64> = serde_json::from_value(p["baseline"].clone()).unwrap();
            let candidate: Vec<f64> = serde_json::from_value(p["candidate"].clone()).unwrap();
            let iterations: Vec<usize> = serde_json::from_value(p["iterations"].clone()).unwrap();
            stats::PairedAnalysis::compute_with_config(
                &baseline,
                &candidate,
                &iterations,
                10_000,
                0.0,
                p["timer_resolution_ns"].as_f64().unwrap(),
            )
            .expect("valid paired rounds")
        })
        .collect();
    println!("{}", serde_json::to_string(&result).unwrap());
}
