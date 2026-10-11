#![cfg(feature = "cli")]
use std::process::Command;

fn cli() -> Command {
    let executable = env!("CARGO_BIN_EXE_zenbench");
    if let Some(runner) = std::env::var_os("ZENBENCH_AFFINITY_TEST_RUNNER") {
        let mut command = Command::new(runner);
        command.arg(executable);
        command
    } else {
        Command::new(executable)
    }
}

#[test]
fn missing_comparison_artifacts_cannot_report_success() {
    let missing = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!("missing-evidence-{}", zenbench::RunId::generate()));
    let output = cli()
        .arg("compare")
        .arg(&missing)
        .arg(&missing)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Error loading baseline"));
}

#[test]
fn saved_evidence_is_readable_and_carries_working_commands() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!("read evidence $;'-{}", zenbench::RunId::generate()));
    std::fs::create_dir_all(&root).unwrap();
    let input = root.join("result.json.gz");
    let mut result = zenbench::SuiteResult::default();
    result.loop_overhead_ns = 983.8325645756457;
    result.save(&input).unwrap();
    for (format, expected) in [
        ("llm", result.to_llm()),
        ("csv", result.to_csv()),
        ("md", result.to_markdown()),
    ] {
        let output = cli()
            .arg("read")
            .arg(&input)
            .args(["--format", format])
            .output()
            .unwrap();
        assert!(output.status.success(), "{:?}", output);
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            format!("{expected}\n")
        );
    }
    let snapshot = root.join("selected.csv");
    let output = cli()
        .arg("snapshot")
        .arg(&input)
        .arg("--output")
        .arg(&snapshot)
        .env("ZENBENCH_DATA_DIR", root.join("data"))
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output);
    let text = std::fs::read_to_string(&snapshot).unwrap();
    let usage: serde_json::Value = serde_json::from_str(
        text.lines()
            .find_map(|line| line.strip_prefix("# usage="))
            .unwrap(),
    )
    .unwrap();
    let decode_args: Vec<_> = usage["decode"]
        .as_array()
        .unwrap()
        .iter()
        .skip(1)
        .map(|v| v.as_str().unwrap())
        .collect();
    let output = cli().args(decode_args).output().unwrap();
    assert!(output.status.success(), "{:?}", output);
    let decoded: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(decoded, serde_json::to_value(&result).unwrap());
    let reference = std::fs::read_dir(root.join("data").join("runs"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&reference).unwrap()).unwrap();
    assert_eq!(record["usage"]["read"][2], reference.to_str().unwrap());
    let output = cli().arg("read").arg(&reference).output().unwrap();
    assert!(output.status.success(), "{:?}", output);
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("{}\n", result.to_llm())
    );
    let output = cli()
        .arg("compare")
        .arg(&reference)
        .arg(&input)
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(2),
        "exploratory data cannot confirm"
    );
    let output = cli()
        .arg("read")
        .arg(root.join("missing"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot read evidence"));
    std::fs::remove_dir_all(root).unwrap();
}
