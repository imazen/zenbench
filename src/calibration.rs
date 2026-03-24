//! Built-in calibration workloads for cross-machine normalization.
//!
//! Runs a small set of known workloads at engine startup to characterize
//! the hardware. Results are stored in `SuiteResult.calibration` and can
//! be used to normalize benchmark times across different machines.
//!
//! Calibration takes ~50ms and runs before any user benchmarks. Opt out
//! with `ZENBENCH_NO_CALIBRATE=1`.

use serde::{Deserialize, Serialize};

/// Hardware calibration results from built-in workloads.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Calibration {
    /// Integer throughput: ns per iteration of a tight wrapping_add loop.
    /// Lower = faster CPU. Typical: 0.3–1.0 ns/iter.
    pub integer_ns: f64,
    /// Memory bandwidth: GiB/s for sequential 1 MiB read.
    /// Higher = faster memory subsystem. Typical: 10–50 GiB/s.
    pub memory_bw_gibps: f64,
    /// Memory latency: ns per pointer-chase step through 4 MiB buffer.
    /// Lower = faster caches/memory. Typical: 3–15 ns.
    pub memory_lat_ns: f64,
}

/// Run all calibration workloads. Takes ~50ms.
pub fn run_calibration() -> Calibration {
    let integer_ns = calibrate_integer();
    let memory_bw_gibps = calibrate_memory_bandwidth();
    let memory_lat_ns = calibrate_memory_latency();

    Calibration {
        integer_ns,
        memory_bw_gibps,
        memory_lat_ns,
    }
}

/// Tight integer loop: measures raw ALU throughput.
fn calibrate_integer() -> f64 {
    let iters = 10_000_000u64;
    let mut best_ns = f64::MAX;

    for _ in 0..5 {
        let start = std::time::Instant::now();
        let mut v = 0u64;
        for i in 0..iters {
            v = v.wrapping_add(std::hint::black_box(i));
        }
        std::hint::black_box(v);
        let elapsed = start.elapsed().as_nanos() as f64;
        let per_iter = elapsed / iters as f64;
        if per_iter < best_ns {
            best_ns = per_iter;
        }
    }

    best_ns
}

/// Sequential memory read: measures memory bandwidth.
fn calibrate_memory_bandwidth() -> f64 {
    let size = 1024 * 1024; // 1 MiB
    let buf: Vec<u64> = vec![1u64; size / 8];
    let mut best_gibps = 0.0_f64;

    for _ in 0..5 {
        let start = std::time::Instant::now();
        let mut sum = 0u64;
        for &val in &buf {
            sum = sum.wrapping_add(val);
        }
        std::hint::black_box(sum);
        let elapsed_s = start.elapsed().as_secs_f64();
        let gibps = (size as f64) / elapsed_s / (1024.0 * 1024.0 * 1024.0);
        if gibps > best_gibps {
            best_gibps = gibps;
        }
    }

    best_gibps
}

/// Pointer-chase through 4 MiB buffer: measures memory latency.
fn calibrate_memory_latency() -> f64 {
    let n_elements = 4 * 1024 * 1024 / 8; // 4 MiB of u64s = 512K elements
    let mut chain: Vec<usize> = (0..n_elements).collect();

    // Create a pseudo-random permutation for pointer chasing.
    // This defeats hardware prefetchers.
    let mut rng_state = 0xDEAD_BEEF_CAFE_BABEu64;
    for i in (1..n_elements).rev() {
        // Simple LCG for shuffling
        rng_state = rng_state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let j = (rng_state >> 33) as usize % (i + 1);
        chain.swap(i, j);
    }

    let steps = 1_000_000;
    let mut best_ns = f64::MAX;

    for _ in 0..3 {
        let start = std::time::Instant::now();
        let mut idx = 0usize;
        for _ in 0..steps {
            idx = chain[idx % n_elements];
        }
        std::hint::black_box(idx);
        let elapsed_ns = start.elapsed().as_nanos() as f64;
        let per_step = elapsed_ns / steps as f64;
        if per_step < best_ns {
            best_ns = per_step;
        }
    }

    best_ns
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calibration_values_are_sane() {
        let cal = run_calibration();
        assert!(
            cal.integer_ns > 0.0 && cal.integer_ns < 100.0,
            "integer_ns should be 0-100, got {}",
            cal.integer_ns
        );
        assert!(
            cal.memory_bw_gibps > 0.1 && cal.memory_bw_gibps < 200.0,
            "memory_bw should be 0.1-200 GiB/s, got {}",
            cal.memory_bw_gibps
        );
        assert!(
            cal.memory_lat_ns > 0.1 && cal.memory_lat_ns < 500.0,
            "memory_lat should be 0.1-500 ns, got {}",
            cal.memory_lat_ns
        );
    }

    #[test]
    fn calibration_is_fast() {
        let start = std::time::Instant::now();
        let _cal = run_calibration();
        assert!(
            start.elapsed() < std::time::Duration::from_secs(5),
            "calibration should complete in < 5s"
        );
    }
}
