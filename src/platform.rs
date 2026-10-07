use std::sync::Mutex;
use std::time::Instant;
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
    /// When the CPU counters were last refreshed. CPU usage is a DELTA between
    /// two refreshes; reading it too soon after the previous one yields 0.0.
    last_cpu_refresh: Mutex<Instant>,
}

impl SystemMonitor {
    pub fn new() -> Self {
        let sys = System::new_all(); // establishes the first CPU sample
        Self {
            sys: Mutex::new(sys),
            last_cpu_refresh: Mutex::new(Instant::now()),
        }
    }

    /// Refresh and snapshot current system state.
    ///
    /// Blocks for up to `sysinfo::MINIMUM_CPU_UPDATE_INTERVAL` (200 ms) when
    /// called sooner than that after the previous refresh. That wait is
    /// load-bearing, not politeness: `cpu_usage()` is computed from the delta
    /// between two refreshes, so a refresh that lands inside the minimum
    /// interval reports **0.0% on every core regardless of actual load**.
    ///
    /// This silently disabled the CPU half of `ResourceGate` for its whole
    /// life — `ResourceGate::new(cfg).check()` constructs a monitor and reads
    /// one snapshot immediately, so `cpu_load` was always 0.0 and
    /// `max_cpu_load` never tripped. Caught 2026-08-03 when a five-box
    /// zensysbench comparison gated 0 of 205 cells while the dev box sat at
    /// load 17 (measured: first snapshot 0.000, second 0.059, loadavg 2.65).
    /// A gate that cannot see the busiest box on the LAN is worse than no
    /// gate — it certifies contaminated numbers as clean.
    pub fn snapshot(&self) -> SystemState {
        {
            let last = *self.last_cpu_refresh.lock().unwrap();
            let since = last.elapsed();
            if since < sysinfo::MINIMUM_CPU_UPDATE_INTERVAL {
                std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL - since);
            }
        }
        let mut sys = self.sys.lock().unwrap();
        sys.refresh_cpu_all();
        sys.refresh_memory();
        sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        *self.last_cpu_refresh.lock().unwrap() = Instant::now();

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

        // Registered launcher/worker PIDs are part of this benchmark's work.
        // Their CPU counters use the same refresh interval as global CPU use.
        // Keep temperature/RAM checks global and every unregistered process visible.
        let mut owned = std::env::var("ZENBENCH_LAUNCHER_PIDS")
            .map(|s| crate::gate::parse_launcher_pids(&s))
            .unwrap_or_default();
        if let Ok(pid) = sysinfo::get_current_pid() {
            owned.push(pid);
        }
        // Linux also exposes each worker's tasks as process entries. The
        // leader's CPU usage already includes them: omit owned tasks from
        // contention counts without subtracting their CPU a second time.
        let owned_tasks: Vec<_> = owned
            .iter()
            .filter_map(|pid| sys.process(*pid))
            .filter_map(|p| p.tasks())
            .flat_map(|tasks| tasks.iter().copied())
            .collect();
        let (cpu_load, heavy_process_count) = foreign_activity(
            cpu_load,
            cpus.len(),
            sys.processes()
                .values()
                .map(|p| (p.pid(), f64::from(p.cpu_usage()))),
            &owned,
            &owned_tasks,
        );

        SystemState {
            cpu_load,
            available_ram_bytes,
            total_ram_bytes,
            cpu_temp_c,
            heavy_process_count,
        }
    }
}

// Process CPU percentages may exceed 100 for multithreaded work; global load
// is a fraction of all logical CPUs. Do not compare those units directly.
fn foreign_activity(
    global: f64,
    cores: usize,
    processes: impl IntoIterator<Item = (sysinfo::Pid, f64)>,
    owned: &[sysinfo::Pid],
    owned_tasks: &[sysinfo::Pid],
) -> (f64, usize) {
    let mut own_pct = 0.0;
    let mut heavy = 0;
    for (pid, pct) in processes {
        if owned.contains(&pid) {
            own_pct += pct;
        } else if !owned_tasks.contains(&pid) && pct > 10.0 {
            heavy += 1;
        }
    }
    let foreign = if cores == 0 {
        global
    } else {
        (global - own_pct / cores as f64 / 100.0).max(0.0)
    };
    (foreign, heavy)
}

#[cfg(test)]
mod owner_tests {
    use super::foreign_activity;
    use sysinfo::Pid;

    #[test]
    fn registered_mt_owner_does_not_hide_foreign_work() {
        let owner = Pid::from(11);
        let foreign = Pid::from(12);
        let (load, heavy) = foreign_activity(
            0.625,
            32,
            [(owner, 1600.0), (foreign, 400.0)],
            &[owner],
            &[],
        );
        assert_eq!(load, 0.125);
        assert_eq!(heavy, 1);
        let (load, heavy) =
            foreign_activity(0.625, 32, [(owner, 1600.0), (foreign, 400.0)], &[], &[]);
        assert_eq!(load, 0.625);
        assert_eq!(heavy, 2);
    }

    #[test]
    fn registered_tasks_are_not_double_subtracted_or_counted_as_rivals() {
        let owner = Pid::from(11);
        let task = Pid::from(13);
        let foreign = Pid::from(12);
        assert_eq!(
            foreign_activity(
                0.625,
                32,
                [(owner, 1600.0), (task, 800.0), (foreign, 400.0)],
                &[owner],
                &[task]
            ),
            (0.125, 1)
        );
        assert_eq!(
            foreign_activity(
                0.625,
                32,
                [(owner, 1600.0), (task, 800.0), (foreign, 400.0)],
                &[owner],
                &[]
            ),
            (0.125, 2)
        );
    }

    #[test]
    fn registering_an_absent_pid_cannot_reduce_foreign_load() {
        let absent = Pid::from(11);
        let foreign = Pid::from(12);
        assert_eq!(
            foreign_activity(0.5, 32, [(foreign, 1600.0)], &[absent], &[]),
            (0.5, 1)
        );
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
