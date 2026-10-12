//! Cooperative FIFO admission. The directory is a local-machine trust boundary.
//! Running reservations left by a crashed owner fail closed: its children may live.
#![cfg(not(target_arch = "wasm32"))]

use fs4::FileExt;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const LEASE: &str = "ZENBENCH_COORDINATION_LEASE";

#[derive(Serialize, Deserialize)]
struct Ticket {
    sequence: u64,
    pid: u32,
    start: u64,
    activity: String,
    running: bool,
}

pub(crate) struct Reservation {
    root: PathBuf,
    sequence: u64,
    guard: Option<File>,
    retain: bool,
}

fn open(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
}

pub(crate) fn directory() -> io::Result<PathBuf> {
    if let Some(value) = std::env::var_os("ZENBENCH_COORDINATION_DIR") {
        let path = PathBuf::from(value);
        if !path.is_absolute() || !path.is_dir() {
            return Err(io::Error::other(
                "ZENBENCH_COORDINATION_DIR must name an existing absolute local directory shared by all participants",
            ));
        }
        Ok(path)
    } else {
        let root = crate::paths::user_cache_dir().join("admission-v1");
        fs::create_dir_all(&root)?;
        Ok(root)
    }
}

fn process_table() -> sysinfo::System {
    let mut system = sysinfo::System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    system
}

impl Reservation {
    pub(crate) fn acquire(activity: &str, timeout: Duration) -> io::Result<Self> {
        Self::at(directory()?, activity, timeout, true)
    }

    fn at(root: PathBuf, activity: &str, timeout: Duration, inherit: bool) -> io::Result<Self> {
        let started = Instant::now();
        if inherit {
            if let Some(value) = std::env::var_os(LEASE) {
                let sequence = value
                    .to_str()
                    .and_then(|s| s.parse::<u64>().ok())
                    .ok_or_else(|| io::Error::other("invalid inherited admission reservation"))?;
                let record: Ticket =
                    serde_json::from_slice(&fs::read(root.join(format!("{sequence}.json")))?)?;
                let guard = open(&root.join(format!("{sequence}.guard")))?;
                if !matches!(
                    FileExt::try_lock(&guard),
                    Err(fs4::TryLockError::WouldBlock)
                ) || !record.running
                {
                    return Err(io::Error::other(
                        "inherited admission reservation is not held",
                    ));
                }
                let system = process_table();
                let mut cursor = sysinfo::Pid::from_u32(std::process::id());
                let mut ancestor = false;
                for _ in 0..256 {
                    let Some(process) = system.process(cursor) else {
                        break;
                    };
                    if cursor.as_u32() == record.pid && process.start_time() == record.start {
                        ancestor = true;
                        break;
                    }
                    let Some(parent) = process.parent() else {
                        break;
                    };
                    cursor = parent;
                }
                if !ancestor {
                    return Err(io::Error::other("admission owner is not a live ancestor"));
                }
                return Ok(Self {
                    root,
                    sequence,
                    guard: None,
                    retain: false,
                });
            }
        }
        let control = open(&root.join("queue.lock"))?;
        // The control mutex only protects small filesystem transactions. Its
        // acquisition is bounded too, including an abandoned/stalled client.
        lock_until(&control, started, timeout)?;
        let counter = root.join("sequence");
        let previous = match fs::read_to_string(&counter) {
            Ok(s) => s.parse::<u64>().map_err(io::Error::other)?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => 0,
            Err(e) => return Err(e),
        };
        let sequence = previous
            .checked_add(1)
            .ok_or_else(|| io::Error::other("admission sequence exhausted"))?;
        fs::write(&counter, sequence.to_string())?;
        let guard = open(&root.join(format!("{sequence}.guard")))?;
        FileExt::lock(&guard)?;
        let system = process_table();
        let start = system
            .process(sysinfo::Pid::from_u32(std::process::id()))
            .ok_or_else(|| io::Error::other("cannot identify admission owner"))?
            .start_time();
        let mut record = Ticket {
            sequence,
            pid: std::process::id(),
            start,
            activity: activity.into(),
            running: false,
        };
        let record_path = root.join(format!("{sequence}.json"));
        fs::write(&record_path, serde_json::to_vec(&record)?)?;
        let mut reservation = Self {
            root,
            sequence,
            guard: Some(guard),
            retain: false,
        };
        FileExt::unlock(&control)?;
        let mut announced = false;
        loop {
            lock_until(&control, started, timeout)?;
            let mut blocked = None;
            for entry in fs::read_dir(&reservation.root)? {
                let path = entry?.path();
                if path.extension().and_then(|s| s.to_str()) != Some("json") {
                    continue;
                }
                let bytes = match fs::read(&path) {
                    Ok(bytes) => bytes,
                    Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                    Err(e) => return Err(e),
                };
                let ticket: Ticket = serde_json::from_slice(&bytes)?;
                if ticket.sequence >= sequence {
                    continue;
                }
                let owner_guard =
                    open(&reservation.root.join(format!("{}.guard", ticket.sequence)))?;
                match FileExt::try_lock(&owner_guard) {
                    Ok(()) if !ticket.running => {
                        fs::remove_file(path)?;
                    }
                    Ok(()) => {
                        if !path.exists() {
                            continue;
                        } // owner completed during scan
                        return Err(io::Error::other(format!(
                            "orphaned running reservation {} ({}); verify all its work has stopped before removing {}",
                            ticket.sequence,
                            ticket.activity,
                            path.display()
                        )));
                    }
                    Err(fs4::TryLockError::WouldBlock) => {
                        blocked = Some(ticket);
                    }
                    Err(fs4::TryLockError::Error(e)) => return Err(e),
                }
            }
            if blocked.is_none() {
                record.running = true;
                fs::write(&record_path, serde_json::to_vec(&record)?)?;
                FileExt::unlock(&control)?;
                // Once admitted, a crash must leave a running record. Normal
                // drop cleans it; launchers explicitly retain until reaped.
                reservation.retain = false;
                return Ok(reservation);
            }
            FileExt::unlock(&control)?;
            if !announced {
                let owner = blocked.unwrap();
                eprintln!(
                    "[zenbench] admission queued behind #{} {} (pid {}); directory={}",
                    owner.sequence,
                    owner.activity,
                    owner.pid,
                    reservation.root.display()
                );
                announced = true;
            }
            if started.elapsed() >= timeout {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "admission deadline expired; task not started",
                ));
            }
            std::thread::sleep(
                Duration::from_millis(50).min(timeout.saturating_sub(started.elapsed())),
            );
        }
    }

    pub(crate) fn command(&self, command: &mut std::process::Command) {
        command
            .env("ZENBENCH_COORDINATION_DIR", &self.root)
            .env(LEASE, self.sequence.to_string());
    }

    pub(crate) fn retain_until_reaped(&mut self) {
        self.retain = true;
    }
    pub(crate) fn reaped(&mut self) {
        self.retain = false;
    }
}

