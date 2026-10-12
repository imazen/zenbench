#![cfg(feature = "cli")]
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn cli(root: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_zenbench"));
    command
        .env("ZENBENCH_COORDINATION_DIR", root)
        .env_remove("ZENBENCH_COORDINATION_LEASE");
    command
}

#[test]
fn nice_help_and_invalid_admission_do_not_start_work() {
    let root = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join(format!("nice-invalid-{}", std::process::id()));
    let output = cli(&root).args(["nice", "--help"]).output().unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("FIFO"));
    let output = cli(&root)
        .args(["nice", "--", "definitely-no-such-command"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("existing absolute"));
    assert!(!root.exists());
}

// Explicit invocation from just nice-smoke selects the real systemd check.
// This fixture is a standalone CLI test executable mode, not an environment-
// dependent pass/skip inside a normal test.
#[test]
fn queue_timeout_does_not_start_command_and_crash_is_not_permission() {
    let root = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join(format!("nice-queue-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    // An OS-held guard is the independent oracle: a real CLI must wait for it.
    let guard = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join("1.guard"))
        .unwrap();
    fs4::FileExt::lock(&guard).unwrap();
    std::fs::write(root.join("sequence"), "1").unwrap();
    std::fs::write(
        root.join("1.json"),
        r#"{"sequence":1,"pid":1,"start":0,"activity":"test owner","running":true}"#,
    )
    .unwrap();
    let mut child = cli(&root)
        .args(["nice", "--wait", "1", "--", "definitely-no-such-command"])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let start = Instant::now();
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "CLI ignored queue deadline"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("task not started"));
    drop(guard);
    let output = cli(&root)
        .args(["nice", "--", "definitely-no-such-command"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("orphaned running"));
    std::fs::remove_dir_all(root).unwrap();
}
