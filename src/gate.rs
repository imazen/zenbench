use crate::platform::{SystemMonitor, SystemState};
use std::time::{Duration, Instant};

/// Configuration for resource gating.
///
/// Before each measurement round, the harness checks system state
/// and waits if conditions aren't suitable for accurate benchmarking.
#[derive(Debug, Clone)]
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
            max_cpu_load: 0.30,
            min_available_ram_bytes: 512 * 1024 * 1024,
            max_cpu_temp_c: Some(90.0),
            max_heavy_processes: 3,
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

    /// Check if conditions are favorable. Returns None if OK, or the blocking reason.
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
        if state.heavy_process_count > self.config.max_heavy_processes {
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
    pub fn wait_for_clear(&mut self) -> bool {
        self.wait_for_clear_with_deadline(None)
    }

    /// Like [`wait_for_clear`], but with an explicit deadline.
    ///
    /// The gate will wait at most `min(max_wait, deadline)`.
    pub fn wait_for_clear_with_deadline(&mut self, deadline: Option<Duration>) -> bool {
        if !self.config.enabled {
            return true;
        }

        let effective_max = match deadline {
            Some(dl) => self.config.max_wait.min(dl),
            None => self.config.max_wait,
        };

        let start = Instant::now();
        loop {
            let state = self.monitor.snapshot();
            match self.check_state(&state) {
                None => return true,
                Some(reason) => {
                    if start.elapsed() >= effective_max {
                        eprintln!(
                            "[zenbench] gate timeout after {:.1}s: {}",
                            start.elapsed().as_secs_f64(),
                            reason
                        );
                        return false;
                    }
                    if self.total_waits == 0 || self.total_waits % 10 == 0 {
                        eprintln!("[zenbench] waiting: {}", reason);
                    }
                    self.total_waits += 1;
                    std::thread::sleep(self.config.poll_interval);
                    self.total_wait_time += self.config.poll_interval;
                }
            }
        }
    }

    /// Whether the benchmark results should be considered unreliable due to
    /// excessive waiting (indicates a noisy system).
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
