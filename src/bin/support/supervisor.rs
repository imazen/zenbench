//! Foreground command supervision: process groups on Unix, Jobs on Windows.
//! Unix programs must not daemonize or leave their process group.
use process_wrap::std::*;
use std::io;
use std::process::{Command, ExitStatus};
use std::time::{Duration, Instant};

pub(super) fn run(
    mut command: Command,
    budget: Duration,
    reservation: &mut crate::coordination::Reservation,
    scope: Option<&str>,
) -> io::Result<ExitStatus> {
    reservation.command(&mut command);
    reservation.retain_until_reaped();
    let mut wrapped = StdCommandWrap::from(command);
    #[cfg(unix)]
    wrapped.wrap(ProcessGroup::leader());
    #[cfg(windows)]
    wrapped.wrap(JobObject);
    // A wrapper can fail after spawn; retain admission if no child handle
    // is available to prove cleanup.
    let mut child = wrapped.spawn()?;
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            #[cfg(unix)]
            {
                // A foreground program must join its workers. Kill residual
                // group members before allowing the next measurement.
                let _ = child.start_kill();
                wait_group_empty(child.id(), Duration::from_secs(5))?;
            }
            cleanup_scope(scope)?;
            reservation.reaped();
            return Ok(status);
        }
        if start.elapsed() >= budget {
            child.start_kill()?;
            child.wait()?;
            #[cfg(unix)]
            wait_group_empty(child.id(), Duration::from_secs(5))?;
            cleanup_scope(scope)?;
            reservation.reaped();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "child deadline exceeded; process group/job terminated",
            ));
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
