use zenbench::platform::{CpuProbeConfig, CpuProbeReport, probe_cpu_thread};

#[test]
fn probes_the_actual_rayon_workers_and_preserves_their_masks() {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(2)
        .build()
        .unwrap();
    let mut config = CpuProbeConfig::default();
    config.samples = 3;
    config.iterations = 1024;
    let workers = pool
        .broadcast(|_| probe_cpu_thread(config))
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let report = CpuProbeReport::from_workers(workers).unwrap();
    assert_eq!(report.workers.len(), 2);
    assert_eq!(report.integer.len(), 2);
    for worker in &report.workers {
        assert_eq!(worker.samples.len(), config.samples);
        assert_eq!(worker.affinity_before, worker.affinity_after);
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        assert!(worker.affinity_before.is_ok());
    }
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["certifies_cpu_class"], false);
}

#[cfg(feature = "cli")]
#[test]
fn cli_probe_does_not_upgrade_admission_or_claim_external_pool_coverage() {
    let command = || {
        let executable = env!("CARGO_BIN_EXE_zenbench");
        let mut cmd = if let Some(runner) = std::env::var_os("ZENBENCH_AFFINITY_TEST_RUNNER") {
            let mut cmd = std::process::Command::new(runner);
            cmd.arg(executable);
            cmd
        } else {
            std::process::Command::new(executable)
        };
        cmd.env_remove("ZENBENCH_CPU_POLICY")
            .env_remove("ZENBENCH_CPU_SET");
        cmd
    };
    let original = command().arg("cpu-info").output().unwrap();
    assert!(
        original.status.success(),
        "{}",
        String::from_utf8_lossy(&original.stderr)
    );
    let original: serde_json::Value = serde_json::from_slice(&original.stdout).unwrap();
    assert!(original.get("probe").is_none());
    let output = command()
        .args(["cpu-info", "--probe", "--workers", "2"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let observed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(observed["placement"], original["placement"]);
    assert_eq!(observed["probe"]["Ok"]["certifies_cpu_class"], false);
    assert_eq!(
        observed["probe"]["Ok"]["workers"].as_array().unwrap().len(),
        2
    );
    assert_eq!(observed["probe_scope"], "new_diagnostic_workers_only");
}
