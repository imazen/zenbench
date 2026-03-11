//! Fire-and-forget benchmark daemon.
//!
//! Spawns benchmark processes that run independently, write results to disk,
//! and can be monitored/killed via the CLI or MCP.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// State file written by the daemon process.
const STATE_DIR: &str = ".zenbench";
const RUNS_DIR: &str = "runs";
#[allow(dead_code)]
const LOCK_FILE: &str = "bench.lock";

/// Status of a benchmark run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RunStatus {
    /// Queued but not started.
    Queued,
    /// Currently running.
    Running,
    /// Completed successfully.
    Completed,
    /// Failed with error.
    Failed(String),
    /// Killed (by user or auto-kill).
    Killed,
}

/// Metadata for a benchmark run (written to disk as JSON).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunState {
    pub id: String,
    pub pid: u32,
    pub git_hash: Option<String>,
    pub command: String,
    pub status: RunStatus,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    pub result_path: Option<PathBuf>,
}

impl RunState {
    /// Create a new run state for a process about to start.
    pub fn new(id: String, command: String, git_hash: Option<String>) -> Self {
        let started_at = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            id,
            pid: std::process::id(),
            git_hash,
            command,
            status: RunStatus::Queued,
            started_at,
            finished_at: None,
            result_path: None,
        }
    }
}

/// The runs directory for storing daemon state.
pub fn runs_dir(project_root: &Path) -> PathBuf {
    project_root.join(STATE_DIR).join(RUNS_DIR)
}

/// Path to the cross-process lock file.
pub fn lock_path(project_root: &Path) -> PathBuf {
    project_root.join(STATE_DIR).join(LOCK_FILE)
}

/// List all known runs, with automatic state reconciliation.
///
/// Dead processes with result files are marked Completed.
/// Dead processes without result files are marked Failed.
pub fn list_runs(project_root: &Path) -> std::io::Result<Vec<RunState>> {
    let dir = runs_dir(project_root);
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut runs = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json")
            && !path
                .file_name()
                .is_some_and(|n| n.to_string_lossy().contains(".results."))
        {
            if let Ok(data) = std::fs::read_to_string(&path) {
                if let Ok(mut state) = serde_json::from_str::<RunState>(&data) {
                    // Reconcile: detect processes that finished without updating state
                    if reconcile_state(project_root, &mut state) {
                        // Save the updated state back to disk
                        let _ = save_run_state(project_root, &state);
                    }
                    runs.push(state);
                }
            }
        }
    }

    runs.sort_by_key(|r| r.started_at);
    Ok(runs)
}

/// Save run state to disk.
pub fn save_run_state(project_root: &Path, state: &RunState) -> std::io::Result<()> {
    let dir = runs_dir(project_root);
    std::fs::create_dir_all(&dir)?;

    let path = dir.join(format!("{}.json", state.id));
    let json = serde_json::to_string_pretty(state).map_err(std::io::Error::other)?;
    std::fs::write(path, json)
}

/// Load a specific run state, with automatic reconciliation.
pub fn load_run_state(project_root: &Path, run_id: &str) -> std::io::Result<RunState> {
    let path = runs_dir(project_root).join(format!("{run_id}.json"));
    let data = std::fs::read_to_string(path)?;
    let mut state: RunState = serde_json::from_str(&data).map_err(std::io::Error::other)?;

    if reconcile_state(project_root, &mut state) {
        let _ = save_run_state(project_root, &state);
    }

    Ok(state)
}

/// Reconcile a run's status with reality.
///
/// If the run is marked Running/Queued but the process is dead,
/// check if results exist to determine Completed vs Failed.
/// Returns true if the state was modified.
fn reconcile_state(project_root: &Path, state: &mut RunState) -> bool {
    if state.status != RunStatus::Running && state.status != RunStatus::Queued {
        return false;
    }

    if is_process_alive(state.pid) {
        return false;
    }

    // Process is dead — figure out the outcome
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let has_results = state
        .result_path
        .as_ref()
        .is_some_and(|p| p.exists() && std::fs::metadata(p).is_ok_and(|m| m.len() > 10));

    if has_results {
        state.status = RunStatus::Completed;
    } else {
        // Check if stderr log exists and has error info
        let stderr_path = runs_dir(project_root).join(format!("{}.stderr.log", state.id));
        let error_msg = if stderr_path.exists() {
            // Read last 500 bytes for error context
            std::fs::read_to_string(&stderr_path)
                .ok()
                .and_then(|s| {
                    let trimmed = s.trim();
                    if trimmed.is_empty() {
                        None
                    } else {
                        let tail: String = trimmed
                            .chars()
                            .rev()
                            .take(500)
                            .collect::<String>()
                            .chars()
                            .rev()
                            .collect();
                        Some(tail)
                    }
                })
                .unwrap_or_else(|| "process exited without results".to_string())
        } else {
            "process exited without results".to_string()
        };
        state.status = RunStatus::Failed(error_msg);
    }
    state.finished_at = Some(now);
    true
}

