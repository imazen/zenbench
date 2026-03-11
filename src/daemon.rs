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

/// List all known runs.
pub fn list_runs(project_root: &Path) -> std::io::Result<Vec<RunState>> {
    let dir = runs_dir(project_root);
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut runs = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json") {
            if let Ok(data) = std::fs::read_to_string(&path) {
                if let Ok(state) = serde_json::from_str::<RunState>(&data) {
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

/// Load a specific run state.
pub fn load_run_state(project_root: &Path, run_id: &str) -> std::io::Result<RunState> {
    let path = runs_dir(project_root).join(format!("{run_id}.json"));
    let data = std::fs::read_to_string(path)?;
    serde_json::from_str(&data).map_err(std::io::Error::other)
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

/// Check if a process is still running.
pub fn is_process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        use std::process::Command;
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .is_ok_and(|s| s.success())
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
            let path = runs_dir(project_root).join(format!("{}.json", run.id));
            if std::fs::remove_file(path).is_ok() {
                cleaned += 1;
            }
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
/// Returns the run ID of the spawned process.
pub fn spawn_detached(
    project_root: &Path,
    command: &str,
    args: &[&str],
) -> std::io::Result<String> {
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

    // Build the result path
    let result_dir = runs_dir(project_root);
    std::fs::create_dir_all(&result_dir)?;
    let result_path = result_dir.join(format!("{run_id}.results.json"));

    // Spawn the process
    let child = std::process::Command::new(command)
        .args(args)
        .env("ZENBENCH_RUN_ID", &run_id)
        .env("ZENBENCH_RESULT_PATH", &result_path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    let mut state = RunState::new(
        run_id.clone(),
        format!("{command} {}", args.join(" ")),
        git_hash,
    );
    state.pid = child.id();
    state.status = RunStatus::Running;
    state.result_path = Some(result_path);
    save_run_state(project_root, &state)?;

    eprintln!("[zenbench] spawned run {run_id} (PID {})", child.id());
    Ok(run_id)
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
