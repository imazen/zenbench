//! Fixed-work observations on the calling worker, independent of CPU admission.
//! A probe can expose timing variation; it never certifies a CPU class or pool.

use serde::{Deserialize, Serialize};
use std::time::Instant;

/// Configuration for a short, repeatable worker diagnostic.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CpuProbeConfig {
    /// Separate compute warmup before collecting samples, in 50..=1000 ms.
    /// It encourages sustained demand; it does not certify maximum frequency.
    #[serde(default)]
    pub warmup_ms: u64,
    /// Number of measured batches for each workload, in 3..=128.
    pub samples: usize,
    /// Identical work per batch on every worker, in 1_024..=1_048_576.
    pub iterations: u32,
}

impl Default for CpuProbeConfig {
    fn default() -> Self {
        Self {
            warmup_ms: 100,
            samples: 12,
            iterations: 32_768,
        }
    }
}

impl CpuProbeConfig {
    fn validate(self) -> Result<(), String> {
        if !(50..=1000).contains(&self.warmup_ms)
            || !(3..=128).contains(&self.samples)
            || !(1_024..=1_048_576).contains(&self.iterations)
        {
            return Err("CPU probe requires 50..=1000 ms warmup, 3..=128 samples and 1024..=1048576 iterations".into());
        }
        Ok(())
    }
}

/// One fixed-work batch. CPU IDs are observations at batch boundaries only.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CpuProbeSample {
    pub integer_ns_per_iteration: f64,
    pub cache_ns_per_iteration: f64,
    /// Linux's last executed CPU before the batch; unavailable elsewhere.
    pub cpu_before: Option<usize>,
    /// Linux's last executed CPU after the batch. Intermediate migration is unknown.
    pub cpu_after: Option<usize>,
}

/// Observations made by one actual worker, with its original scheduling policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CpuProbeThread {
    pub config: CpuProbeConfig,
    /// Actual time spent in the separate compute warmup, outside sample timers.
    /// Zero in historical records means no recorded warmup observation.
    #[serde(default)]
    pub warmup_elapsed_ns: u64,
    /// Requested macOS QoS readbacks, not physical core-class observations.
    #[serde(default)]
    pub qos_before: Option<super::ThreadQosState>,
    #[serde(default)]
    pub qos_after: Option<super::ThreadQosState>,
    /// The calling thread's diagnostic Rust ID, not an OS CPU or thread index.
    pub thread_id: String,
    /// Read errors are retained; missing masks never become placement evidence.
    pub affinity_before: Result<Vec<usize>, String>,
    pub affinity_after: Result<Vec<usize>, String>,
    pub samples: Vec<CpuProbeSample>,
}

/// Distribution of a workload's fixed-work times on one worker.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CpuProbeSpread {
    pub min_ns: f64,
    pub median_ns: f64,
    pub max_ns: f64,
    /// Scaled MAD, using the same definition as benchmark summaries.
    pub mad_ns: f64,
    pub slowest_over_fastest: f64,
}

impl CpuProbeSpread {
    fn from_values(values: &[f64]) -> Self {
        let summary = crate::stats::Summary::from_slice(values);
        Self {
            min_ns: summary.min,
            median_ns: summary.median,
            max_ns: summary.max,
            mad_ns: summary.mad,
            slowest_over_fastest: summary.max / summary.min,
        }
    }
}

/// Observational report from workers measured with identical probe configuration.
///
/// Ratios describe variation, without attributing it to P/E cores, migration,
/// SMT contention, DVFS, thermal effects, or unrelated load. A small ratio does
/// not prove uniform cores. No "passed" or certified placement status is issued.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CpuProbeReport {
    /// Always false: timing observations never certify placement.
    #[serde(skip_deserializing)]
    certifies_cpu_class: bool,
    pub workers: Vec<CpuProbeThread>,
    /// Integer and cache distributions, in worker order.
    pub integer: Vec<CpuProbeSpread>,
    pub cache: Vec<CpuProbeSpread>,
    pub integer_slowest_over_fastest_median: f64,
    pub cache_slowest_over_fastest_median: f64,
}

