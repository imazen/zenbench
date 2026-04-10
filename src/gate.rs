use crate::platform::{SystemMonitor, SystemState};
use std::time::{Duration, Instant};

/// Configuration for resource gating.
///
/// Before each measurement round, the harness checks system state
/// and waits if conditions aren't suitable for accurate benchmarking.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct GateConfig {
    /// Maximum CPU load fraction [0.0, 1.0] before we wait.
    /// Default: 0.15 (15%).
    pub max_cpu_load: f64,
    /// Minimum available RAM in bytes before we wait.
    /// Default: 512 MB.
    pub min_available_ram_bytes: u64,
    /// Maximum CPU temperature in Celsius before we wait.
    /// Default: 85°C. Set to None to disable.
    pub max_cpu_temp_c: Option<f64>,
    /// Maximum number of heavy processes (>10% CPU) before we wait.
    /// Default: 0.
    pub max_heavy_processes: usize,
    /// How long to wait for conditions to become favorable.
    /// Default: 60 seconds.
    pub max_wait: Duration,
    /// Polling interval when waiting.
    /// Default: 500ms.
    pub poll_interval: Duration,
    /// If true, refuse to report results if too many waits occurred.
    /// Default: false.
    pub strict: bool,
    /// Maximum number of waits before results are flagged as unreliable.
    /// Default: 10.
    pub max_wait_count: usize,
    /// Whether gating is enabled at all.
    /// Default: true.
    pub enabled: bool,
}

impl Default for GateConfig {
    fn default() -> Self {
        Self {
            max_cpu_load: 0.20,
            min_available_ram_bytes: 512 * 1024 * 1024,
            max_cpu_temp_c: Some(85.0),
            max_heavy_processes: 1, // Allow 1 background process (IDE, browser)
            max_wait: Duration::from_secs(30),
            poll_interval: Duration::from_millis(500),
            strict: false,
            max_wait_count: 10,
            enabled: true,
        }
    }
}

impl GateConfig {
    /// Permissive config for CI environments where we can't control the system.
    pub fn ci() -> Self {
        Self {
            max_cpu_load: 0.50,
            min_available_ram_bytes: 256 * 1024 * 1024,
            max_cpu_temp_c: None, // Often not available in CI
            max_heavy_processes: 5,
            max_wait: Duration::from_secs(30),
            strict: false,
            max_wait_count: 20,
            ..Default::default()
        }
    }

    /// Strict config for local development with quiet system.
    pub fn strict() -> Self {
        Self {
            max_cpu_load: 0.05,
            max_heavy_processes: 0,
            strict: true,
            max_wait_count: 5,
            ..Default::default()
        }
    }

    /// Disabled — no waiting, no checks.
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Default::default()
        }
    }

    pub fn max_cpu_load(mut self, load: f64) -> Self {
        self.max_cpu_load = load;
        self
    }

    pub fn min_available_ram_mb(mut self, mb: u64) -> Self {
        self.min_available_ram_bytes = mb * 1024 * 1024;
        self
    }

    pub fn max_cpu_temp_c(mut self, temp: Option<f64>) -> Self {
        self.max_cpu_temp_c = temp;
        self
    }

    pub fn max_heavy_processes(mut self, count: usize) -> Self {
        self.max_heavy_processes = count;
        self
    }

    pub fn max_wait(mut self, dur: Duration) -> Self {
        self.max_wait = dur;
        self
    }
}

/// Reason for a gate wait.
#[derive(Debug, Clone)]
pub enum GateReason {
    CpuLoad(f64),
    LowRam(u64),
    CpuTemp(f64),
    HeavyProcesses(usize),
}

impl std::fmt::Display for GateReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GateReason::CpuLoad(load) => write!(f, "CPU load {:.0}%", load * 100.0),
            GateReason::LowRam(bytes) => write!(f, "available RAM {}MB", bytes / 1024 / 1024),
            GateReason::CpuTemp(temp) => write!(f, "CPU temp {:.0}°C", temp),
            GateReason::HeavyProcesses(n) => write!(f, "{} heavy process(es)", n),
        }
    }
}

