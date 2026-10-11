//! Default-policy negative controls run in fresh processes with CI opt-outs removed.
use std::process::Command;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;
use zenbench::platform::{CpuPolicy, PlacementStatus, check_cpu_placement};
use zenbench::{GateConfig, run_gated};

#[test]
fn process_policy() {
    if let Ok(mode) = std::env::var("ZENBENCH_CPU_TEST_MODE") {
        let visited = Arc::new(AtomicUsize::new(0));
        let seen = visited.clone();
        let expected = check_cpu_placement();
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        let _pin = (mode == "caller_pin").then(|| {
            let cpu = zenbench::platform::current_thread_affinity().unwrap()[0];
            zenbench::platform::pin_current_thread(&[cpu]).unwrap()
        });
        let outcome = std::panic::catch_unwind(move || {
            run_gated(GateConfig::disabled(), |suite| {
                if mode == "suite_override" {
                    suite.cpu_policy(CpuPolicy::Unrestricted);
                }
                suite.group("cpu-proof", |group| {
                    group
                        .config()
                        .max_rounds(5)
                        .auto_rounds(false)
                        .warmup_time(Duration::ZERO)
                        .sample_target_ns(10_000)
                        .min_sample_ns(0);
                    group.bench("work", move |b| {
                        seen.fetch_add(1, Ordering::Relaxed);
                        b.iter(|| std::hint::black_box(42));
                    });
                });
            })
        });
        let permitted = expected.is_ok()
            || std::env::var("ZENBENCH_CPU_TEST_MODE").unwrap() == "suite_override";
        if permitted {
            let result = outcome.unwrap();
            assert!(visited.load(Ordering::Relaxed) > 0);
            let placement = result.cpu_placement.as_ref().unwrap();
            assert!(result.comparisons.iter().all(|c| {
                c.cpu_placement.as_ref().is_some_and(|p| {
                    p.status == placement.status && p.effective_cpus == placement.effective_cpus
                })
            }));
            let rows = result.to_llm();
            assert!(rows.contains("cpu_enforcement="));
            assert!(rows.contains("pool_thread_count=unknown"));
            if placement.status == PlacementStatus::Unrestricted {
                assert!(rows.contains("UNVERIFIED_UNRESTRICTED"));
                assert!(rows.contains("cpu_class_uniform=false"));
            }
            assert!(result.to_csv().contains("cpu_placement_evidence"));
        } else {
            assert!(
                outcome.is_err(),
                "default accepted unverified CPU placement"
            );
            assert_eq!(
                visited.load(Ordering::Relaxed),
                0,
                "work ran before refusal"
            );
        }
    } else {
        for (mode, policy) in [
            ("default", None),
            ("caller_pin", None),
            ("opt_out", Some("unrestricted")),
            ("invalid", Some("fast")),
            ("suite_override", None),
        ] {
            let executable = std::env::current_exe().unwrap();
            let mut child = if let Some(runner) = std::env::var_os("ZENBENCH_AFFINITY_TEST_RUNNER")
            {
                let mut command = Command::new(runner);
                command.arg(executable);
                command
            } else {
                Command::new(executable)
            };
            child
                .args(["--exact", "process_policy", "--nocapture"])
                .env("ZENBENCH_CPU_TEST_MODE", mode)
                .env("ZENBENCH_NO_SAVE", "1")
                .env("ZENBENCH_NO_CALIBRATE", "1")
                .env_remove("ZENBENCH_CPU_POLICY")
                .env_remove("ZENBENCH_CPU_SET")
                .env_remove("ZENBENCH_PIN");
            if let Some(policy) = policy {
                child.env("ZENBENCH_CPU_POLICY", policy);
            }
            let output = child.output().unwrap();
            assert!(
                output.status.success(),
                "{mode}:\n{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
#[test]
fn suite_affinity_is_inherited_and_groups_can_override() {
    let before = zenbench::platform::current_thread_affinity().unwrap();
    let cpu = before[0];
    let result = run_gated(GateConfig::disabled(), |suite| {
        suite.cpu_policy(CpuPolicy::Unrestricted).affinity(&[cpu]);
        for name in ["inherit", "override"] {
            let expected = if name == "override" {
                before.clone()
            } else {
                vec![cpu]
            };
            suite.group(name, |g| {
                if name == "override" {
                    g.affinity(&before);
                }
                g.config()
                    .max_rounds(5)
                    .auto_rounds(false)
                    .warmup_time(Duration::ZERO)
                    .sample_target_ns(10_000)
                    .min_sample_ns(0);
                g.bench("mask", move |b| {
                    assert_eq!(
                        zenbench::platform::current_thread_affinity().unwrap(),
                        expected
                    );
                    b.iter(|| std::hint::black_box(42));
                });
            });
        }
    });
    assert_eq!(result.comparisons.len(), 2);
    assert_eq!(
        zenbench::platform::current_thread_affinity().unwrap(),
        before
    );
}

#[cfg(feature = "cli")]
#[test]
fn launcher_helper_cannot_certify_an_opt_out() {
    let output = Command::new(env!("CARGO_BIN_EXE_zenbench"))
        .args(["cpu-child", "this-program-must-never-be-launched"])
        .env("ZENBENCH_CPU_POLICY", "unrestricted")
        .env_remove("ZENBENCH_CPU_SET")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("launcher requires an enforced uniform process boundary")
    );
}
