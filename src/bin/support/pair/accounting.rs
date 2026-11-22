//! Linux cgroup v2 accounting for an invocation and its short-lived descendants.
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub(super) const METHOD: &str = "linux-cgroup-v2-cpu-stat";

pub(super) fn validate_parent(path: &Path) -> Result<PathBuf, String> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        Err("cgroup accounting requires Linux".into())
    }
    #[cfg(target_os = "linux")]
    {
        let path = if path == Path::new("auto") {
            let membership = std::fs::read_to_string("/proc/self/cgroup")
                .map_err(|e| format!("cannot discover current cgroup: {e}"))?;
            current_parent(&membership)?
        } else {
            path.to_path_buf()
        };
        let root = path
            .canonicalize()
            .map_err(|e| format!("accounting parent: {e}"))?;
        require_cgroup_v2(&root)?;
        // Delegation must apply to this caller. Starting outside the subtree
        // can cross CPU affinity, quotas, accounting and resource policy.
        if !members(&root)?
            .lines()
            .any(|pid| pid.parse() == Ok(std::process::id()))
        {
            return Err("the accounting parent must contain the zenbench supervisor. Start with `systemd-run --user --scope -p Delegate=yes zenbench pair PLAN --mode=confirm --accounting-cgroup`".into());
        }
        rustix::fs::access(&root, rustix::fs::Access::WRITE_OK)
            .and_then(|()| rustix::fs::access(root.join("cgroup.procs"), rustix::fs::Access::WRITE_OK))
            .map_err(|e| format!("the current cgroup is not writable ({e}). Start with `systemd-run --user --scope -p Delegate=yes zenbench pair PLAN --mode=confirm --accounting-cgroup`"))?;
        Ok(root)
    }
}

#[cfg(target_os = "linux")]
fn current_parent(membership: &str) -> Result<PathBuf, String> {
    let path = membership
        .lines()
        .find_map(|line| line.strip_prefix("0::/"))
        .ok_or("current cgroup v2 membership is unavailable; supply --accounting-cgroup=PATH")?;
    if path.split('/').any(|part| matches!(part, ".." | ".")) {
        return Err("current cgroup membership cannot be resolved to a local path; supply --accounting-cgroup=PATH".into());
    }
    Ok(Path::new("/sys/fs/cgroup").join(path.trim_start_matches('/')))
}

#[cfg(target_os = "linux")]
fn require_cgroup_v2(path: &Path) -> Result<(), String> {
    let stat = rustix::fs::statfs(path).map_err(|e| e.to_string())?;
    if stat.f_type != 0x6367_7270 || !path.join("cgroup.procs").is_file() {
        return Err("accounting path must be a cgroup v2 directory".into());
    }
    Ok(())
}

fn members(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path.join("cgroup.procs"))
        .map_err(|e| format!("cannot read cgroup membership: {e}"))
}

pub(super) struct Scope {
    path: PathBuf,
    min_window: Duration,
}

impl Scope {
    pub(super) fn create(parent: &Path) -> Result<Self, String> {
        let host = std::fs::read_to_string("/proc/stat").map_err(|e| e.to_string())?;
        let logical_cpus = host
            .lines()
            .filter_map(|line| line.split_whitespace().next())
            .filter(|field| {
                field
                    .strip_prefix("cpu")
                    .is_some_and(|cpu| cpu.parse::<u32>().is_ok())
            })
            .count() as u64;
        let min_window = minimum_window(logical_cpus, clock_ticks())
            .ok_or("cannot determine the host CPU counter resolution")?;
        let path = parent.join(format!(
            "zenbench-{}-{:x}",
            std::process::id(),
            super::now()
        ));
        std::fs::create_dir(&path).map_err(|e| format!("cannot create accounting cgroup: {e}"))?;
        let scope = Self { path, min_window };
        if scope.usage()? != 0 || members(&scope.path)?.lines().next().is_some() {
            return Err("fresh accounting cgroup already contains work".into());
        }
        Ok(scope)
    }

