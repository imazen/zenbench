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