/// Kill stale runs that were started with a different git hash.
///
/// Returns the number of runs killed.
pub fn kill_stale_runs(project_root: &Path, current_hash: &str) -> std::io::Result<usize> {
    let runs = list_runs(project_root)?;
    let mut killed = 0;

    for run in runs {
        if run.status == RunStatus::Running || run.status == RunStatus::Queued {
            let is_stale = run.git_hash.as_deref().is_some_and(|h| h != current_hash);

            if is_stale {
                // Try to kill the process
                if kill_process(run.pid) {
                    let mut updated = run;
                    updated.status = RunStatus::Killed;
                    updated.finished_at = Some(
                        SystemTime::now()
                            .duration_since(SystemTime::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                    );
                    save_run_state(project_root, &updated)?;
                    killed += 1;
                }
            }
        }
    }

    Ok(killed)
}

/// Kill a specific run by ID.
pub fn kill_run(project_root: &Path, run_id: &str) -> std::io::Result<bool> {
    let mut state = load_run_state(project_root, run_id)?;
    if (state.status == RunStatus::Running || state.status == RunStatus::Queued)
        && kill_process(state.pid)
    {
        state.status = RunStatus::Killed;
        state.finished_at = Some(
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        );
        save_run_state(project_root, &state)?;
        return Ok(true);
    }
    Ok(false)
}

/// Attempt to kill a process by PID. Returns true if the signal was sent.
fn kill_process(pid: u32) -> bool {
    // Cross-platform process kill
    #[cfg(unix)]
    {
        use std::process::Command;
        Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .status()
            .is_ok_and(|s| s.success())
    }

    #[cfg(windows)]
    {
        use std::process::Command;
        Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .status()
            .is_ok_and(|s| s.success())
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        false
    }
}

/// Check if a process is still running (not a zombie, not dead).
pub fn is_process_alive(pid: u32) -> bool {
    #[cfg(target_os = "linux")]
    {
        // On Linux, check /proc/<pid>/stat to detect zombies.
        // kill -0 succeeds for zombies, so we can't rely on it alone.
        let stat_path = format!("/proc/{pid}/stat");
        match std::fs::read_to_string(&stat_path) {
            Ok(stat) => {
                // Format: "pid (comm) state ..."
                // The state field is after the closing paren of comm
                if let Some(pos) = stat.rfind(')') {
                    let after = stat[pos + 1..].trim_start();
                    // State 'Z' = zombie, 'X'/'x' = dead
                    !after.starts_with('Z') && !after.starts_with('X') && !after.starts_with('x')
                } else {
                    false
                }
            }
            Err(_) => false, // /proc/<pid> doesn't exist → process is gone
        }
    }

    #[cfg(all(unix, not(target_os = "linux")))]
    {
        use std::process::Command;
        // On macOS/BSD, use ps to check state. 'Z' = zombie.
        let output = Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "state="])
            .output();
        match output {
            Ok(o) if o.status.success() => {
                let state = String::from_utf8_lossy(&o.stdout);
                let state = state.trim();
                !state.is_empty() && !state.starts_with('Z')
            }
            _ => false,
        }
    }

    #[cfg(windows)]
    {
        use std::process::Command;
        let output = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output();
        output.is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        false
    }
}

/// Clean up finished runs older than the given age.
pub fn cleanup_old_runs(project_root: &Path, max_age_secs: u64) -> std::io::Result<usize> {
    let runs = list_runs(project_root)?;
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let mut cleaned = 0;
    for run in runs {
        if run.status != RunStatus::Running
            && run.status != RunStatus::Queued
            && now.saturating_sub(run.started_at) > max_age_secs
        {
            let state_path = runs_dir(project_root).join(format!("{}.json", run.id));
            let stderr_path = runs_dir(project_root).join(format!("{}.stderr.log", run.id));
            let _ = std::fs::remove_file(state_path);
            let _ = std::fs::remove_file(stderr_path);
            // Optionally remove the results file too
            if let Some(ref result_path) = run.result_path {
                let _ = std::fs::remove_file(result_path);
            }
            cleaned += 1;
        }
    }

    Ok(cleaned)
}

