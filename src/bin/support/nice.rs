use std::ffi::OsString;
use std::io;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

pub(crate) fn run(
    program: &Path,
    args: &[OsString],
    wait: u64,
    timeout: u64,
    jobs: Option<usize>,
    memory_mib: Option<u64>,
) -> io::Result<i32> {
    if wait == 0 || timeout == 0 || jobs == Some(0) || memory_mib == Some(0) {
        return Err(io::Error::other(
            "wait, timeout, jobs and memory must be positive",
        ));
    }
    let started = Instant::now();
    let budget = Duration::from_secs(wait);
    let mut reservation =
        crate::coordination::Reservation::acquire("nice background task", budget)?;
    let monitor = zenbench::platform::SystemMonitor::new();
    let state = loop {
        let state = monitor.snapshot();
        if state.available_ram_bytes >= 4 * 1024_u64.pow(3) && state.cpu_load <= 0.8 {
            break state;
        }
        if started.elapsed() >= budget {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "resource admission deadline expired; task not started",
            ));
        }
        eprintln!(
            "[zenbench nice] waiting: CPU {:.0}%, available RAM {} MiB",
            state.cpu_load * 100.0,
            state.available_ram_bytes / 1024 / 1024
        );
        std::thread::sleep(Duration::from_secs(1));
    };
    let jobs = jobs.unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map(|n| n.get().saturating_sub(4).max(1))
            .unwrap_or(1)
    });
    let memory = memory_mib.unwrap_or((state.total_ram_bytes / 2 / 1024 / 1024).min(24 * 1024));
    if memory > state.available_ram_bytes / 1024 / 1024 {
        return Err(io::Error::other(
            "requested memory ceiling exceeds currently available RAM",
        ));
    }
    eprintln!(
        "[zenbench nice] admitted; jobs={jobs}, coordination={}",
        crate::coordination::directory()?.display()
    );
    #[cfg(windows)]
    let _priority = {
        let mut limits = win32job::ExtendedLimitInfo::new();
        limits.limit_priority_class(win32job::PriorityClass::BelowNormal);
        let job = win32job::Job::create_with_limit_info(&limits).map_err(io::Error::other)?;
        job.assign_current_process().map_err(io::Error::other)?;
        job
    };
    #[cfg(target_os = "linux")]
    let unit = format!(
        "zenbench-nice-{}-{}.scope",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    #[cfg(target_os = "linux")]
    let mut command = {
        // Scope mode inherits cwd and the complete environment without putting
        // secret environment values into an argv list or a retained manifest.
        let mut command = Command::new("systemd-run");
        command
            .args(["--user", "--scope", "--quiet"])
            .arg(format!("--unit={unit}"))
            .arg(format!("--property=MemoryMax={}M", memory))
            .arg("--property=MemorySwapMax=0")
            .arg("nice")
            .args(["-n", "19", "ionice", "-c", "3"])
            .arg(program);
        command
    };
    #[cfg(not(target_os = "linux"))]
    let mut command = {
        if memory_mib.is_some() {
            return Err(io::Error::other(
                "memory ceilings currently require Linux systemd user scopes",
            ));
        }
        #[cfg(unix)]
        let command = {
            let mut c = Command::new("nice");
            c.args(["-n", "19"]).arg(program);
            c
        };
        #[cfg(not(unix))]
        let command = Command::new(program);
        eprintln!("[zenbench nice] no hard memory ceiling on this platform; foreground tasks only");
        command
    };
    command
        .args(args)
        .env("CARGO_BUILD_JOBS", jobs.to_string())
        .env("RAYON_NUM_THREADS", jobs.to_string())
        .env("OMP_NUM_THREADS", jobs.to_string());
    #[cfg(target_os = "linux")]
    let scope = Some(unit.as_str());
    #[cfg(not(target_os = "linux"))]
    let scope = None;
    let status = super::supervisor::run(
        command,
        Duration::from_secs(timeout),
        &mut reservation,
        scope,
        &[],
        &mut None,
    )?;
    Ok(status.code().unwrap_or(1))
}
