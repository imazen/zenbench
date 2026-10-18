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

/// Private observation hooks for the owned leader process.
pub(super) struct ChildObservation<'a> {
    pub process_id: &'a mut Option<u32>,
    pub poll: &'a mut dyn FnMut(u32) -> io::Result<()>,
    pub lifecycle: Option<&'a mut Lifecycle>,
}

/// Owned leader lifecycle bounds from the supervisor's own exit probes.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Lifecycle {
    /// Taken before the last `try_wait() == None`: a lower bound on exit.
    pub last_alive: Option<Instant>,
    /// Taken after the owned `try_wait()` returned the exit status, before
    /// cleanup: an upper bound on exit and, on Unix, on reaping the leader.
    /// Absent on deadline, interruption and probe-error paths.
    pub exit_observed: Option<Instant>,
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
    command: Command,
    budget: Duration,
    reservation: &mut crate::coordination::Reservation,
    scope: Option<&str>,
    logs: &[std::path::PathBuf],
    process_id: &mut Option<u32>,
    poll: &mut dyn FnMut() -> io::Result<()>,
) -> io::Result<Outcome> {
    run_observed(
        command,
        budget,
        reservation,
        scope,
        logs,
        process_id,
        &mut |_| poll(),
    )
}

pub(super) fn run_observed(
    command: Command,
    budget: Duration,
    reservation: &mut crate::coordination::Reservation,
    scope: Option<&str>,
    logs: &[std::path::PathBuf],
    process_id: &mut Option<u32>,
    poll: &mut dyn FnMut(u32) -> io::Result<()>,
) -> io::Result<Outcome> {
    run_with_observer(
        command,
        budget,
        reservation,
        scope,
        logs,
        ChildObservation {
            process_id,
            poll,
            lifecycle: None,
        },
    )
}

pub(super) fn run_with_observer(
    mut command: Command,
    budget: Duration,
    reservation: &mut crate::coordination::Reservation,
    scope: Option<&str>,
    logs: &[std::path::PathBuf],
    mut observation: ChildObservation<'_>,
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
    *observation.process_id = Some(child.id());
    let start = Instant::now();
    loop {
        let interruption = (observation.poll)(child.id()).err();
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
        let before_probe = Instant::now();
        if let Some(status) = child.try_wait()? {
            if let Some(lifecycle) = observation.lifecycle.as_deref_mut() {
                lifecycle.exit_observed = Some(Instant::now());
            }
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
        if let Some(lifecycle) = observation.lifecycle.as_deref_mut() {
            lifecycle.last_alive = Some(before_probe);
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

// Owned-lifecycle oracle: the poll holds the leader until /proc reports it as a
// zombie, so the next owned probe must observe exit and only earlier probes saw
// it live. Linux-only because the oracle reads the kernel task state.
#[cfg(all(test, target_os = "linux"))]
mod lifecycle_tests {
    use super::*;
    use std::path::PathBuf;

    fn state(pid: u32) -> Option<char> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        stat.rsplit_once(')')?.1.trim_start().chars().next()
    }

    fn root(name: &str) -> PathBuf {
        let root = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .join(format!("lifecycle-{name}-{}", std::process::id()));
        std::fs::create_dir_all(root.join("queue")).unwrap();
        root
    }

    #[test]
    fn exit_bounds_bracket_the_observed_zombie() {
        let root = root("exit");
        let release = root.join("release");
        let _ = std::fs::remove_file(&release);
        let mut reservation = crate::coordination::Reservation::at(
            root.join("queue"),
            "lifecycle test",
            Duration::ZERO,
            false,
        )
        .unwrap();
        let mut command = Command::new("sh");
        command
            .args(["-c", "while [ ! -e \"$1\" ]; do sleep 0.002; done", "sh"])
            .arg(&release);
        let mut polls: Vec<(Instant, Option<char>)> = Vec::new();
        let mut zombie_seen = None;
        let mut lifecycle = Lifecycle::default();
        let result = run_with_observer(
            command,
            Duration::from_secs(20),
            &mut reservation,
            None,
            &[],
            ChildObservation {
                process_id: &mut None,
                poll: &mut |pid| {
                    polls.push((Instant::now(), state(pid)));
                    if polls.len() == 8 {
                        std::fs::write(&release, b"").unwrap();
                        let deadline = Instant::now() + Duration::from_secs(10);
                        while state(pid) != Some('Z') && Instant::now() < deadline {
                            std::thread::sleep(Duration::from_millis(1));
                        }
                        zombie_seen = (state(pid) == Some('Z')).then(Instant::now);
                    }
                    Ok(())
                },
                lifecycle: Some(&mut lifecycle),
            },
        );
        let returned = Instant::now();
        assert!(result.expect("child completes").status.success());
        let zombie_seen = zombie_seen.expect("leader observed as a zombie before the final probe");
        assert_eq!(polls.len(), 8, "the probe after release must observe exit");
        assert!(polls.iter().all(|(_, s)| s.is_some_and(|c| c != 'Z')));
        let alive = lifecycle
            .last_alive
            .expect("live probes record a lower bound");
        let exit = lifecycle
            .exit_observed
            .expect("the owned exit probe records an upper bound");
        // The lower bound comes from the last live probe, not a stale or exited one.
        assert!(
            alive > polls[6].0 && alive < polls[7].0,
            "stale or late liveness bound"
        );
        // The upper bound follows the zombie the oracle observed, before return.
        assert!(alive < zombie_seen && zombie_seen < exit && exit < returned);
        reservation.reaped();
    }

    #[test]
    fn a_deadline_records_no_exit_observation() {
        let root = root("deadline");
        let mut reservation = crate::coordination::Reservation::at(
            root.join("queue"),
            "lifecycle test",
            Duration::ZERO,
            false,
        )
        .unwrap();
        let mut command = Command::new("sleep");
        command.arg("30");
        let mut lifecycle = Lifecycle::default();
        let began = Instant::now();
        let error = run_with_observer(
            command,
            Duration::from_millis(300),
            &mut reservation,
            None,
            &[],
            ChildObservation {
                process_id: &mut None,
                poll: &mut |_| Ok(()),
                lifecycle: Some(&mut lifecycle),
            },
        )
        .err()
        .expect("the deadline must fail");
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        let alive = lifecycle
            .last_alive
            .expect("live probes before the deadline");
        assert!(alive > began && alive < Instant::now());
        assert!(lifecycle.exit_observed.is_none());
    }
}
