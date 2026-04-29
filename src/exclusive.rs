//! Cross-process exclusive benchmarking lock with heartbeat-based
//! diagnostics.
//!
//! Wraps fs4 advisory file locking with a small text sentinel that
//! records the holder's identity (project, binary, pid, hostname), the
//! benchmark currently running, and an estimated completion time. Other
//! processes blocked on the lock can `peek()` the file to print accurate
//! "waiting on <project>/<bench>, ETA in 4m" messages.
//!
//! On wasm32 the lock is a no-op — there is no filesystem and no
//! competing process to coordinate with.

#![cfg_attr(target_arch = "wasm32", allow(dead_code))]

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use super::{AcquireConfig, HolderInfo, Lock, LockInner, format_iso8601, parse_iso8601};
    use fs4::FileExt;
    use std::fs::OpenOptions;
    use std::io::{Read, Seek, SeekFrom, Write};
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{Duration, Instant, SystemTime};

    /// Default lock path when the caller doesn't override it.
    pub fn default_path() -> std::path::PathBuf {
        std::env::temp_dir().join("zenbench-exclusive.lock")
    }

    /// How often to refresh the "waiting on …" message while blocked.
    const WAIT_MESSAGE_FALLBACK: Duration = Duration::from_secs(15);

    /// Length the holder file is rounded up to. Padding makes the layout
    /// stable so an in-place rewrite never shrinks the file mid-read.
    const PAD_LEN: usize = 1024;

    pub fn acquire(cfg: AcquireConfig) -> std::io::Result<Lock> {
        let path = cfg.path.clone().unwrap_or_else(default_path);
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }

        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)?;

        // Try non-blocking first so we can print a "waiting on …" message
        // describing exactly who's holding the lock before we go to sleep.
        let waited = match FileExt::try_lock(&file) {
            Ok(()) => false,
            Err(fs4::TryLockError::WouldBlock) => {
                let interval = if cfg.waiting_interval.is_zero() {
                    WAIT_MESSAGE_FALLBACK
                } else {
                    cfg.waiting_interval
                };
                wait_with_messages(&file, &path, &cfg, interval)?;
                true
            }
            Err(fs4::TryLockError::Error(e)) => return Err(e),
        };

        // We hold the lock. Write our holder record.
        let now = SystemTime::now();
        let info = HolderInfo {
            pid: std::process::id(),
            hostname: hostname(),
            project: cfg.project.clone(),
            binary: cfg.binary.clone(),
            benchmark: cfg.benchmark.clone(),
            start: now,
            heartbeat: now,
            eta: cfg.estimated_duration.map(|d| now + d),
            activity: cfg.activity.clone(),
        };
        write_holder(&file, &info)?;

        if waited && !cfg.quiet {
            eprintln!(
                "[zenbench] lock acquired after waiting; running {}/{}",
                info.project, info.benchmark
            );
        }

        let inner = Arc::new(Mutex::new(LockInner { file, info }));
        let stop = Arc::new(AtomicBool::new(false));

        // Spawn the heartbeat thread. It just keeps the heartbeat
        // timestamp fresh so peekers can distinguish a live holder from
        // a leaked file.
        let heartbeat_period = if cfg.heartbeat.is_zero() {
            Duration::from_secs(5)
        } else {
            cfg.heartbeat
        };
        let hb_inner = Arc::clone(&inner);
        let hb_stop = Arc::clone(&stop);
        let handle = thread::Builder::new()
            .name("zenbench-exclusive-heartbeat".into())
            .spawn(move || heartbeat_loop(hb_inner, hb_stop, heartbeat_period))
            .ok();

        Ok(Lock {
            inner: Some(inner),
            stop,
            heartbeat_thread: handle,
            path,
        })
    }

    pub fn try_acquire(cfg: AcquireConfig) -> std::io::Result<Result<Lock, Option<HolderInfo>>> {
        let path = cfg.path.clone().unwrap_or_else(default_path);
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }

        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)?;

        match FileExt::try_lock(&file) {
            Ok(()) => {
                let now = SystemTime::now();
                let info = HolderInfo {
                    pid: std::process::id(),
                    hostname: hostname(),
                    project: cfg.project.clone(),
                    binary: cfg.binary.clone(),
                    benchmark: cfg.benchmark.clone(),
                    start: now,
                    heartbeat: now,
                    eta: cfg.estimated_duration.map(|d| now + d),
                    activity: cfg.activity.clone(),
                };
                write_holder(&file, &info)?;

                let inner = Arc::new(Mutex::new(LockInner { file, info }));
                let stop = Arc::new(AtomicBool::new(false));
                let heartbeat_period = if cfg.heartbeat.is_zero() {
                    Duration::from_secs(5)
                } else {
                    cfg.heartbeat
                };
                let hb_inner = Arc::clone(&inner);
                let hb_stop = Arc::clone(&stop);
                let handle = thread::Builder::new()
                    .name("zenbench-exclusive-heartbeat".into())
                    .spawn(move || heartbeat_loop(hb_inner, hb_stop, heartbeat_period))
                    .ok();

                Ok(Ok(Lock {
                    inner: Some(inner),
                    stop,
                    heartbeat_thread: handle,
                    path,
                }))
            }
            Err(fs4::TryLockError::WouldBlock) => {
                let info = read_holder(&path).ok().flatten();
                Ok(Err(info))
            }
            Err(fs4::TryLockError::Error(e)) => Err(e),
        }
    }

    pub fn peek(path: &Path) -> std::io::Result<Option<HolderInfo>> {
        if !path.exists() {
            return Ok(None);
        }
        read_holder(path)
    }

    pub fn drop_lock(lock: &mut Lock) {
        lock.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = lock.heartbeat_thread.take() {
            let _ = handle.join();
        }
        if let Some(inner) = lock.inner.take() {
            // Last reference: release fs4 lock when File drops.
            if let Ok(inner) = Arc::try_unwrap(inner) {
                let inner = inner.into_inner().unwrap_or_else(|p| p.into_inner());
                let _ = FileExt::unlock(&inner.file);
                drop(inner.file);
            }
        }
    }

    pub fn update_benchmark(lock: &Lock, benchmark: &str) {
        if let Some(inner_arc) = lock.inner.as_ref() {
            if let Ok(mut inner) = inner_arc.lock() {
                inner.info.benchmark = benchmark.to_string();
                let _ = write_holder(&inner.file, &inner.info);
            }
        }
    }

    pub fn update_eta(lock: &Lock, eta: SystemTime) {
        if let Some(inner_arc) = lock.inner.as_ref() {
            if let Ok(mut inner) = inner_arc.lock() {
                inner.info.eta = Some(eta);
                let _ = write_holder(&inner.file, &inner.info);
            }
        }
    }

    pub fn read_info(lock: &Lock) -> Option<HolderInfo> {
        lock.inner
            .as_ref()
            .and_then(|i| i.lock().ok().map(|g| g.info.clone()))
    }

    fn heartbeat_loop(inner: Arc<Mutex<LockInner>>, stop: Arc<AtomicBool>, period: Duration) {
        // Sleep in small slices so stop is honored quickly.
        let slice = Duration::from_millis(100);
        loop {
            let mut waited = Duration::ZERO;
            while waited < period {
                if stop.load(Ordering::SeqCst) {
                    return;
                }
                thread::sleep(slice);
                waited += slice;
            }
            if stop.load(Ordering::SeqCst) {
                return;
            }
            if let Ok(mut g) = inner.lock() {
                g.info.heartbeat = SystemTime::now();
                let _ = write_holder(&g.file, &g.info);
            }
        }
    }

    fn wait_with_messages(
        file: &std::fs::File,
        path: &Path,
        cfg: &AcquireConfig,
        message_interval: Duration,
    ) -> std::io::Result<()> {
        let deadline = cfg.timeout.map(|t| Instant::now() + t);
        let mut next_message = Instant::now();
        let poll = Duration::from_millis(200);
        loop {
            // Print "waiting on …" if we're due.
            if Instant::now() >= next_message && !cfg.quiet {
                if let Ok(Some(info)) = read_holder(path) {
                    eprintln!("[zenbench] {}", info.waiting_message(SystemTime::now()));
                } else {
                    eprintln!(
                        "[zenbench] waiting on zenbench-exclusive lock at {} (no holder info available)",
                        path.display()
                    );
                }
                next_message = Instant::now() + message_interval;
            }

            match FileExt::try_lock(file) {
                Ok(()) => return Ok(()),
                Err(fs4::TryLockError::WouldBlock) => {}
                Err(fs4::TryLockError::Error(e)) => return Err(e),
            }

            if let Some(d) = deadline {
                if Instant::now() >= d {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::WouldBlock,
                        "exclusive bench lock acquire timed out",
                    ));
                }
            }
            thread::sleep(poll);
        }
    }

    fn write_holder(file: &std::fs::File, info: &HolderInfo) -> std::io::Result<()> {
        // We hold the fs4 lock so we're the only writer. Rewrite the
        // file in place; pad to a fixed length so a peek that races a
        // rewrite always sees a complete record (older content tail is
        // overwritten with whitespace, never truncated away).
        let mut body = String::with_capacity(PAD_LEN);
        body.push_str("zenbench-exclusive v1\n");
        body.push_str(&format!("pid={}\n", info.pid));
        body.push_str(&format!("hostname={}\n", info.hostname));
        body.push_str(&format!("project={}\n", info.project));
        body.push_str(&format!("binary={}\n", info.binary));
        body.push_str(&format!("benchmark={}\n", info.benchmark));
        body.push_str(&format!("activity={}\n", info.activity));
        body.push_str(&format!("start={}\n", format_iso8601(info.start)));
        body.push_str(&format!("heartbeat={}\n", format_iso8601(info.heartbeat)));
        if let Some(eta) = info.eta {
            body.push_str(&format!("eta={}\n", format_iso8601(eta)));
        } else {
            body.push_str("eta=\n");
        }
        body.push_str("eof=1\n");

        // Pad with spaces so the on-disk size is stable.
        if body.len() < PAD_LEN {
            body.extend(std::iter::repeat_n(' ', PAD_LEN - body.len() - 1));
            body.push('\n');
        }

        let mut f = file;
        f.seek(SeekFrom::Start(0))?;
        f.write_all(body.as_bytes())?;
        f.flush()?;
        // We keep the file at PAD_LEN bytes; do not truncate.
        Ok(())
    }

    pub(super) fn read_holder(path: &Path) -> std::io::Result<Option<HolderInfo>> {
        // Open without locking — we want to peek at the live holder.
        let mut file = match OpenOptions::new().read(true).open(path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        let mut buf = String::new();
        file.read_to_string(&mut buf)?;

        // Validate the eof marker — an in-progress rewrite should still
        // contain it because we pad to fixed length, but a brand-new
        // empty file or a partial first write would not.
        if !buf.contains("eof=1") {
            return Ok(None);
        }

        let mut info = HolderInfo {
            pid: 0,
            hostname: String::new(),
            project: String::new(),
            binary: String::new(),
            benchmark: String::new(),
            activity: String::new(),
            start: SystemTime::UNIX_EPOCH,
            heartbeat: SystemTime::UNIX_EPOCH,
            eta: None,
        };
        let mut version_seen = false;
        for line in buf.lines() {
            let line = line.trim_end();
            if line == "zenbench-exclusive v1" {
                version_seen = true;
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            match k {
                "pid" => info.pid = v.parse().unwrap_or(0),
                "hostname" => info.hostname = v.to_string(),
                "project" => info.project = v.to_string(),
                "binary" => info.binary = v.to_string(),
                "benchmark" => info.benchmark = v.to_string(),
                "activity" => info.activity = v.to_string(),
                "start" => {
                    if let Some(t) = parse_iso8601(v) {
                        info.start = t;
                    }
                }
                "heartbeat" => {
                    if let Some(t) = parse_iso8601(v) {
                        info.heartbeat = t;
                    }
                }
                "eta" if !v.is_empty() => {
                    info.eta = parse_iso8601(v);
                }
                _ => {}
            }
        }
        if !version_seen {
            return Ok(None);
        }
        Ok(Some(info))
    }

    fn hostname() -> String {
        sysinfo::System::host_name()
            .or_else(|| std::env::var("HOSTNAME").ok())
            .or_else(|| std::env::var("COMPUTERNAME").ok())
            .unwrap_or_else(|| "unknown".into())
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use super::{AcquireConfig, HolderInfo, Lock};
    use std::path::{Path, PathBuf};
    use std::time::SystemTime;

    pub fn default_path() -> PathBuf {
        PathBuf::from("/zenbench-exclusive.lock")
    }
    pub fn acquire(_cfg: AcquireConfig) -> std::io::Result<Lock> {
        Ok(Lock { _priv: () })
    }
    pub fn try_acquire(_cfg: AcquireConfig) -> std::io::Result<Result<Lock, Option<HolderInfo>>> {
        Ok(Ok(Lock { _priv: () }))
    }
    pub fn peek(_path: &Path) -> std::io::Result<Option<HolderInfo>> {
        Ok(None)
    }
    pub fn drop_lock(_lock: &mut Lock) {}
    pub fn update_benchmark(_lock: &Lock, _benchmark: &str) {}
    pub fn update_eta(_lock: &Lock, _eta: SystemTime) {}
    pub fn read_info(_lock: &Lock) -> Option<HolderInfo> {
        None
    }
}

/// Configuration for [`Lock::acquire`] / [`Lock::try_acquire`].
#[derive(Clone, Debug)]
pub struct AcquireConfig {
    /// Path to the lock file. Defaults to
    /// `temp_dir()/zenbench-exclusive.lock` — a single well-known
    /// system-wide rendezvous so independent benchmark harnesses
    /// participate in the same mutex.
    pub path: Option<PathBuf>,
    /// Block at most this long before giving up. `None` blocks forever.
    pub timeout: Option<Duration>,
    /// How often the heartbeat thread refreshes the holder timestamp.
    /// Zero falls back to 5 seconds.
    pub heartbeat: Duration,
    /// How often to re-print the "waiting on …" message while blocked.
    /// Zero falls back to 15 seconds.
    pub waiting_interval: Duration,
    /// Suppress the "waiting on …" / "lock acquired" stderr messages.
    pub quiet: bool,
    /// User-facing project name (e.g. cargo crate name).
    pub project: String,
    /// User-facing binary name (e.g. cargo bench name).
    pub binary: String,
    /// Benchmark currently running. Update via [`Lock::update_benchmark`]
    /// as the run progresses through groups.
    pub benchmark: String,
    /// Free-form activity description (shown in `peek()` output).
    pub activity: String,
    /// Estimated wall-clock duration of the run. Stored as `eta` in the
    /// holder file so peekers can render "ETA in 3m". Update with
    /// [`Lock::update_eta`] once you have a better estimate.
    pub estimated_duration: Option<Duration>,
}

impl Default for AcquireConfig {
    fn default() -> Self {
        Self {
            path: None,
            timeout: None,
            heartbeat: Duration::from_secs(5),
            waiting_interval: Duration::from_secs(15),
            quiet: false,
            project: detect_project(),
            binary: detect_binary(),
            benchmark: String::new(),
            activity: String::new(),
            estimated_duration: None,
        }
    }
}

/// Information about whoever currently holds (or last held) the lock.
#[derive(Clone, Debug)]
pub struct HolderInfo {
    pub pid: u32,
    pub hostname: String,
    pub project: String,
    pub binary: String,
    pub benchmark: String,
    pub activity: String,
    pub start: SystemTime,
    pub heartbeat: SystemTime,
    pub eta: Option<SystemTime>,
}

impl HolderInfo {
    /// True if the most recent heartbeat is older than `threshold` ago.
    /// Stale info is *not* a license to break the lock — fs4 already
    /// handles crashed holders. Stale heartbeat with the lock still
    /// held means a hung-but-alive process (debugger, kernel hang).
    pub fn is_stale(&self, threshold: Duration, now: SystemTime) -> bool {
        match now.duration_since(self.heartbeat) {
            Ok(d) => d > threshold,
            Err(_) => false,
        }
    }

    /// Format a "waiting on …" message that names the project, the
    /// benchmark in flight, and an ETA if known.
    pub fn waiting_message(&self, now: SystemTime) -> String {
        let project = if self.project.is_empty() {
            "<unknown project>".to_string()
        } else {
            self.project.clone()
        };
        let bench = if self.benchmark.is_empty() {
            "<benchmark unset>".to_string()
        } else {
            self.benchmark.clone()
        };

        let mut s = format!(
            "waiting on exclusive bench lock — {project}/{bench} (pid {pid}",
            project = project,
            bench = bench,
            pid = self.pid,
        );
        if !self.binary.is_empty() && self.binary != self.project {
            s.push_str(&format!(", binary {}", self.binary));
        }
        if let Ok(d) = now.duration_since(self.start) {
            s.push_str(&format!(", running {}", format_duration(d)));
        }
        if let Some(eta) = self.eta {
            match eta.duration_since(now) {
                Ok(d) if !d.is_zero() => s.push_str(&format!(", ETA in {}", format_duration(d))),
                Ok(_) => s.push_str(", ETA reached"),
                Err(_) => s.push_str(", ETA passed"),
            }
        }
        if let Ok(d) = now.duration_since(self.heartbeat) {
            if d > Duration::from_secs(15) {
                s.push_str(&format!(
                    ", last heartbeat {} ago — possibly hung",
                    format_duration(d)
                ));
            }
        }
        s.push(')');
        s
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct LockInner {
    file: std::fs::File,
    info: HolderInfo,
}

/// Cross-process exclusive bench lock. Drop releases.
pub struct Lock {
    #[cfg(not(target_arch = "wasm32"))]
    inner: Option<std::sync::Arc<std::sync::Mutex<LockInner>>>,
    #[cfg(not(target_arch = "wasm32"))]
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    #[cfg(not(target_arch = "wasm32"))]
    heartbeat_thread: Option<std::thread::JoinHandle<()>>,
    #[cfg(not(target_arch = "wasm32"))]
    path: PathBuf,
    #[cfg(target_arch = "wasm32")]
    _priv: (),
}

impl Lock {
    /// Block until the lock is acquired (subject to `cfg.timeout`).
    /// Prints periodic "waiting on …" messages to stderr while blocked,
    /// each describing the current holder and ETA.
    pub fn acquire(cfg: AcquireConfig) -> std::io::Result<Self> {
        imp::acquire(cfg)
    }

    /// Try to acquire without blocking. On success, returns `Ok(Lock)`.
    /// If another process holds it, returns `Ok(Err(Some(info)))` with
    /// the holder's record. `Ok(Err(None))` means the lock was held but
    /// the holder file was unreadable.
    pub fn try_acquire(cfg: AcquireConfig) -> std::io::Result<Result<Self, Option<HolderInfo>>> {
        imp::try_acquire(cfg)
    }

    /// Read the current holder's record without taking the lock.
    /// Returns `Ok(None)` if the file is absent, empty, or
    /// mid-rewrite — the file format is self-validating with an
    /// `eof=1` sentinel.
    pub fn peek(path: &Path) -> std::io::Result<Option<HolderInfo>> {
        imp::peek(path)
    }

    /// Default lock path for the host. Use this if you want to construct
    /// a custom config that still rendezvouses on the system-wide path.
    pub fn default_path() -> PathBuf {
        imp::default_path()
    }

    /// Update the `benchmark=` field in the holder file. Call this from
    /// the engine as it transitions between bench groups.
    pub fn update_benchmark(&self, benchmark: &str) {
        imp::update_benchmark(self, benchmark)
    }

    /// Set a new ETA. Useful once the engine has measured a few groups
    /// and can extrapolate completion.
    pub fn update_eta(&self, eta: SystemTime) {
        imp::update_eta(self, eta)
    }

    /// Snapshot of the current in-memory holder record.
    pub fn info(&self) -> Option<HolderInfo> {
        imp::read_info(self)
    }

    /// Filesystem path of the lock file.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        imp::drop_lock(self);
    }
}

fn detect_project() -> String {
    std::env::var("CARGO_PKG_NAME").unwrap_or_else(|_| String::new())
}

fn detect_binary() -> String {
    std::env::var("CARGO_BIN_NAME")
        .or_else(|_| std::env::var("CARGO_CRATE_NAME"))
        .ok()
        .or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|p| p.file_name().map(|s| s.to_string_lossy().into_owned()))
        })
        .unwrap_or_default()
}

fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m{:02}s", secs / 60, secs % 60)
    } else {
        format!("{}h{:02}m", secs / 3600, (secs % 3600) / 60)
    }
}

fn format_iso8601(t: SystemTime) -> String {
    let secs = t
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    seconds_to_iso8601(secs)
}

fn parse_iso8601(s: &str) -> Option<SystemTime> {
    // Format: YYYY-MM-DDTHH:MM:SSZ
    let bytes = s.as_bytes();
    if bytes.len() != 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'Z'
    {
        return None;
    }
    let parse = |start: usize, end: usize| -> Option<u64> {
        std::str::from_utf8(&bytes[start..end])
            .ok()?
            .parse::<u64>()
            .ok()
    };
    let y = parse(0, 4)?;
    let mo = parse(5, 7)?;
    let d = parse(8, 10)?;
    let h = parse(11, 13)?;
    let mi = parse(14, 16)?;
    let se = parse(17, 19)?;
    let secs = iso_to_secs(y, mo, d, h, mi, se)?;
    Some(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
}

fn seconds_to_iso8601(secs: u64) -> String {
    let days = secs / 86400;
    let rem = secs % 86400;
    let hours = rem / 3600;
    let minutes = (rem % 3600) / 60;
    let seconds = rem % 60;

    let mut y: u64 = 1970;
    let mut d = days;
    loop {
        let yd = if is_leap(y) { 366 } else { 365 };
        if d < yd {
            break;
        }
        d -= yd;
        y += 1;
    }
    let month_days: [u64; 12] = if is_leap(y) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut m = 1u64;
    for &md in &month_days {
        if d < md {
            break;
        }
        d -= md;
        m += 1;
    }
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y,
        m,
        d + 1,
        hours,
        minutes,
        seconds
    )
}

