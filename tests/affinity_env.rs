//! Exercise the cached environment through fresh processes, without mutating it
//! under other Rust test threads.
#![cfg(any(target_os = "linux", target_os = "windows"))]

use std::process::Command;
use std::time::Duration;
use zenbench::platform::current_thread_affinity;
use zenbench::{GateConfig, run_gated};

#[test]
fn environment_pinning() {
    if let Ok(mode) = std::env::var("ZENBENCH_AFFINITY_TEST_MODE") {
        let before = current_thread_affinity().unwrap();
        let core = before[0];
        let expected = if mode == "override" || mode == "unset" {
            before.clone()
        } else {
            vec![core]
        };
        let run = std::panic::catch_unwind(|| {
            run_gated(GateConfig::disabled(), |suite| {
                suite.group("environment", |g| {
                    if mode == "override" {
                        g.affinity(&before);
                    }
                    g.config()
                        .max_rounds(5)
                        .auto_rounds(false)
                        .warmup_time(Duration::ZERO)
                        .sample_target_ns(10_000)
                        .min_sample_ns(0);
                    g.bench("work", move |b| {
                        assert_eq!(current_thread_affinity().unwrap(), expected);
                        b.iter(|| std::hint::black_box(42));
                    });
                });
            })
        });
        if mode == "invalid" {
            let payload = run.unwrap_err();
            let message = payload.downcast_ref::<String>().unwrap();
            assert!(
                message.contains("benchmark affinity") || message.contains("affinity:"),
                "{message}"
            );
        } else {
            assert_eq!(run.unwrap().comparisons.len(), 1);
        }
        assert_eq!(current_thread_affinity().unwrap(), before);
    } else {
        let core = current_thread_affinity().unwrap()[0];
        for (mode, spec) in [
            ("unset", None),
            ("pin", Some(format!("{core}-{core},{core}"))),
            ("override", Some(core.to_string())),
            ("invalid", Some(String::new())),
            ("invalid", Some("3-2".into())),
            ("invalid", Some(usize::MAX.to_string())),
        ] {
            let executable = std::env::current_exe().unwrap();
            // Cross's user-mode emulator does not intercept execve in the
            // guest. The caller must supply the emulator for each fresh child.
            let mut child = if let Some(runner) = std::env::var_os("ZENBENCH_AFFINITY_TEST_RUNNER")
            {
                let mut command = Command::new(runner);
                command.arg(executable);
                command
            } else {
                Command::new(executable)
            };
            child
                .args(["--exact", "environment_pinning", "--nocapture"])
                .env("ZENBENCH_AFFINITY_TEST_MODE", mode)
                .env("ZENBENCH_NO_SAVE", "1")
                .env("ZENBENCH_NO_CALIBRATE", "1")
                .env_remove("ZENBENCH_PIN");
            if let Some(spec) = spec {
                child.env("ZENBENCH_PIN", spec);
            }
            let output = child.output().unwrap();
            assert!(
                output.status.success(),
                "{mode} failed:\n{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}