/// The resource gate: checks system state and waits for favorable conditions.
pub struct ResourceGate {
    config: GateConfig,
    monitor: SystemMonitor,
    total_waits: usize,
    total_wait_time: Duration,
}

impl ResourceGate {
    pub fn new(config: GateConfig) -> Self {
        Self {
            monitor: SystemMonitor::new(),
            config,
            total_waits: 0,
            total_wait_time: Duration::ZERO,
        }
    }

    /// Set thread allowance for the current benchmark group.
    ///
    /// When benchmarks spawn N threads, those threads may appear as
    /// "heavy processes" in the post-round gate check (CPU load lingers).
    /// This allowance raises the heavy_process threshold to compensate.
    /// Check if conditions are favorable. Returns None if OK, or the blocking reason.
    #[allow(dead_code)] // Used by bin targets
    pub fn check(&self) -> Option<GateReason> {
        if !self.config.enabled {
            return None;
        }

        let state = self.monitor.snapshot();
        self.check_state(&state)
    }

    fn check_state(&self, state: &SystemState) -> Option<GateReason> {
        if state.cpu_load > self.config.max_cpu_load {
            return Some(GateReason::CpuLoad(state.cpu_load));
        }
        if state.available_ram_bytes < self.config.min_available_ram_bytes {
            return Some(GateReason::LowRam(state.available_ram_bytes));
        }
        if let (Some(max_temp), Some(current_temp)) = (self.config.max_cpu_temp_c, state.cpu_temp_c)
        {
            if current_temp > max_temp {
                return Some(GateReason::CpuTemp(current_temp));
            }
        }
        let effective_max_heavy = self.config.max_heavy_processes;
        if state.heavy_process_count > effective_max_heavy {
            return Some(GateReason::HeavyProcesses(state.heavy_process_count));
        }
        None
    }

    /// Wait until conditions are favorable, or timeout.
    ///
    /// `deadline` optionally caps the maximum wait to the remaining time
    /// in the caller's budget. This prevents a gate wait from consuming
    /// more time than the measurement group has left.
    ///
    /// Returns true if conditions became favorable, false if timed out.
    #[allow(dead_code)] // Public API for external gate users
    pub fn wait_for_clear(&mut self) -> bool {
        self.wait_for_clear_with_deadline(None)
    }

    /// Like [`ResourceGate::wait_for_clear`], but with an explicit deadline.
    ///
    /// The gate will wait at most `min(max_wait, deadline)`.
    #[allow(dead_code)] // Public API for external gate users
    pub fn wait_for_clear_with_deadline(&mut self, deadline: Option<Duration>) -> bool {
        if !self.config.enabled {
            return true;
        }

        let effective_max = match deadline {
            Some(dl) => self.config.max_wait.min(dl),
            None => self.config.max_wait,
        };

        let start = Instant::now();
        let mut last_status = Instant::now() - Duration::from_secs(10); // force first update
        loop {
            let state = self.monitor.snapshot();
            match self.check_state(&state) {
                None => {
                    crate::report::clear_status();
                    return true;
                }
                Some(reason) => {
                    if start.elapsed() >= effective_max {
                        crate::report::clear_status();
                        return false;
                    }
                    // Throttle status updates to every 5 seconds
                    if last_status.elapsed() >= Duration::from_secs(5) {
                        let elapsed = start.elapsed().as_secs_f64();
                        let max = effective_max.as_secs_f64();
                        crate::report::status(&format!(
                            "[zenbench] waiting ({elapsed:.0}s/{max:.0}s): {reason}"
                        ));
                        last_status = Instant::now();
                    }
                    self.total_waits += 1;
                    std::thread::sleep(self.config.poll_interval);
                    self.total_wait_time += self.config.poll_interval;
                }
            }
        }
    }