    pub(super) fn command(&self, program: &Path) -> Result<std::process::Command, String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let mut command = std::process::Command::new(exe);
        command
            .arg("pair-cpu-scope")
            .arg(&self.path)
            .arg(program)
            .arg("--");
        Ok(command)
    }

    fn usage(&self) -> Result<u64, String> {
        let stat = std::fs::read_to_string(self.path.join("cpu.stat"))
            .map_err(|e| format!("cannot read cgroup cpu.stat: {e}"))?;
        field(&stat, "usage_usec").ok_or_else(|| "missing or malformed cgroup usage_usec".into())
    }

    pub(super) fn sample(&self) -> Result<Sample, String> {
        let owned_before = self.usage()?;
        let host = std::fs::read_to_string("/proc/stat").map_err(|e| e.to_string())?;
        let counters = host_cpu(&host).ok_or("missing or malformed host CPU counters")?;
        let owned_after = self.usage()?;
        if owned_after < owned_before {
            return Err("cgroup CPU counter moved backwards".into());
        }
        Ok(Sample {
            counters,
            owned_before,
            owned_after,
            observed_at: Instant::now(),
        })
    }

    pub(super) fn wait_for_sample(&self, previous: &Sample) {
        if self.min_window > sysinfo::MINIMUM_CPU_UPDATE_INTERVAL {
            std::thread::sleep(
                self.min_window
                    .saturating_sub(previous.observed_at.elapsed()),
            );
        }
    }

    pub(super) fn verify_empty(&self) -> Result<(), String> {
        let text = std::fs::read_to_string(self.path.join("cgroup.events"))
            .map_err(|e| format!("cannot read cgroup.events: {e}"))?;
        if field(&text, "populated") != Some(0) {
            return Err("accounting cgroup still populated or unreadable after leader exit".into());
        }
        Ok(())
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        if let Err(e) = std::fs::remove_dir(&self.path) {
            eprintln!(
                "[zenbench pair] accounting cgroup remains at {}: {e}",
                self.path.display()
            );
        }
    }
}

pub(crate) fn enter(
    path: &Path,
    program: &Path,
    args: &[std::ffi::OsString],
) -> Result<(), String> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (path, program, args);
        Err("cgroup accounting requires Linux".into())
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        require_cgroup_v2(path)?;
        std::fs::write(path.join("cgroup.procs"), b"0\n")
            .map_err(|e| format!("cannot enter invocation accounting cgroup: {e}"))?;
        if !members(path)?
            .lines()
            .any(|pid| pid.parse() == Ok(std::process::id()))
        {
            return Err("invocation cgroup membership could not be verified".into());
        }
        Err(std::process::Command::new(program)
            .args(args)
            .exec()
            .to_string())
    }
}

fn field(text: &str, name: &str) -> Option<u64> {
    let (key, value) = text
        .lines()
        .filter_map(|l| l.split_once(' '))
        .find(|(k, _)| *k == name)?;
    debug_assert_eq!(key, name);
    value.trim().parse().ok()
}

fn host_cpu(text: &str) -> Option<[u64; 8]> {
    let mut fields = text.lines().next()?.split_whitespace();
    if fields.next()? != "cpu" {
        return None;
    }
    let mut counters = [0; 8];
    for counter in &mut counters {
        *counter = fields.next()?.parse().ok()?;
    }
    // Guest columns already contribute to user and nice above.
    Some(counters)
}

pub(super) struct Sample {
    counters: [u64; 8],
    owned_before: u64,
    owned_after: u64,
    observed_at: Instant,
}

#[cfg(target_os = "linux")]
fn clock_ticks() -> u64 {
    rustix::param::clock_ticks_per_second()
}

#[cfg(not(target_os = "linux"))]
fn clock_ticks() -> u64 {
    0
}

fn minimum_window(logical_cpus: u64, ticks_per_second: u64) -> Option<Duration> {
    let ticks_per_second = logical_cpus.checked_mul(ticks_per_second)?;
    if ticks_per_second == 0 {
        return None;
    }
    // Aim for 300 total ticks so the six-tick quantization allowance leaves
    // about three percentage points for measured background work. Keep the
    // wait below the strict two-second observation deadline on one CPU.
    Some(
        Duration::from_micros(300_000_000_u64.div_ceil(ticks_per_second))
            .min(Duration::from_millis(1600))
            .max(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL),
    )
}