fn lock_until(file: &File, start: Instant, timeout: Duration) -> io::Result<()> {
    loop {
        match FileExt::try_lock(file) {
            Ok(()) => return Ok(()),
            Err(fs4::TryLockError::Error(e)) => return Err(e),
            Err(fs4::TryLockError::WouldBlock) => {}
        }
        if start.elapsed() >= timeout {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "admission control deadline expired",
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if self.guard.is_some() && !self.retain {
            // A ticket number is never reused. Remove the record before closing
            // the guard so a scanner cannot mistake completion for an orphan.
            let _ = fs::remove_file(self.root.join(format!("{}.json", self.sequence)));
            drop(self.guard.take());
            let _ = fs::remove_file(self.root.join(format!("{}.guard", self.sequence)));
        }
    }
}

/// Queued owners are idle by protocol. Exclude only their benchmark-name match;
/// never subtract their CPU consumption from resource observations.
pub(crate) fn queued_pids(system: &sysinfo::System) -> Vec<sysinfo::Pid> {
    let Ok(root) = directory() else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(&root) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            if entry.path().extension()?.to_str()? != "json" {
                return None;
            }
            let ticket: Ticket = serde_json::from_slice(&fs::read(entry.path()).ok()?).ok()?;
            if ticket.running {
                return None;
            }
            let pid = sysinfo::Pid::from_u32(ticket.pid);
            if system.process(pid)?.start_time() != ticket.start {
                return None;
            }
            let guard = open(&root.join(format!("{}.guard", ticket.sequence))).ok()?;
            matches!(
                FileExt::try_lock(&guard),
                Err(fs4::TryLockError::WouldBlock)
            )
            .then_some(pid)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn root(name: &str) -> PathBuf {
        let path = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .join(format!("admission-{name}-{}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        path
    }
    #[test]
    fn live_owner_blocks_then_releases() {
        let root = root("live");
        let first = Reservation::at(root.clone(), "first", Duration::ZERO, false).unwrap();
        assert!(Reservation::at(root.clone(), "second", Duration::ZERO, false).is_err());
        drop(first);
        drop(Reservation::at(root.clone(), "third", Duration::ZERO, false).unwrap());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn abandoned_running_owner_never_becomes_permission() {
        let root = root("orphan");
        let mut first =
            Reservation::at(root.clone(), "external child", Duration::ZERO, false).unwrap();
        first.retain_until_reaped();
        drop(first);
        let error = Reservation::at(root.clone(), "benchmark", Duration::ZERO, false)
            .err()
            .unwrap();
        assert!(error.to_string().contains("orphaned running"));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn registered_waiters_are_admitted_in_fifo_order() {
        let root = root("fifo");
        let first = Reservation::at(root.clone(), "owner", Duration::ZERO, false).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let mut handles = Vec::new();
        for n in 0..2 {
            let dir = root.clone();
            let tx = tx.clone();
            handles.push(std::thread::spawn(move || {
                let _reservation =
                    Reservation::at(dir, "waiter", Duration::from_secs(10), false).unwrap();
                tx.send(n).unwrap();
            }));
            let start = Instant::now();
            while !root.join(format!("{}.json", n + 2)).exists() {
                assert!(start.elapsed() < Duration::from_secs(5));
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        assert!(rx.try_recv().is_err());
        drop(first);
        assert_eq!(rx.recv_timeout(Duration::from_secs(10)).unwrap(), 0);
        assert_eq!(rx.recv_timeout(Duration::from_secs(10)).unwrap(), 1);
        for handle in handles {
            handle.join().unwrap();
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn abandoned_queue_entry_is_pruned() {
        let root = root("queued");
        fs::write(root.join("sequence"), "1").unwrap();
        fs::write(
            root.join("1.json"),
            serde_json::to_vec(&Ticket {
                sequence: 1,
                pid: 0,
                start: 0,
                activity: "dead waiter".into(),
                running: false,
            })
            .unwrap(),
        )
        .unwrap();
        drop(Reservation::at(root.clone(), "benchmark", Duration::ZERO, false).unwrap());
        assert!(!root.join("1.json").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
