#![cfg(feature = "cli")]
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;
fn cli() -> Command {
    let exe = env!("CARGO_BIN_EXE_zenbench");
    if let Some(runner) = std::env::var_os("ZENBENCH_AFFINITY_TEST_RUNNER") {
        let mut cmd = Command::new(runner);
        cmd.arg(exe);
        cmd
    } else {
        Command::new(exe)
    }
}
fn root() -> PathBuf {
    let p = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join(format!(
            "fleet-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir(&p).unwrap();
    p
}
fn json_file(path: &Path, value: Value) {
    std::fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
}
#[test]
fn capabilities_identify_actual_runner_and_preserve_placement_failure() {
    let output = cli()
        .arg("capabilities")
        .env("ZENBENCH_CPU_POLICY", "invalid")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["schema"], 1);
    assert_eq!(v["runner"]["sha256"].as_str().unwrap().len(), 64);
    assert!(v["placement"]["Err"].is_string());
    assert_eq!(v["runner"]["native_arch_verified"], false);
}
#[test]
fn bundles_survive_relocation_and_refuse_modified_bytes() {
    let root = root();
    std::fs::write(root.join("input"), b"fixed input").unwrap();
    let spec = json!({"schema":1,"target":"test-target","plan":{"input":"data/input"},"files":{"data/input":{"source":"input","executable":false}}});
    json_file(&root.join("spec.json"), spec);
    let output = cli()
        .arg("bundle")
        .arg(root.join("spec.json"))
        .arg(root.join("a"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::rename(root.join("a"), root.join("b")).unwrap();
    assert!(
        cli()
            .arg("bundle-check")
            .arg(root.join("b"))
            .status()
            .unwrap()
            .success()
    );
    std::fs::write(root.join("b/data/input"), b"fixed inpuT").unwrap();
    assert_eq!(
        cli()
            .arg("bundle-check")
            .arg(root.join("b"))
            .status()
            .unwrap()
            .code(),
        Some(2)
    );
    // Evidence remains for inspection, as for other subprocess fixtures.
}

#[test]
fn fleet_driver_fixture() {
    let kind = std::env::var("FLEET_FIXTURE").unwrap_or_default();
    if kind == "sleep" {
        std::thread::sleep(std::time::Duration::from_secs(10));
    }
    let data = vec![7u8; 1024 * 1024];
    std::hint::black_box(&data);
    drop(data);
    if let Some(path) = std::env::var_os("ZENBENCH_RESULT_FILE") {
        json_file(
            Path::new(&path),
            json!({"schema":1,"operations":1,"correctness":if kind=="wrong" {"wrong"} else {"ok"},"memory":[]}),
        );
    }
}
fn observation_bundle(root: &Path, backend: &str, kind: &str) {
    let driver = std::env::current_exe().unwrap();
    let driver_name = if cfg!(windows) {
        "driver.exe"
    } else {
        "driver"
    };
    let mut files = json!({driver_name:{"source":driver,"executable":true}});
    let mut program = driver_name;
    let mut args = vec![
        "--exact".to_string(),
        "fleet_driver_fixture".into(),
        "--nocapture".into(),
    ];
    if let Some(runner) = std::env::var_os("ZENBENCH_AFFINITY_TEST_RUNNER") {
        let runner = std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|dir| dir.join(&runner))
            .find(|path| path.is_file())
            .expect("test runner must be installed on PATH");
        files["runner"] = json!({"source":runner,"executable":true});
        program = "runner";
        args.insert(0, driver_name.into());
    }
    let os = if cfg!(target_os = "macos") {
        "darwin"
    } else {
        std::env::consts::OS
    };
    let spec = json!({"schema":1,"target":format!("{}-test-{os}",std::env::consts::ARCH),"files":files,"plan":{
        "schema":1,"program":program,"args":args,"env":{"FLEET_FIXTURE":kind},"backend":backend,"operations":1,"timeout_seconds":2,"expected_correctness":"ok","workload":{"fixture":"transient-spike"}
    }});
    json_file(&root.join("spec.json"), spec);
    let o = cli()
        .arg("bundle")
        .arg(root.join("spec.json"))
        .arg(root.join("bundle"))
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}
fn run_observation(root: &Path) -> Command {
    let mut c = cli();
    c.arg("fleet-run")
        .arg(root.join("bundle"))
        .arg("--output")
        .arg(root.join("attempt"))
        .args([
            "--experiment",
            "fixture",
            "--machine",
            "fixture-cpu",
            "--domain",
            "unrestricted",
            "--replicate",
            "0",
            "--attempt",
            "test",
            "--wait",
            "2",
        ])
        .env("ZENBENCH_CPU_POLICY", "unrestricted")
        .env("ZENBENCH_COORDINATION_DIR", root.join("queue"));
    c
}
#[test]
fn fresh_attempt_checks_output_and_retains_failures_and_cancellation() {
    // Serial to avoid one test's sleeping workload contending with another.
    for (kind, status) in [
        ("ok", "complete"),
        ("wrong", "wrong_output"),
        ("sleep", "timeout"),
        ("cancel", "cancelled"),
    ] {
        let root = root();
        std::fs::create_dir(root.join("queue")).unwrap();
        observation_bundle(&root, "wall", kind);
        let mut c = run_observation(&root);
        if kind == "cancel" {
            std::fs::write(root.join("cancel"), b"").unwrap();
            c.arg("--cancel").arg(root.join("cancel"));
        }
        let o = c.output().unwrap();
        let v: Value =
            serde_json::from_slice(&std::fs::read(root.join("attempt/packet.json")).unwrap())
                .unwrap();
        assert_eq!(
            v["status"],
            status,
            "{} {}",
            String::from_utf8_lossy(&o.stderr),
            v
        );
        assert_eq!(o.status.success(), kind == "ok");
        assert_eq!(v["replicate"], 0);
        assert_eq!(v["attempt"], "test");
        let remaining: Vec<_> = std::fs::read_dir(root.join("queue"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|s| s == "json"))
            .collect();
        assert!(remaining.is_empty(), "{remaining:?}");
    }
}

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
#[test]
fn os_high_water_is_named_and_separate_from_requested_heap() {
    let root = root();
    std::fs::create_dir(root.join("queue")).unwrap();
    observation_bundle(&root, "os_peak", "ok");
    let output = run_observation(&root).output().unwrap();
    let packet: Value =
        serde_json::from_slice(&std::fs::read(root.join("attempt/packet.json")).unwrap()).unwrap();
    assert_eq!(
        packet["status"],
        "complete",
        "{} {packet}",
        String::from_utf8_lossy(&output.stderr)
    );
    let memory = packet["memory"].as_array().unwrap();
    assert!(
        memory
            .iter()
            .all(|m| m["quantity"] != "requested_live_heap_peak")
    );
    let rss = memory
        .iter()
        .find(|m| m["quantity"] == "resident_high_water")
        .unwrap();
    assert!(rss["bytes"].as_u64().unwrap() >= 1024 * 1024, "{rss}");
    #[cfg(windows)]
    assert!(
        memory
            .iter()
            .any(|m| m["quantity"] == "private_commit_high_water")
    );
}

#[test]
fn fleet_exports_are_compact_and_comparisons_verify_evidence() {
    let root = root();
    std::fs::create_dir(root.join("queue")).unwrap();
    observation_bundle(&root, "wall", "ok");
    let result = run_observation(&root).output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let attempt = root.join("attempt");
    let diff = cli()
        .arg("fleet-diff")
        .arg(&attempt)
        .arg(&attempt)
        .output()
        .unwrap();
    assert!(
        diff.status.success(),
        "{}",
        String::from_utf8_lossy(&diff.stderr)
    );
    let export = root.join("records.jsonl");
    let result = cli()
        .arg("fleet-export")
        .arg(&export)
        .arg(&attempt)
        .output()
        .unwrap();
    assert!(result.status.success());
    let text = std::fs::read_to_string(&export).unwrap();
    assert_eq!(text.lines().count(), 1);
    assert!(
        text.len()
            < std::fs::metadata(attempt.join("packet.json"))
                .unwrap()
                .len() as usize
    );
    let record: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(record["status"], "complete");
    assert_eq!(record["packet_sha256"].as_str().unwrap().len(), 64);
    assert!(
        record["commands"]["compare"]
            .as_str()
            .unwrap()
            .contains("fleet-diff")
    );
    // An OS/driver stream edited after collection is no longer verified evidence.
    std::fs::write(attempt.join("stdout.txt"), b"modified stream").unwrap();
    let result = cli().arg("fleet-read").arg(&attempt).output().unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("artifact differs"));
}
