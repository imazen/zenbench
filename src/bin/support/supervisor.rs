//! Foreground command supervision: process groups on Unix, Jobs on Windows.
//! Unix programs must not daemonize or leave their process group.
use process_wrap::std::*;
use std::io;
use std::process::{Command, ExitStatus};
use std::time::{Duration, Instant};

pub(super) struct Outcome {
    pub status: ExitStatus,
    // Windows process-lifetime working-set and private-commit high-water marks.
    // An unavailable counter retains its reason rather than becoming zero.
    pub windows_peaks: Option<Result<(u64, u64), String>>,
}

pub(super) fn run(
    command: Command,
    budget: Duration,
    reservation: &mut crate::coordination::Reservation,
    scope: Option<&str>,
    logs: &[std::path::PathBuf],
    process_id: &mut Option<u32>,
) -> io::Result<ExitStatus> {
    run_controlled(
        command,
        budget,
        reservation,
        scope,
        logs,
        process_id,
        &mut || Ok(()),
    )
    .map(|outcome| outcome.status)
}

pub(super) fn run_controlled(
    mut command: Command,
    budget: Duration,
    reservation: &mut crate::coordination::Reservation,
    scope: Option<&str>,
    logs: &[std::path::PathBuf],
    process_id: &mut Option<u32>,
    poll: &mut dyn FnMut() -> io::Result<()>,
) -> io::Result<Outcome> {
    reservation.command(&mut command);
    reservation.retain_until_reaped();
    let mut wrapped = StdCommandWrap::from(command);
    #[cfg(unix)]
    wrapped.wrap(ProcessGroup::leader());
    #[cfg(windows)]
    let observer = {
        let job = std::sync::Arc::new(win32job::Job::create().map_err(io::Error::other)?);
        // JobObject pre_spawn suspends the process. The observer is attached
        // first during wrap_child, before JobObject resumes the child.
        wrapped.wrap(JobObserver(job.clone())).wrap(JobObject);
        job
    };
    // A wrapper can fail after spawn; retain admission if no child handle
    // is available to prove cleanup.
    let mut child = wrapped.spawn()?;
    *process_id = Some(child.id());
    let start = Instant::now();
    loop {
        let interruption = poll().err();
        let oversized = logs
            .iter()
            .any(|p| std::fs::metadata(p).is_ok_and(|m| m.len() > 64 * 1024 * 1024));
        if start.elapsed() >= budget || oversized || interruption.is_some() {
            child.start_kill()?;
            child.inner_mut().wait()?;
            #[cfg(windows)]
            wait_job_empty(&observer)?;
            #[cfg(unix)]
            wait_group_empty(child.id(), Duration::from_secs(5))?;
            cleanup_scope(scope)?;
            reservation.reaped();
            return Err(interruption.unwrap_or_else(|| {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    if oversized {
                        "child log limit exceeded; process group/job terminated"
                    } else {
                        "child deadline exceeded; process group/job terminated"
                    },
                )
            }));
        }
        if let Some(status) = child.try_wait()? {
            #[cfg(windows)]
            let windows_peaks = {
                use std::os::windows::io::AsRawHandle;
                Some(win32job::utils::get_process_memory_info(child.inner().as_raw_handle() as isize)
                    .map(|c| (c.peak_working_set_size as u64, c.peak_pagefile_usage as u64))
                    .map_err(|e| e.to_string()))
            };
            #[cfg(not(windows))]
            let windows_peaks = None;
            #[cfg(unix)]
            {
                // A foreground program must join its workers. Kill residual
                // group members before allowing the next measurement.
                let _ = child.start_kill();
                wait_group_empty(child.id(), Duration::from_secs(5))?;
            }
            #[cfg(windows)]
            {
                child.start_kill()?;
                wait_job_empty(&observer)?;
            }
            cleanup_scope(scope)?;
            reservation.reaped();
            return Ok(Outcome {
                status,
                windows_peaks,
            });
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(unix)]
fn wait_group_empty(group: u32, budget: Duration) -> io::Result<()> {
    let start = Instant::now();
    loop {
        // ps is queried outside the child's measured work. Zombies do not
        // execute; waitpid cannot reap grandchildren adopted by another parent.
        let output = Command::new("ps").args(["-axo", "pgid=,stat="]).output()?;
        if !output.status.success() {
            return Err(io::Error::other("cannot verify process-group cleanup"));
        }
        let active = String::from_utf8_lossy(&output.stdout).lines().any(|line| {
            let mut fields = line.split_whitespace();
            fields.next().and_then(|s| s.parse::<u32>().ok()) == Some(group)
                && fields.next().is_none_or(|s| !s.starts_with('Z'))
        });
        if !active {
            return Ok(());
        }
        if start.elapsed() >= budget {
            return Err(io::Error::other(
                "process group still active; reservation retained",
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn cleanup_scope(scope: Option<&str>) -> io::Result<()> {
    let Some(scope) = scope else { return Ok(()) };
    let started = Instant::now();
    loop {
        let state = Command::new("systemctl")
            .args(["--user", "show", scope, "--property=ActiveState,LoadState"])
            .output()?;
        let text = String::from_utf8_lossy(&state.stdout);
        if text.lines().any(|s| {
            matches!(
                s,
                "LoadState=not-found" | "ActiveState=inactive" | "ActiveState=failed"
            )
        }) {
            return Ok(());
        }
        if !state.status.success() {
            return Err(io::Error::other(
                "cannot verify scope completion; reservation retained",
            ));
        }
        if started.elapsed() > Duration::from_secs(5) {
            return Err(io::Error::other("scope still active; reservation retained"));
        }
        let _ = Command::new("systemctl")
            .args(["--user", "kill", "--signal=KILL", "--kill-whom=all", scope])
            .output()?;
        std::thread::sleep(Duration::from_millis(50));
    }
}

// Independently query Job membership: the process wrapper's try_wait reports
// the leader status, which alone does not prove its descendants have exited.
#[cfg(windows)]
#[derive(Debug)]
struct JobObserver(std::sync::Arc<win32job::Job>);
#[cfg(windows)]
impl StdCommandWrapper for JobObserver {
    fn wrap_child(
        &mut self,
        child: Box<dyn StdChildWrapper>,
        _core: &StdCommandWrap,
    ) -> io::Result<Box<dyn StdChildWrapper>> {
        use std::os::windows::io::AsRawHandle;
        self.0
            .assign_process(child.inner().as_raw_handle() as isize)
            .map_err(io::Error::other)?;
        Ok(child)
    }
}
#[cfg(windows)]
fn wait_job_empty(job: &win32job::Job) -> io::Result<()> {
    let start = Instant::now();
    loop {
        if job
            .query_process_id_list()
            .map_err(io::Error::other)?
            .is_empty()
        {
            return Ok(());
        }
        if start.elapsed() >= Duration::from_secs(5) {
            return Err(io::Error::other(
                "Windows Job still active; reservation retained",
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
