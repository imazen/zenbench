#![cfg(feature = "cli")]
use std::process::Command;

#[test]
fn missing_comparison_artifacts_cannot_report_success() {
    let missing = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!("missing-evidence-{}", zenbench::RunId::generate()));
    let executable = env!("CARGO_BIN_EXE_zenbench");
    let mut command = if let Some(runner) = std::env::var_os("ZENBENCH_AFFINITY_TEST_RUNNER") {
        let mut command = Command::new(runner);
        command.arg(executable);
        command
    } else {
        Command::new(executable)
    };
    let output = command
        .arg("compare")
        .arg(&missing)
        .arg(&missing)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Error loading baseline"));
}