    /// Block until no other benchmark harness is running.
    ///
    /// Detects zenbench, criterion, divan, and cargo-bench processes by name.
    /// This prevents concurrent benchmarks from corrupting each other's data.
    /// General system noise is NOT gated here — only benchmark-vs-benchmark.
    pub fn wait_for_no_benchmarks(&mut self) {
        if !self.config.enabled {
            return;
        }

        // Benchmark process names to look for (case-insensitive substrings)
        const BENCH_NAMES: &[&str] = &["criterion", "divan", "zenbench", "cargo-bench", "bench-"];

        let our_pid = sysinfo::get_current_pid().ok();

        // Collect PIDs to exclude: ourselves, plus any ancestor PIDs set by
        // the launcher (e.g., `zenbench self-compare` sets ZENBENCH_LAUNCHER_PIDS
        // so the child benchmark doesn't wait on its own parent CLI process).
        let mut excluded_pids: Vec<sysinfo::Pid> = Vec::new();
        if let Some(our) = our_pid {
            excluded_pids.push(our);
        }
        if let Ok(pids_str) = std::env::var("ZENBENCH_LAUNCHER_PIDS") {
            for s in pids_str.split(',') {
                if let Ok(pid) = s.trim().parse::<usize>() {
                    excluded_pids.push(sysinfo::Pid::from(pid));
                }
            }
        }

        let start = Instant::now();
        let max_wait = Duration::from_secs(30);
        let mut warned = false;

        loop {
            // Use a fresh System scan for process detection (the monitor
            // may not refresh process command lines in its snapshot).
            let mut sys = sysinfo::System::new();
            sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

            let bench_count = sys
                .processes()
                .values()
                .filter(|p| {
                    // Skip ourselves and launcher ancestors
                    if excluded_pids.contains(&p.pid()) {
                        return false;
                    }

                    let name = p.name().to_string_lossy().to_lowercase();
                    let cmd: String = p
                        .cmd()
                        .iter()
                        .map(|s| s.to_string_lossy().to_lowercase())
                        .collect::<Vec<_>>()
                        .join(" ");
                    BENCH_NAMES
                        .iter()
                        .any(|&pat| name.contains(pat) || cmd.contains(pat))
                })
                .count();

            if bench_count == 0 {
                if warned {
                    crate::report::clear_status();
                }
                return;
            }

            if start.elapsed() >= max_wait {
                crate::report::clear_status();
                return; // give up waiting, measure anyway
            }

            if !warned {
                crate::report::status(&format!(
                    "[zenbench] waiting for {bench_count} other benchmark process(es) to finish..."
                ));
                warned = true;
            }

            std::thread::sleep(Duration::from_secs(1));
            self.total_waits += 1;
            self.total_wait_time += Duration::from_secs(1);
        }
    }

    /// Non-blocking system check. Records whether the system is noisy
    /// but never blocks. The statistical machinery handles noisy samples.
    pub fn check_and_record(&mut self) {
        if !self.config.enabled {
            return;
        }
        let state = self.monitor.snapshot();
        if self.check_state(&state).is_some() {
            self.total_waits += 1;
        }
    }

    /// Brief non-blocking gate check. Waits up to `max_wait` for conditions to
    /// improve, then proceeds regardless. Shows a single status line during the
    /// wait, clears it when done.
    ///
    /// This replaces the old blocking gate that could consume 89% of total
    /// benchmark time on busy systems. The statistical machinery (IQR outlier
    /// removal, bootstrap CI, MAD) handles noisy samples — the gate just gives
    /// the system a brief chance to settle.
    #[allow(dead_code)]
    pub fn brief_wait(&mut self, max_wait: Duration) {
        if !self.config.enabled {
            return;
        }

        let start = Instant::now();
        loop {
            let state = self.monitor.snapshot();
            if self.check_state(&state).is_none() {
                crate::report::clear_status();
                return; // system is quiet, proceed
            }
            if start.elapsed() >= max_wait {
                crate::report::clear_status();
                self.total_waits += 1;
                self.total_wait_time += start.elapsed();
                return; // time's up, measure anyway
            }
            if self.total_waits == 0 {
                // Show status only on first wait of this group
                if let Some(reason) = self.check_state(&state) {
                    crate::report::status(&format!(
                        "[zenbench] system busy ({reason}), waiting up to {:.0}s...",
                        max_wait.as_secs_f64(),
                    ));
                }
            }
            std::thread::sleep(self.config.poll_interval);
        }
    }

