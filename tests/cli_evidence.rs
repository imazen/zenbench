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
fn bare_invocation_surfaces_guides_without_knowing_any_subcommands() {
    let output = cli().output().unwrap();
    let text = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2));
    assert!(text.contains("zenbench guide"), "{text}");
    assert!(text.contains("zenbench guide --topic simd"), "{text}");
    assert!(output.stdout.is_empty());
}

#[test]
fn first_use_help_explains_workflows_without_requiring_a_valid_machine_plan() {
    for (args, required) in [
        (
            vec!["guide", "--topic", "methodology"],
            vec![
                "BUSY WORKSTATIONS",
                "whole-host",
                "process-level",
                "not implemented",
                "measurement-protocol.md",
            ],
        ),
        (
            vec!["guide", "--topic", "simd"],
            vec![
                "compile-time",
                "held-token",
                "cargo asm",
                "A/A",
                "Differential-test",
            ],
        ),
        (
            vec!["run", "--help"],
            vec!["does not preserve", "initial Cargo child wait"],
        ),
        (vec!["--help"], vec!["zenbench guide", "Iterate exit 0"]),
        (
            vec!["guide"],
            vec![
                "KEEP TWO BUILDS",
                "clean/dirty",
                "PROFILE WHY",
                "INCONCLUSIVE",
            ],
        ),
        (
            vec!["exec", "--help"],
            vec!["cpuset", "Rayon", "exploration only"],
        ),
        (
            vec!["cpu-info", "--help"],
            vec!["cache", "does not certify"],
        ),
        (
            vec!["compare", "--help"],
            vec!["Exit 0", "exploratory", "CSV"],
        ),
        (
            vec!["self-compare", "--help"],
            vec!["uncommitted", "return 2", "A/B"],
        ),
    ] {
        let output = cli()
            .args(&args)
            .env("ZENBENCH_MODE", "deliberately-invalid")
            .env("ZENBENCH_CPU_POLICY", "deliberately-invalid")
            .output()
            .unwrap();
        assert!(output.status.success(), "{args:?}: {output:?}");
        let text = std::str::from_utf8(&output.stdout).unwrap();
        for guidance in required {
            assert!(
                text.contains(guidance),
                "{args:?} missing {guidance:?}: {text}"
            );
        }
        assert!(output.stderr.is_empty(), "{output:?}");
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
    let compare_args: Vec<_> = usage["compare"]
        .as_array()
        .unwrap()
        .iter()
        .skip(1)
        .map(|value| match value.as_str().unwrap() {
            "<BASELINE>" => reference.to_str().unwrap(),
            argument => argument,
        })
        .collect();
    let output = cli().args(compare_args).output().unwrap();
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

#[test]
fn workflow_namespaces_retain_flat_command_compatibility() {
    let top = cli().arg("--help").output().unwrap();
    assert!(top.status.success());
    let help = String::from_utf8(top.stdout).unwrap();
    for family in ["data", "fleet", "jobs"] {
        assert!(help.contains(family), "{help}");
    }
    assert!(!help.contains("  fleet-run "), "{help}");
    for (canonical, legacy) in [
        (vec!["data", "read"], "read"),
        (vec!["data", "compare"], "compare"),
        (vec!["data", "pair-diff"], "pair-diff"),
        (vec!["data", "snapshot"], "snapshot"),
        (vec!["data", "archive"], "archive"),
        (vec!["data", "reanalyze"], "reanalyze"),
        (vec!["data", "baseline"], "baseline"),
        (vec!["fleet", "run"], "fleet-run"),
        (vec!["fleet", "read"], "fleet-read"),
        (vec!["fleet", "diff"], "fleet-diff"),
        (vec!["fleet", "export"], "fleet-export"),
        (vec!["fleet", "capabilities"], "capabilities"),
        (vec!["fleet", "bundle"], "bundle"),
        (vec!["fleet", "bundle-check"], "bundle-check"),
        (vec!["jobs", "run"], "run"),
        (vec!["jobs", "list"], "list"),
        (vec!["jobs", "status"], "status"),
        (vec!["jobs", "kill"], "kill"),
        (vec!["jobs", "results"], "results"),
        (vec!["jobs", "clean"], "clean"),
    ] {
        let grouped = cli().args(&canonical).arg("--help").output().unwrap();
        let flat = cli().args([legacy, "--help"]).output().unwrap();
        assert!(grouped.status.success(), "{canonical:?}: {grouped:?}");
        assert!(flat.status.success(), "{legacy}: {flat:?}");
        // Help descriptions and option definitions come from the same source.
        let grouped = String::from_utf8(grouped.stdout).unwrap();
        let flat = String::from_utf8(flat.stdout).unwrap();
        assert_eq!(grouped.lines().next(), flat.lines().next());
    }
    let canonical = cli()
        .args(["data", "read", "missing-cleanup-packet.json"])
        .output()
        .unwrap();
    let legacy = cli()
        .args(["read", "missing-cleanup-packet.json"])
        .output()
        .unwrap();
    assert_eq!(canonical.status.code(), Some(2));
    assert_eq!(canonical.status.code(), legacy.status.code());
    assert_eq!(canonical.stderr, legacy.stderr);
}

#[test]
fn saved_suite_exports_html_and_svg_without_measuring() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!("chart-export-{}", zenbench::RunId::generate()));
    std::fs::create_dir_all(&root).unwrap();
    let input = root.join("result.json.gz");
    let mut result = zenbench::SuiteResult::default();
    let mut group = zenbench::ComparisonResult::default();
    group.group_name = "Illustrative chart".into();
    for (name, mean) in [("baseline", 100.0), ("candidate", 80.0)] {
        let mut benchmark = zenbench::BenchmarkResult::default();
        benchmark.name = name.into();
        benchmark.summary = zenbench::Summary::from_slice(&[mean - 1.0, mean, mean + 1.0]);
        group.benchmarks.push(benchmark);
    }
    result.comparisons.push(group);
    result.save(&input).unwrap();
    let original = std::fs::read(&input).unwrap();
    let export = root.join("slides");
    let output = cli()
        .args(["data", "read"])
        .arg(&input)
        .args(["--format", "html", "--save-charts"])
        .arg(&export)
        // Evidence reads must work without a valid measurement environment.
        .env("ZENBENCH_CPU_POLICY", "deliberately-invalid")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let html = String::from_utf8(output.stdout).unwrap();
    assert!(html.contains("<!DOCTYPE html>"));
    assert!(html.contains("Illustrative chart"));
    let svg = std::fs::read_to_string(export.join("Illustrative_chart.svg")).unwrap();
    assert!(svg.contains("<svg"));
    assert!(svg.contains("candidate"));
    assert_eq!(std::fs::read(&input).unwrap(), original);

    #[cfg(feature = "charts")]
    {
        let output = cli()
            .args(["data", "read"])
            .arg(&input)
            .arg("--publish-charts")
            .arg(root.join("themed"))
            .args(["--chart-theme", "light", "--vertical"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let svg = std::fs::read_to_string(root.join("themed/Illustrative_chart.svg")).unwrap();
        assert!(svg.contains("<svg"));
        assert!(svg.contains("candidate"));
    }

    let blocked = root.join("not-a-directory");
    std::fs::write(&blocked, "keep this file").unwrap();
    let output = cli()
        .args(["data", "read"])
        .arg(&input)
        .arg("--save-charts")
        .arg(&blocked)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot export SVG"));
    assert_eq!(std::fs::read_to_string(blocked).unwrap(), "keep this file");
    assert_eq!(std::fs::read(&input).unwrap(), original);
}

#[test]
fn pair_packets_do_not_silently_export_suite_charts() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!("pair-chart-{}", zenbench::RunId::generate()));
    std::fs::create_dir_all(&root).unwrap();
    let input = root.join("pair.json");
    std::fs::write(&input, r#"{"kind":"zenbench_pair","schema":1}"#).unwrap();
    for args in [vec!["--format", "html"], vec!["--save-charts", "unused"]] {
        let output = cli()
            .args(["data", "read"])
            .arg(&input)
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&output.stderr).contains("require native suite evidence"));
    }
}