impl CpuProbeReport {
    /// Combine measurements from the actual workers being investigated.
    ///
    /// Callers control simultaneity and pool coverage. Do not combine serial and
    /// parallel measurements, or infer coverage of workers absent from this list.
    pub fn from_workers(workers: Vec<CpuProbeThread>) -> Result<Self, String> {
        let first = workers
            .first()
            .ok_or("CPU probe needs at least one worker")?;
        first.config.validate()?;
        let mut ids = std::collections::HashSet::new();
        for worker in &workers {
            if worker.config.samples != first.config.samples
                || worker.config.iterations != first.config.iterations
                || worker.config.warmup_ms != first.config.warmup_ms
                || worker.samples.len() != first.config.samples
            {
                return Err("CPU probe workers must use identical work and sample counts".into());
            }
            if worker.warmup_elapsed_ns < worker.config.warmup_ms * 1_000_000 {
                return Err("CPU probe worker lacks the requested separate warmup".into());
            }
            if !ids.insert(&worker.thread_id) {
                return Err("CPU probe workers must have distinct thread IDs".into());
            }
            if worker.samples.iter().any(|s| {
                !s.integer_ns_per_iteration.is_finite()
                    || s.integer_ns_per_iteration <= 0.0
                    || !s.cache_ns_per_iteration.is_finite()
                    || s.cache_ns_per_iteration <= 0.0
            }) {
                return Err("CPU probe times must be finite and positive".into());
            }
        }
        let integer: Vec<_> = workers
            .iter()
            .map(|worker| {
                CpuProbeSpread::from_values(
                    &worker
                        .samples
                        .iter()
                        .map(|s| s.integer_ns_per_iteration)
                        .collect::<Vec<_>>(),
                )
            })
            .collect();
        let cache: Vec<_> = workers
            .iter()
            .map(|worker| {
                CpuProbeSpread::from_values(
                    &worker
                        .samples
                        .iter()
                        .map(|s| s.cache_ns_per_iteration)
                        .collect::<Vec<_>>(),
                )
            })
            .collect();
        let ratio = |spreads: &[CpuProbeSpread]| {
            let min = spreads
                .iter()
                .map(|s| s.median_ns)
                .fold(f64::INFINITY, f64::min);
            let max = spreads.iter().map(|s| s.median_ns).fold(0.0, f64::max);
            max / min
        };
        Ok(Self {
            certifies_cpu_class: false,
            integer_slowest_over_fastest_median: ratio(&integer),
            cache_slowest_over_fastest_median: ratio(&cache),
            workers,
            integer,
            cache,
        })
    }
}

pub(crate) fn warmup_cpu_for(duration: std::time::Duration) -> std::time::Duration {
    let start = Instant::now();
    let mut value = 1u64;
    while start.elapsed() < duration {
        for i in 0..4096u64 {
            value = std::hint::black_box(value)
                .wrapping_mul(6364136223846793005)
                .rotate_left(7)
                ^ i;
        }
        std::hint::black_box(value);
    }
    start.elapsed()
}

/// Warm up, then measure dependent integer and 64 KiB pointer-chase workloads
/// on the calling thread. Does not change affinity or QoS, nor create a pool.
///
/// Invoke inside the pool being checked: for a private Rayon pool, use
/// `pool.broadcast(|_| probe_cpu_thread(config))` and combine those results
/// using [`crate::platform::CpuProbeReport::from_workers`]. Repeat between
/// measurement windows to observe later changes. Concurrent workers may contend
/// for cores or SMT resources; this is part of the observation, not a diagnosis.
///
/// This diagnostic neither bypasses [`crate::platform::CpuPolicy::Auto`] nor
/// upgrades macOS scheduling hints to enforced placement.
pub fn probe_cpu_thread(config: CpuProbeConfig) -> Result<CpuProbeThread, String> {
    config.validate()?;
    let mask = || super::current_thread_affinity().map_err(|e| e.to_string());
    let affinity_before = mask();
    let qos_before = super::current_thread_qos().ok();
    // A permutation cycle spanning 8192 elements; each worker has private data.
    let chain: Vec<u64> = (0..8192).map(|i| (i + 4051) % 8192).collect();
    let integer = || {
        let mut value = 1u64;
        for i in 0..config.iterations {
            value = std::hint::black_box(value)
                .wrapping_mul(6364136223846793005)
                .rotate_left(7)
                ^ u64::from(i);
        }
        std::hint::black_box(value);
    };
    let cache = || {
        let mut index = 0;
        for _ in 0..config.iterations {
            index = chain[std::hint::black_box(index)] as usize;
        }
        std::hint::black_box(index);
    };
    // Keep clock-demand warmup completely outside the measured batches.
    let warmup_duration = std::time::Duration::from_millis(config.warmup_ms);
    let warmup_elapsed_ns = warmup_cpu_for(warmup_duration)
        .as_nanos()
        .min(u128::from(u64::MAX)) as u64;
    // Seed workload data independently, still outside all sample timers.
    for _ in 0..2 {
        integer();
        cache();
    }
    let mut samples = Vec::with_capacity(config.samples);
    for round in 0..config.samples {
        let cpu_before = observed_cpu();
        let time = |work: &dyn Fn()| {
            let start = Instant::now();
            work();
            start.elapsed().as_nanos().max(1) as f64 / f64::from(config.iterations)
        };
        // Alternate order so startup/clock changes do not always favor one arm.
        let (integer_ns_per_iteration, cache_ns_per_iteration) = if round % 2 == 0 {
            (time(&integer), time(&cache))
        } else {
            let cache_ns = time(&cache);
            (time(&integer), cache_ns)
        };
        samples.push(CpuProbeSample {
            integer_ns_per_iteration,
            cache_ns_per_iteration,
            cpu_before,
            cpu_after: observed_cpu(),
        });
    }
    Ok(CpuProbeThread {
        config,
        warmup_elapsed_ns,
        qos_before,
        qos_after: super::current_thread_qos().ok(),
        thread_id: format!("{:?}", std::thread::current().id()),
        affinity_before,
        affinity_after: mask(),
        samples,
    })
}