impl Sample {
    pub(super) fn load_since(&self, previous: &Self) -> Option<(f64, u64)> {
        self.foreign_load(previous, clock_ticks())
    }

    fn foreign_load(&self, previous: &Self, ticks_per_second: u64) -> Option<(f64, u64)> {
        // Both owned reads bracket their host read. Subtract only CPU that
        // accrued inside the host interval, across all process lifetimes.
        let owned_usec = self
            .owned_before
            .checked_sub(previous.owned_after)?
            .saturating_sub(1);
        if ticks_per_second == 0 {
            return None;
        }
        let mut total = 0u64;
        let mut active = 0u64;
        for (index, (current, old)) in self.counters.iter().zip(previous.counters).enumerate() {
            let delta = current.checked_sub(old)?;
            total = total.checked_add(delta)?;
            if index != 3 && index != 4 {
                // idle and iowait
                active = active.checked_add(delta)?;
            }
        }
        // Each exported field is quantized in USER_HZ. Bound the possible
        // active CPU upward (six fields) and host capacity downward (eight).
        let capacity = total.checked_sub(8)? as f64;
        let active_upper = active.checked_add(6)? as f64;
        let owned_ticks = owned_usec as f64 * ticks_per_second as f64 / 1e6;
        if capacity == 0.0 || owned_ticks > active_upper {
            return None;
        }
        let fraction = ((active_upper - owned_ticks) / capacity).min(1.0);
        Some((fraction, owned_usec))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn finds_current_scope_and_rejects_unusable_membership() {
        assert_eq!(
            current_parent("0::/user.slice/bench.scope\n").unwrap(),
            Path::new("/sys/fs/cgroup/user.slice/bench.scope")
        );
        assert_eq!(
            current_parent("0::/\n").unwrap(),
            Path::new("/sys/fs/cgroup")
        );
        assert!(current_parent("1:cpu:/legacy\n").is_err());
        assert!(current_parent("0::/../../external\n").is_err());
    }
    #[test]
    fn accounts_for_reaped_work_and_keeps_foreign_load() {
        let observed_at = Instant::now();
        let start = Sample {
            counters: [100; 8],
            owned_before: 0,
            owned_after: 100,
            observed_at,
        };
        let end = Sample {
            counters: [140, 100, 100, 1060, 100, 100, 100, 100],
            owned_before: 400_101,
            owned_after: 450_000,
            observed_at,
        };
        let (foreign, owned) = end.foreign_load(&start, 100).unwrap();
        assert_eq!(owned, 400_000);
        assert!(foreign < 0.05);
        let busy = Sample {
            counters: [220, 100, 100, 980, 100, 100, 100, 100],
            ..end
        };
        assert!(busy.foreign_load(&start, 100).unwrap().0 > 0.05);
        let reset = Sample {
            owned_before: 0,
            ..busy
        };
        assert!(reset.foreign_load(&start, 100).is_none());
        assert!(host_cpu("cpu 1 2 3 4 5").is_none());
        assert_eq!(
            host_cpu("cpu 1 2 3 4 5 6 7 8 9 10"),
            Some([1, 2, 3, 4, 5, 6, 7, 8])
        );
    }

    #[test]
    fn small_hosts_get_enough_cpu_ticks_to_admit_quiet_work() {
        assert_eq!(minimum_window(1, 100), Some(Duration::from_millis(1600)));
        assert_eq!(minimum_window(4, 100), Some(Duration::from_millis(750)));
        assert_eq!(
            minimum_window(16, 100),
            Some(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL)
        );
        assert_eq!(minimum_window(0, 100), None);
        let start = Sample {
            counters: [10; 8],
            owned_before: 0,
            owned_after: 0,
            observed_at: Instant::now(),
        };
        let short = Sample {
            counters: [10, 10, 10, 90, 10, 10, 10, 10],
            ..start
        };
        let long = Sample {
            counters: [16, 10, 10, 304, 10, 10, 10, 10],
            ..start
        };
        assert!(short.foreign_load(&start, 100).unwrap().0 > 0.05);
        assert!(long.foreign_load(&start, 100).unwrap().0 < 0.05);
    }
}