/// Spawn a benchmark binary as a detached background process.
///
/// The benchmark is expected to be a `cargo bench` target or a standalone binary
/// that writes results to a JSON file. The daemon records the run state and
/// auto-kills stale runs from previous git commits.
///
/// Returns the run ID and the Child handle. The caller SHOULD call
/// `child.wait()` to prevent zombie processes. If the caller exits
/// without waiting, the OS will reparent and reap the zombie.
pub fn spawn_detached(
    project_root: &Path,
    command: &str,
    args: &[&str],
) -> std::io::Result<(String, std::process::Child)> {
    let git_hash = crate::platform::git_commit_hash();

    // Auto-kill stale runs before spawning
    if let Some(ref hash) = git_hash {
        let killed = kill_stale_runs(project_root, hash)?;
        if killed > 0 {
            eprintln!("[zenbench] killed {killed} stale run(s) from previous commits");
        }
    }

    let run_id = format!(
        "{}-{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        std::process::id()
    );

    // Build absolute paths so they work regardless of cwd
    let abs_project = std::fs::canonicalize(project_root)?;
    let result_dir = runs_dir(&abs_project);
    std::fs::create_dir_all(&result_dir)?;
    let result_path = result_dir.join(format!("{run_id}.results.json"));
    let stderr_path = result_dir.join(format!("{run_id}.stderr.log"));

    // Open stderr log file for the child process
    let stderr_file = std::fs::File::create(&stderr_path)?;

    // Spawn the process with stderr going to a log file (not a pipe!)
    // Piping stderr and not consuming it causes the child to hang when the buffer fills.
    let child = std::process::Command::new(command)
        .args(args)
        .current_dir(&abs_project)
        .env("ZENBENCH_RUN_ID", &run_id)
        .env("ZENBENCH_RESULT_PATH", &result_path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::from(stderr_file))
        .spawn()?;

    let mut state = RunState::new(
        run_id.clone(),
        format!("{command} {}", args.join(" ")),
        git_hash,
    );
    state.pid = child.id();
    state.status = RunStatus::Running;
    state.result_path = Some(result_path);
    save_run_state(&abs_project, &state)?;

    eprintln!("[zenbench] spawned run {run_id} (PID {})", child.id());
    Ok((run_id, child))
}

/// Spawn a benchmark and discard the child handle (fire-and-forget).
///
/// Use this when you don't need to wait for the process.
/// The zombie will be reaped when this process exits.
pub fn spawn_fire_and_forget(
    project_root: &Path,
    command: &str,
    args: &[&str],
) -> std::io::Result<String> {
    let (run_id, _child) = spawn_detached(project_root, command, args)?;
    Ok(run_id)
}

/// Wait for a specific run to complete.
///
/// Polls the process at the given interval. Returns the final RunState.
/// Times out after `timeout` duration.
pub fn wait_for_run(
    project_root: &Path,
    run_id: &str,
    poll_interval: std::time::Duration,
    timeout: std::time::Duration,
) -> std::io::Result<RunState> {
    let start = std::time::Instant::now();

    loop {
        let state = load_run_state(project_root, run_id)?;

        match &state.status {
            RunStatus::Completed | RunStatus::Killed => return Ok(state),
            RunStatus::Failed(_) => return Ok(state),
            RunStatus::Running | RunStatus::Queued => {
                if start.elapsed() >= timeout {
                    return Err(std::io::Error::other(format!(
                        "timed out waiting for run {run_id} after {:.0}s",
                        timeout.as_secs_f64()
                    )));
                }
                std::thread::sleep(poll_interval);
            }
        }
    }
}

/// Find the most recent run that has completed with results.
pub fn find_latest_with_results(project_root: &Path) -> std::io::Result<Option<RunState>> {
    let runs = list_runs(project_root)?;
    Ok(runs
        .into_iter()
        .rev()
        .find(|r| r.status == RunStatus::Completed && r.result_path.is_some()))
}

/// Check the environment for fire-and-forget mode.
///
/// If `ZENBENCH_RESULT_PATH` is set, the benchmark should save results
/// to that path when complete. Returns the path if set.
pub fn result_path_from_env() -> Option<PathBuf> {
    std::env::var("ZENBENCH_RESULT_PATH")
        .ok()
        .map(PathBuf::from)
}
