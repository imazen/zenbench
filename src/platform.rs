use std::sync::Mutex;
use sysinfo::System;

/// Cross-platform system state snapshot.
#[derive(Debug, Clone)]
pub struct SystemState {
    /// CPU utilization as fraction [0.0, 1.0].
    pub cpu_load: f64,
    /// Available RAM in bytes.
    pub available_ram_bytes: u64,
    /// Total RAM in bytes.
    pub total_ram_bytes: u64,
    /// CPU temperature in Celsius (if available).
    pub cpu_temp_c: Option<f64>,
    /// Number of "heavy" processes (>10% CPU) besides us.
    pub heavy_process_count: usize,
}

/// Shared system info handle. sysinfo::System is not Sync, so we wrap in Mutex.
pub struct SystemMonitor {
    sys: Mutex<System>,
}

impl SystemMonitor {
    pub fn new() -> Self {
        let sys = System::new_all();
        Self {
            sys: Mutex::new(sys),
        }
    }

    /// Refresh and snapshot current system state.
    pub fn snapshot(&self) -> SystemState {
        let mut sys = self.sys.lock().unwrap();
        sys.refresh_cpu_all();
        sys.refresh_memory();
        sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

        let cpus = sys.cpus();
        let cpu_load = if cpus.is_empty() {
            0.0
        } else {
            cpus.iter().map(|c| c.cpu_usage() as f64).sum::<f64>() / cpus.len() as f64 / 100.0
        };

        let available_ram_bytes = sys.available_memory();
        let total_ram_bytes = sys.total_memory();

        // CPU temperature: try to find it from components
        // sysinfo provides component temperatures on Linux and macOS
        let cpu_temp_c = {
            let components = sysinfo::Components::new_with_refreshed_list();
            components
                .iter()
                .filter(|c| {
                    let label = c.label().to_lowercase();
                    label.contains("cpu")
                        || label.contains("core")
                        || label.contains("package")
                        || label.contains("tctl")
                })
                .filter_map(|c| c.temperature())
                .map(|t| t as f64)
                .reduce(f64::max)
        };

        // Count heavy processes (>10% CPU usage on any core)
        let our_pid = sysinfo::get_current_pid().ok();
        let heavy_process_count = sys
            .processes()
            .values()
            .filter(|p| {
                // Exclude ourselves
                if let Some(our) = our_pid
                    && p.pid() == our
                {
                    return false;
                }
                p.cpu_usage() > 10.0
            })
            .count();

        SystemState {
            cpu_load,
            available_ram_bytes,
            total_ram_bytes,
            cpu_temp_c,
            heavy_process_count,
        }
    }
}

impl Default for SystemMonitor {
    fn default() -> Self {
        Self::new()
    }
}

/// Hardware fingerprint for testbed identification.
///
/// Stored in `SuiteResult` so baseline comparisons can detect when
/// the hardware has changed between runs.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[non_exhaustive]
pub struct Testbed {
    /// CPU model string (e.g., "AMD EPYC 7763 64-Core Processor").
    pub cpu_model: String,
    /// Target architecture (e.g., "x86_64", "aarch64").
    pub arch: String,
    /// Operating system (e.g., "linux", "windows", "macos").
    pub os: String,
    /// Logical (hyperthreaded) core count.
    pub logical_cores: usize,
    /// Physical core count.
    pub physical_cores: usize,
}

impl std::fmt::Display for Testbed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} ({}/{} cores, {}/{})",
            self.cpu_model, self.physical_cores, self.logical_cores, self.arch, self.os,
        )
    }
}

/// Detect the current hardware testbed.
pub fn detect_testbed() -> Testbed {
    let sys = System::new_all();
    let cpu_model = sys
        .cpus()
        .first()
        .map(|c| c.brand().trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let logical_cores = sys.cpus().len().max(1);
    let physical_cores = sysinfo::System::physical_core_count().unwrap_or(logical_cores);

    Testbed {
        cpu_model,
        arch: std::env::consts::ARCH.to_string(),
        os: std::env::consts::OS.to_string(),
        logical_cores,
        physical_cores,
    }
}

/// Detect if we're running in a CI environment.
/// Measure the timer resolution by finding the minimum non-zero delta
/// between consecutive `Instant::now()` calls.
///
/// Returns the resolution in nanoseconds. Typical values:
/// - Linux TSC: ~25ns
/// - macOS: ~40ns
/// - Windows QPC: ~300ns
pub fn timer_resolution_ns() -> u64 {
    let mut min_delta = u64::MAX;
    for _ in 0..1000 {
        let a = std::time::Instant::now();
        let b = std::time::Instant::now();
        let delta = b.duration_since(a).as_nanos() as u64;
        if delta > 0 && delta < min_delta {
            min_delta = delta;
        }
    }
    // Fallback: if all deltas were 0 (very fast timer), assume 1ns
    if min_delta == u64::MAX { 1 } else { min_delta }
}

pub fn detect_ci() -> Option<&'static str> {
    if std::env::var("GITHUB_ACTIONS").is_ok() {
        return Some("github-actions");
    }
    if std::env::var("GITLAB_CI").is_ok() {
        return Some("gitlab-ci");
    }
    if std::env::var("CIRCLECI").is_ok() {
        return Some("circleci");
    }
    if std::env::var("TRAVIS").is_ok() {
        return Some("travis-ci");
    }
    if std::env::var("JENKINS_URL").is_ok() {
        return Some("jenkins");
    }
    if std::env::var("BUILDKITE").is_ok() {
        return Some("buildkite");
    }
    if std::env::var("AZURE_PIPELINES").is_ok() || std::env::var("TF_BUILD").is_ok() {
        return Some("azure-pipelines");
    }
    if std::env::var("CI").is_ok() {
        return Some("unknown-ci");
    }
    None
}

/// Get the current git commit hash, if available.
pub fn git_commit_hash() -> Option<String> {
    std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout)
                    .ok()
                    .map(|s| s.trim().to_string())
            } else {
                None
            }
        })
}

/// Get the current git commit short hash.
pub fn git_short_hash() -> Option<String> {
    std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout)
                    .ok()
                    .map(|s| s.trim().to_string())
            } else {
                None
            }
        })
}