#[cfg(target_os = "linux")]
fn observed_cpu() -> Option<usize> {
    parse_cpu(&std::fs::read_to_string("/proc/thread-self/stat").ok()?)
}

#[cfg(target_os = "linux")]
fn parse_cpu(stat: &str) -> Option<usize> {
    // Field 2 can contain spaces and ')' characters; field 3 follows the final
    // ')'. Linux stat's processor field is 39, hence offset 36 from field 3.
    stat.rsplit_once(')')?
        .1
        .split_whitespace()
        .nth(36)?
        .parse()
        .ok()
}

#[cfg(not(target_os = "linux"))]
fn observed_cpu() -> Option<usize> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn worker(id: &str, values: &[f64]) -> CpuProbeThread {
        let config = CpuProbeConfig {
            samples: values.len(),
            ..Default::default()
        };
        CpuProbeThread {
            config,
            warmup_elapsed_ns: config.warmup_ms * 1_000_000,
            qos_before: None,
            qos_after: None,
            thread_id: id.into(),
            affinity_before: Err("unavailable".into()),
            affinity_after: Err("unavailable".into()),
            samples: values
                .iter()
                .map(|&value| CpuProbeSample {
                    integer_ns_per_iteration: value,
                    cache_ns_per_iteration: value * 2.0,
                    cpu_before: None,
                    cpu_after: None,
                })
                .collect(),
        }
    }

    #[test]
    fn persistent_difference_and_transient_noise_are_separate_observations() {
        let report = CpuProbeReport::from_workers(vec![
            worker("fast", &[1.0, 1.0, 1.0]),
            worker("slow", &[2.0, 2.0, 2.0]),
            worker("spike", &[1.0, 1.0, 10.0]),
        ])
        .unwrap();
        assert_eq!(report.integer_slowest_over_fastest_median, 2.0);
        assert_eq!(report.cache_slowest_over_fastest_median, 2.0);
        assert_eq!(report.integer[2].slowest_over_fastest, 10.0);
        assert_eq!(report.integer[1].slowest_over_fastest, 1.0);
        assert_eq!(report.integer[2].mad_ns, 0.0);
        let mut json = serde_json::to_value(&report).unwrap();
        json["certifies_cpu_class"] = serde_json::json!(true);
        let decoded: CpuProbeReport = serde_json::from_value(json).unwrap();
        assert!(!decoded.certifies_cpu_class);
    }

    #[test]
    fn rejects_incomparable_or_invalid_evidence() {
        assert!(CpuProbeReport::from_workers(vec![]).is_err());
        assert!(CpuProbeReport::from_workers(vec![worker("a", &[1.0, 2.0])]).is_err());
        for invalid in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(CpuProbeReport::from_workers(vec![worker("a", &[1.0, invalid, 1.0])]).is_err());
        }
        let a = worker("a", &[1.0; 3]);
        assert!(CpuProbeReport::from_workers(vec![a.clone(), a.clone()]).is_err());
        let mut b = worker("b", &[1.0; 3]);
        b.config.iterations *= 2;
        assert!(CpuProbeReport::from_workers(vec![a, b]).is_err());
        let mut too_short = worker("short", &[1.0; 3]);
        too_short.warmup_elapsed_ns -= 1;
        assert!(CpuProbeReport::from_workers(vec![too_short]).is_err());
        let unconfigured = CpuProbeConfig {
            warmup_ms: 49,
            ..Default::default()
        };
        assert!(probe_cpu_thread(unconfigured).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn parses_cpu_after_a_command_name_with_parentheses() {
        let fields = (3..=52)
            .map(|i| if i == 39 { "7" } else { "0" })
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(parse_cpu(&format!("123 (work (er)) {fields}")), Some(7));
        assert_eq!(parse_cpu("broken"), None);
    }
}
