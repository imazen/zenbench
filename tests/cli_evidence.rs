#![cfg(feature = "cli")]
use std::process::Command;

#[test]
fn missing_comparison_artifacts_cannot_report_success() {
    let missing = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!("missing-evidence-{}", zenbench::RunId::generate()));
    let output = Command::new(env!("CARGO_BIN_EXE_zenbench"))
        .arg("compare")
        .arg(&missing)
        .arg(&missing)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Error loading baseline"));
}