fn iso_to_secs(y: u64, mo: u64, d: u64, h: u64, mi: u64, se: u64) -> Option<u64> {
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h >= 24 || mi >= 60 || se >= 60 {
        return None;
    }
    let mut days: u64 = 0;
    for yy in 1970..y {
        days += if is_leap(yy) { 366 } else { 365 };
    }
    let month_days: [u64; 12] = if is_leap(y) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    for &md in month_days.iter().take((mo - 1) as usize) {
        days += md;
    }
    days += d - 1;
    Some(days * 86400 + h * 3600 + mi * 60 + se)
}

fn is_leap(y: u64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    fn temp_lock_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "zenbench-exclusive-test-{}-{}-{}.lock",
            label,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ))
    }

    fn cfg(path: &Path) -> AcquireConfig {
        AcquireConfig {
            path: Some(path.to_path_buf()),
            quiet: true,
            project: "zenbench".into(),
            binary: "test".into(),
            benchmark: "initial".into(),
            heartbeat: Duration::from_millis(50),
            waiting_interval: Duration::from_millis(50),
            ..Default::default()
        }
    }

    #[test]
    fn acquire_release_roundtrip() {
        let p = temp_lock_path("rt");
        {
            let lock = Lock::acquire(cfg(&p)).expect("acquire");
            let info = Lock::peek(&p).expect("peek").expect("peek some");
            assert_eq!(info.project, "zenbench");
            assert_eq!(info.benchmark, "initial");
            drop(lock);
        }
        // Lock released — re-acquire instantly.
        let lock2 = Lock::acquire(cfg(&p)).expect("reacquire");
        drop(lock2);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn try_acquire_returns_holder_info_when_held() {
        let p = temp_lock_path("try");
        let _held = Lock::acquire(cfg(&p)).expect("acquire 1");
        let mut cfg2 = cfg(&p);
        cfg2.benchmark = "second".into();
        match Lock::try_acquire(cfg2).expect("try_acquire") {
            Ok(_) => panic!("expected lock to be held"),
            Err(Some(info)) => {
                assert_eq!(info.benchmark, "initial");
                assert_eq!(info.pid, std::process::id());
            }
            Err(None) => panic!("expected holder info, got none"),
        }
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn update_benchmark_reflected_in_peek() {
        let p = temp_lock_path("upd");
        let lock = Lock::acquire(cfg(&p)).expect("acquire");
        lock.update_benchmark("phase-2");
        let info = Lock::peek(&p).expect("peek").expect("peek some");
        assert_eq!(info.benchmark, "phase-2");
        drop(lock);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn heartbeat_advances() {
        let p = temp_lock_path("hb");
        let lock = Lock::acquire(cfg(&p)).expect("acquire");
        let first = Lock::peek(&p).unwrap().unwrap().heartbeat;
        // Sentinel file timestamps are ISO-8601 with second precision,
        // so we must sleep across at least one second boundary.
        std::thread::sleep(Duration::from_millis(1200));
        let later = Lock::peek(&p).unwrap().unwrap().heartbeat;
        assert!(
            later > first,
            "heartbeat should advance: first={first:?} later={later:?}"
        );
        drop(lock);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn eta_round_trip() {
        let p = temp_lock_path("eta");
        let mut c = cfg(&p);
        c.estimated_duration = Some(Duration::from_secs(120));
        let lock = Lock::acquire(c).expect("acquire");
        let info = Lock::peek(&p).unwrap().unwrap();
        assert!(info.eta.is_some());
        drop(lock);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn waiting_message_includes_project_and_eta() {
        let now = SystemTime::now();
        let info = HolderInfo {
            pid: 999,
            hostname: "h".into(),
            project: "myproj".into(),
            binary: "bench".into(),
            benchmark: "sort_1k".into(),
            activity: "".into(),
            start: now - Duration::from_secs(45),
            heartbeat: now,
            eta: Some(now + Duration::from_secs(180)),
        };
        let msg = info.waiting_message(now);
        assert!(msg.contains("myproj/sort_1k"), "msg: {msg}");
        assert!(msg.contains("pid 999"));
        assert!(msg.contains("ETA in 3m00s"), "msg: {msg}");
    }

    #[test]
    fn iso_round_trip() {
        let now_secs = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let s = seconds_to_iso8601(now_secs);
        let parsed = parse_iso8601(&s).expect("parse");
        let parsed_secs = parsed
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert_eq!(now_secs, parsed_secs);
    }
}