    /// Whether the benchmark results should be considered unreliable due to
    /// excessive waiting (indicates a noisy system).
    #[allow(dead_code)] // May be used by bin targets
    pub fn is_unreliable(&self) -> bool {
        self.config.strict && self.total_waits > self.config.max_wait_count
    }

    /// Total number of times we had to wait.
    pub fn total_waits(&self) -> usize {
        self.total_waits
    }

    /// Total time spent waiting.
    pub fn total_wait_time(&self) -> Duration {
        self.total_wait_time
    }
}

/// Parse the `ZENBENCH_LAUNCHER_PIDS` env var value into a list of PIDs.
/// Comma-separated, ignores invalid entries. Used by `wait_for_no_benchmarks`
/// (integrated via PR #8 / fix/self-compare-gate).
#[cfg(test)]
fn parse_launcher_pids(val: &str) -> Vec<sysinfo::Pid> {
    val.split(',')
        .filter_map(|s| s.trim().parse::<usize>().ok().map(sysinfo::Pid::from))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_launcher_pids_single() {
        let pids = parse_launcher_pids("12345");
        assert_eq!(pids.len(), 1);
        assert_eq!(pids[0], sysinfo::Pid::from(12345));
    }

    #[test]
    fn parse_launcher_pids_multiple() {
        let pids = parse_launcher_pids("100,200,300");
        assert_eq!(pids.len(), 3);
        assert_eq!(pids[0], sysinfo::Pid::from(100));
        assert_eq!(pids[1], sysinfo::Pid::from(200));
        assert_eq!(pids[2], sysinfo::Pid::from(300));
    }

    #[test]
    fn parse_launcher_pids_with_whitespace() {
        let pids = parse_launcher_pids(" 100 , 200 , 300 ");
        assert_eq!(pids.len(), 3);
    }

    #[test]
    fn parse_launcher_pids_empty() {
        let pids = parse_launcher_pids("");
        assert!(pids.is_empty());
    }

    #[test]
    fn parse_launcher_pids_ignores_invalid() {
        let pids = parse_launcher_pids("123,not_a_pid,456");
        assert_eq!(pids.len(), 2);
        assert_eq!(pids[0], sysinfo::Pid::from(123));
        assert_eq!(pids[1], sysinfo::Pid::from(456));
    }

    #[test]
    fn parse_launcher_pids_chained() {
        // Simulates nested self-compare: parent appends its PID
        let pids = parse_launcher_pids("1000,2000");
        assert_eq!(pids.len(), 2);
    }

    #[test]
    fn gate_disabled_skips_benchmark_check() {
        let mut gate = ResourceGate::new(GateConfig::disabled());
        // Should return immediately without scanning
        gate.wait_for_no_benchmarks();
        assert_eq!(gate.total_waits(), 0);
    }

    #[test]
    fn gate_config_defaults_are_sane() {
        let config = GateConfig::default();
        assert!(config.enabled);
        assert!(config.max_cpu_load > 0.0 && config.max_cpu_load < 1.0);
        assert!(config.min_available_ram_bytes > 0);
        assert!(config.max_wait > Duration::ZERO);
        assert!(config.poll_interval > Duration::ZERO);
    }

    #[test]
    fn gate_config_ci_is_more_permissive() {
        let default = GateConfig::default();
        let ci = GateConfig::ci();
        assert!(ci.max_cpu_load >= default.max_cpu_load);
        assert!(ci.max_heavy_processes >= default.max_heavy_processes);
    }
}
