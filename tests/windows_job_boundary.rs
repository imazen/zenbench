//! Native OS-boundary controls in fresh processes, also on Windows ARM CI.
#![cfg(target_os = "windows")]
#[path = "../src/platform/windows_job.rs"]
mod windows_job;

use std::process::Command;
use std::sync::{Arc, Barrier};

fn reject_escape(original: &[usize], selected: &[usize]) {
    // Windows may reject or clamp the request. Actual confinement is the oracle.
    let _ = affinity::set_process_affinity(original);
    assert_eq!(affinity::get_process_affinity().unwrap(), selected);
    let _ = affinity::set_thread_affinity(original);
    assert_eq!(
        zenbench::platform::current_thread_affinity().unwrap(),
        selected
    );
}

#[test]
fn inherited_job_prevents_process_and_worker_escape() {
    let exe = std::env::current_exe().unwrap();
    if let Ok(mode) = std::env::var("ZENBENCH_JOB_TEST_MODE") {
        let original: Vec<usize> =
            serde_json::from_str(&std::env::var("ZENBENCH_JOB_ORIGINAL_CPUS").unwrap()).unwrap();
        let selected = [original[0]];
        if mode == "descendant" {
            assert_eq!(affinity::get_process_affinity().unwrap(), selected);
            reject_escape(&original, &selected);
            return;
        }
        assert_eq!(mode, "owner");
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .build()
            .unwrap();
        let initial = pool.broadcast(|_| zenbench::platform::current_thread_affinity().unwrap());
        assert!(initial.iter().all(|mask| mask == &original));
        let ready = Arc::new(Barrier::new(2));
        let proceed = Arc::new(Barrier::new(2));
        let thread = {
            let ready = ready.clone();
            let proceed = proceed.clone();
            let original = original.clone();
            std::thread::spawn(move || {
                ready.wait();
                proceed.wait();
                reject_escape(&original, &selected);
            })
        };
        ready.wait();
        windows_job::install(&selected).unwrap();
        assert!(windows_job::installed());
        reject_escape(&original, &selected);
        pool.broadcast(|_| reject_escape(&original, &selected));
        proceed.wait();
        thread.join().unwrap();
        let guard = zenbench::platform::pin_current_thread(&selected).unwrap();
        drop(guard);
        assert_eq!(
            zenbench::platform::current_thread_affinity().unwrap(),
            selected
        );
        let status = Command::new(exe)
            .args([
                "--exact",
                "inherited_job_prevents_process_and_worker_escape",
                "--nocapture",
            ])
            .env("ZENBENCH_JOB_TEST_MODE", "descendant")
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    let original = affinity::get_process_affinity().unwrap();
    assert!(
        original.len() > 1,
        "this negative control needs an available CPU outside the selected set"
    );
    let output = Command::new(exe)
        .args([
            "--exact",
            "inherited_job_prevents_process_and_worker_escape",
            "--nocapture",
        ])
        .env("ZENBENCH_JOB_TEST_MODE", "owner")
        .env(
            "ZENBENCH_JOB_ORIGINAL_CPUS",
            serde_json::to_string(&original).unwrap(),
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(affinity::get_process_affinity().unwrap(), original);
}
