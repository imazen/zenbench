// Verify that fs4 advisory locks are automatically released when the
// holder process dies. This is the load-bearing assumption behind
// zenbench's ProcessLock: we never need a "stale lock" recovery path on
// a single machine because the kernel reaps file descriptors (and their
// flocks) when a process exits — even via SIGKILL / TerminateProcess.

#![cfg(not(target_arch = "wasm32"))]

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn locate_example_binary() -> PathBuf {
    // Build the example synchronously. Cached after the first run.
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let status = Command::new(&cargo)
        .args(["build", "--example", "lock_holder", "--quiet"])
        .status()
        .expect("invoke cargo");
    assert!(status.success(), "cargo build --example lock_holder failed");

    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let exe_name = if cfg!(windows) {
        "lock_holder.exe"
    } else {
        "lock_holder"
    };

    // Tests can run under either profile; check both.
    for profile in ["debug", "release"] {
        let candidate = manifest
            .join("target")
            .join(profile)
            .join("examples")
            .join(exe_name);
        if candidate.exists() {
            return candidate;
        }
    }
    panic!("lock_holder example binary not found under target/{{debug,release}}/examples");
}

#[test]
fn fs4_lock_releases_when_holder_is_killed() {
    let exe = locate_example_binary();

    let lock_path = std::env::temp_dir().join(format!(
        "zenbench-crash-test-{}-{}.lock",
        std::process::id(),
        Instant::now().elapsed().as_nanos()
    ));
    // Ensure we don't leak the file across test runs.
    let _ = std::fs::remove_file(&lock_path);

    let mut child = Command::new(&exe)
        .arg(&lock_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn lock_holder");

    // Wait for the child to confirm it has the lock.
    let stdout = child.stdout.take().expect("child stdout");
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader.read_line(&mut line).expect("read LOCKED handshake");
    assert!(
        line.starts_with("LOCKED "),
        "unexpected handshake: {line:?}"
    );

    // Sanity check: while the child holds the lock, try_lock from this
    // process must fail with WouldBlock. Proves the lock is real, not
    // just a file.
    {
        let probe = std::fs::OpenOptions::new()
            .write(true)
            .read(true)
            .open(&lock_path)
            .expect("open probe");
        match fs4::FileExt::try_lock(&probe) {
            Err(fs4::TryLockError::WouldBlock) => {} // expected
            Ok(()) => panic!("try_lock unexpectedly succeeded while holder is alive"),
            Err(fs4::TryLockError::Error(e)) => panic!("try_lock errored: {e}"),
        }
    }

    // Kill the holder. On Unix this is SIGKILL; on Windows,
    // TerminateProcess. Both bypass any cleanup the child might run.
    child.kill().expect("kill child");
    let _ = child.wait();

    // The kernel should have released the lock the instant the holder's
    // FDs were closed. Try to acquire — give it a small window in case
    // of OS-level reap latency (Windows handle teardown is async-ish).
    let deadline = Instant::now() + Duration::from_secs(5);
    let acquired = loop {
        let f = std::fs::OpenOptions::new()
            .write(true)
            .read(true)
            .open(&lock_path)
            .expect("open lock file");
        match fs4::FileExt::try_lock(&f) {
            Ok(()) => break true,
            Err(fs4::TryLockError::WouldBlock) => {
                if Instant::now() >= deadline {
                    break false;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(fs4::TryLockError::Error(e)) => panic!("try_lock errored: {e}"),
        }
    };

    let _ = std::fs::remove_file(&lock_path);
    assert!(
        acquired,
        "fs4 lock was not released within 5s of holder being killed"
    );
}
