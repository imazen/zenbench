#![cfg(feature = "cli")]
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

// Executed by the parent harness as a real fresh child. Fixed emitted metrics
// are an extraction/statistics oracle, not claimed hardware measurements.
#[test]
fn pair_child_fixture() {
    // libtest's single-threaded output (including cross/QEMU) leaves its
    // "test ... " prefix unterminated. Start our line protocol on a fresh row.
    println!();
    let kind = std::env::var("PAIR_KIND").unwrap_or_else(|_| "base".into());
    if kind == "sleep" {
        std::thread::sleep(std::time::Duration::from_secs(5));
    }
    if kind == "exit" {
        std::process::exit(7);
    }
    if let Ok(ms) = std::env::var("PAIR_LINGER_MS") {
        std::thread::sleep(std::time::Duration::from_millis(ms.parse().unwrap()));
    }
    let values = if kind == "candidate" {
        [8.0, 12.0]
    } else {
        [10.0, 14.0]
    };
    if kind == "malformed" {
        println!("TIME bad");
    } else {
        for value in values {
            if std::env::var("PAIR_STREAM").as_deref() == Ok("stderr") {
                eprintln!("TIME {value}");
            } else {
                println!("TIME {value}");
            }
        }
    }
    if kind == "extra" {
        println!("TIME 99");
    }
    let bytes = if kind == "wrong" {
        b"wrong".as_slice()
    } else {
        b"same".as_slice()
    };
    println!(
        "CHECK {}",
        json!({"digest":String::from_utf8_lossy(bytes),"count":bytes.len()})
    );
    if let Ok(output) = std::env::var("PAIR_OUTPUT") {
        std::fs::write(Path::new(&output).join("result.bin"), bytes).unwrap();
    }
    if std::env::var("PAIR_REMOVE").is_ok() {
        println!("TIME 99");
    }
}
fn root() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join(format!(
            "pair-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
    std::fs::create_dir(&path).unwrap();
    std::fs::create_dir(path.join("queue")).unwrap();
    path
}
fn cli(root: &Path) -> Command {
    let executable = env!("CARGO_BIN_EXE_zenbench");
    let mut command = if let Some(runner) = std::env::var_os("ZENBENCH_AFFINITY_TEST_RUNNER") {
        let mut command = Command::new(runner);
        command.arg(executable);
        command
    } else {
        Command::new(executable)
    };
    command
        .env("ZENBENCH_COORDINATION_DIR", root.join("queue"))
        .env_remove("ZENBENCH_COORDINATION_LEASE")
        .env("ZENBENCH_CPU_POLICY", "unrestricted")
        .env("PAIR_REMOVE", "inherited");
    command
}
fn plan(root: &Path, kind: &str) -> PathBuf {
    let executable = std::env::current_exe().unwrap();
    let (program, args) = if let Some(runner) = std::env::var_os("ZENBENCH_AFFINITY_TEST_RUNNER") {
        (
            PathBuf::from(runner),
            vec![
                executable.to_string_lossy().into_owned(),
                "--exact".into(),
                "pair_child_fixture".into(),
                "--nocapture".into(),
            ],
        )
    } else {
        (
            executable,
            vec![
                "--exact".into(),
                "pair_child_fixture".into(),
                "--nocapture".into(),
            ],
        )
    };
    let remove_key = if cfg!(windows) {
        "pAiR_rEmOvE"
    } else {
        "PAIR_REMOVE"
    };
    let arm = |id: &str, kind: &str| json!({"id":id,"program":program,"args":args,"env":{"PAIR_KIND":kind,"PAIR_OUTPUT":"${output}",(remove_key):null},"revision":"fixture","timeout_seconds":1});
    let plan = json!({"schema":1,"baseline":arm("base","base"),"candidates":[arm("edit",kind)],"cases":[{"id":"spaces and commas, literal","fields":{"unused":"two words"}}],"metric":{"extract":{"kind":"prefix","prefix":"TIME "},"unit":"ns","expected":2},"check":{"kind":"files","paths":["result.bin"]},"warmup":"synthetic fixture; no hardware timing","blocks":2,"deadline_seconds":30,"seed":42});
    let path = root.join("plan.json");
    std::fs::write(&path, serde_json::to_vec(&plan).unwrap()).unwrap();
    path
}
fn read(root: &Path, directory: &str) -> Value {
    let output = cli(root)
        .args(["read"])
        .arg(root.join(directory).join("packet.json.gz"))
        .args(["--format", "json"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}
#[test]
fn same_binary_different_env_preserves_process_clusters_and_control() {
    let root = root();
    let plan = plan(&root, "candidate");
    let preview = cli(&root)
        .arg("pair")
        .arg(&plan)
        .arg("--dry-run")
        .output()
        .unwrap();
    assert!(preview.status.success(), "{preview:?}");
    let preview: Value = serde_json::from_slice(&preview.stdout).unwrap();
    assert_eq!(preview["invocation_count"], 6);
    assert_eq!(preview["runtime_estimate"], Value::Null);
    let output = cli(&root)
        .arg("pair")
        .arg(&plan)
        .arg("--output")
        .arg(root.join("first"))
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let packet = read(&root, "first");
    assert_eq!(packet["complete"], true);
    assert_eq!(packet["invocations"].as_array().unwrap().len(), 6);
    for event in packet["invocations"].as_array().unwrap() {
        assert_eq!(event["values_ns"].as_array().unwrap().len(), 2);
        assert!(event["process_id"].as_u64().unwrap() > 0);
        assert_eq!(event["observations"].as_array().unwrap().len(), 2);
    }
    let summaries = packet["summaries"].as_array().unwrap();
    let candidate = &summaries[0];
    // Independently: means 10 vs 12 => (10/12 - 1) * 100.
    assert!((candidate["latency_change_pct"].as_f64().unwrap() - (-100.0 / 6.0)).abs() < 1e-10);
    assert_eq!(candidate["complete_blocks"], 2);
    assert_eq!(summaries[1]["latency_change_pct"], 0.0);
    let resume = cli(&root)
        .arg("pair")
        .arg(&plan)
        .arg("--resume")
        .arg(root.join("first/packet.json.gz"))
        .arg("--output")
        .arg(root.join("second"))
        .output()
        .unwrap();
    assert!(resume.status.success(), "{resume:?}");
    let resumed = read(&root, "second");
    assert_eq!(resumed["invocations"], packet["invocations"]);
    // Recover from checked block checkpoints if publication was interrupted.
    std::fs::rename(
        root.join("first/packet.json.gz"),
        root.join("first/packet.json.gz.saved"),
    )
    .unwrap();
    use std::io::Write;
    std::fs::OpenOptions::new()
        .append(true)
        .open(root.join("first/events.jsonl"))
        .unwrap()
        .write_all(b"{truncated")
        .unwrap();
    let recovered = cli(&root)
        .arg("pair")
        .arg(&plan)
        .arg("--resume")
        .arg(root.join("first"))
        .arg("--output")
        .arg(root.join("recovered"))
        .output()
        .unwrap();
    assert!(recovered.status.success(), "{recovered:?}");
    let recovered = read(&root, "recovered");
    assert_eq!(recovered["summaries"], packet["summaries"]);
    assert_eq!(recovered["seed"], packet["seed"]);
    std::fs::rename(
        root.join("first/packet.json.gz.saved"),
        root.join("first/packet.json.gz"),
    )
    .unwrap();
    // A partial case must start over; its completed arms are not borrowed.
    std::fs::rename(
        root.join("first/packet.json.gz"),
        root.join("first/packet.json.gz.saved"),
    )
    .unwrap();
    std::fs::rename(
        root.join("first/case-0-block-1.json"),
        root.join("first/case-0-block-1.json.saved"),
    )
    .unwrap();
    let restarted = cli(&root)
        .arg("pair")
        .arg(&plan)
        .arg("--resume")
        .arg(root.join("first"))
        .arg("--output")
        .arg(root.join("restarted"))
        .output()
        .unwrap();
    assert!(restarted.status.success(), "{restarted:?}");
    let restarted = read(&root, "restarted");
    for invocation in restarted["invocations"].as_array().unwrap() {
        assert!(
            Path::new(invocation["output"].as_str().unwrap()).starts_with(root.join("restarted"))
        );
    }
    assert!(
        std::fs::read(root.join("first/events.jsonl"))
            .unwrap()
            .ends_with(b"{truncated")
    );
    std::fs::rename(
        root.join("first/packet.json.gz.saved"),
        root.join("first/packet.json.gz"),
    )
    .unwrap();
    // Resume validates retained outputs, rather than trusting a complete flag.
    let artifact =
        PathBuf::from(packet["invocations"][0]["output"].as_str().unwrap()).join("result.bin");
    std::fs::write(artifact, b"changed").unwrap();
    let resume = cli(&root)
        .arg("pair")
        .arg(&plan)
        .arg("--resume")
        .arg(root.join("first/packet.json.gz"))
        .output()
        .unwrap();
    assert_eq!(resume.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&resume.stderr).contains("artifact changed"));
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn failures_are_retained_and_never_form_complete_pairs() {
    for kind in ["extra", "malformed", "wrong", "exit", "sleep"] {
        let root = root();
        let plan = plan(&root, kind);
        let output = cli(&root)
            .arg("pair")
            .arg(plan)
            .arg("--output")
            .arg(root.join("attempt"))
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "kind={kind}: {output:?}");
        let packet = read(&root, "attempt");
        assert_eq!(packet["complete"], false, "{kind}");
        assert!(
            packet["invocations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e["status"] == "failed"),
            "{kind}"
        );
        assert_eq!(packet["summaries"][0]["complete_blocks"], 0, "{kind}");
        assert_eq!(
            packet["summaries"][0]["latency_change_pct"],
            Value::Null,
            "{kind}"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn pair_checker_fixture() {
    let args: Vec<_> = std::env::args().collect();
    let directories: Vec<_> = args
        .windows(2)
        .filter(|a| a[0] == "--skip")
        .map(|a| PathBuf::from(&a[1]))
        .collect();
    if directories.len() == 2 {
        assert_eq!(
            std::fs::read(directories[0].join("result.bin")).unwrap(),
            std::fs::read(directories[1].join("result.bin")).unwrap()
        );
    } else {
        assert!(directories.is_empty());
    }
}
#[test]
fn json_and_command_correctness_contracts_are_enforced() {
    for (contract, kind, success) in [
        ("json", "candidate", true),
        ("json", "wrong", false),
        ("command", "candidate", true),
        ("command", "wrong", false),
    ] {
        let root = root();
        let plan_path = plan(&root, kind);
        let mut plan: Value = serde_json::from_slice(&std::fs::read(&plan_path).unwrap()).unwrap();
        plan["blocks"] = json!(1);
        if contract == "json" && success {
            plan["metric"]["stream"] = json!("stderr");
            plan["env"] = json!({"PAIR_STREAM":"stderr"});
        }
        plan["check"] = if contract == "json" {
            json!({"kind":"json","prefix":"CHECK ","pointers":["/digest","/count"]})
        } else {
            let executable = std::env::current_exe().unwrap();
            let (program, mut args) =
                if let Some(runner) = std::env::var_os("ZENBENCH_AFFINITY_TEST_RUNNER") {
                    // The checker contract requires a resolved executable path.
                    let runner = PathBuf::from(runner);
                    let runner = if runner.is_absolute() {
                        runner
                    } else {
                        std::env::split_paths(&std::env::var_os("PATH").unwrap())
                            .map(|directory| directory.join(&runner))
                            .find(|path| path.is_file())
                            .expect("configured subprocess runner must exist on PATH")
                    };
                    (runner, vec![executable.to_string_lossy().into_owned()])
                } else {
                    (executable, Vec::new())
                };
            args.extend(
                [
                    "--exact",
                    "pair_checker_fixture",
                    "--nocapture",
                    "--skip",
                    "${baseline}",
                    "--skip",
                    "${candidate}",
                ]
                .map(String::from),
            );
            json!({"kind":"command","program":program,"args":args,"timeout_seconds":1})
        };
        std::fs::write(&plan_path, serde_json::to_vec(&plan).unwrap()).unwrap();
        let output = cli(&root)
            .arg("pair")
            .arg(plan_path)
            .arg("--output")
            .arg(root.join("attempt"))
            .output()
            .unwrap();
        assert_eq!(
            output.status.success(),
            success,
            "{contract}/{kind}: {output:?}"
        );
        assert_eq!(read(&root, "attempt")["complete"], success);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn confirmation_requires_frozen_contract_and_never_accepts_unverified_placement() {
    let root = root();
    let path = plan(&root, "candidate");
    let missing = cli(&root)
        .arg("pair")
        .arg(&path)
        .args(["--mode", "confirm", "--dry-run"])
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("confirmation object"));
    let mut p: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    p["blocks"] = json!(5);
    p["metric"]["resolution"] = json!(0.001);
    p["confirmation"] = json!({"margin_pct":1.0,"control_margin_pct":1.0,"family_confidence":0.95,"scope":"fixture"});
    std::fs::write(&path, serde_json::to_vec(&p).unwrap()).unwrap();
    let output = cli(&root)
        .arg("pair")
        .arg(&path)
        .args(["--mode", "confirm", "--output"])
        .arg(root.join("confirmed"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let packet = read(&root, "confirmed");
    assert_eq!(packet["mode"], "confirm");
    assert_eq!(packet["invocations"].as_array().unwrap().len(), 15);
    assert_eq!(packet["summaries"][0]["verdict"], "invalid");
    assert!(
        packet["summaries"][0]["validity_issues"]
            .as_array()
            .unwrap()
            .contains(&json!("unverified CPU placement"))
    );
    assert_eq!(packet["summaries"][0]["interval"]["blocks"], 5);
}

// Real supervisor wiring: every sampled window carries the owned lifecycle
// bounds, and only windows completing at or after the exit observation are
// terminal. Host load may make windows unclean; that is not asserted here.
#[test]
fn confirmation_windows_carry_the_owned_exit_lifecycle() {
    let root = root();
    let path = plan(&root, "base");
    let mut p: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    p["blocks"] = json!(5);
    p["deadline_seconds"] = json!(240);
    p["env"] = json!({"PAIR_LINGER_MS":"800"});
    p["baseline"]["timeout_seconds"] = json!(30);
    p["candidates"][0]["timeout_seconds"] = json!(30);
    p["metric"]["resolution"] = json!(0.001);
    p["confirmation"] = json!({"margin_pct":1.0,"control_margin_pct":1.0,"family_confidence":0.95,"scope":"fixture"});
    std::fs::write(&path, serde_json::to_vec(&p).unwrap()).unwrap();
    let output = cli(&root)
        .arg("pair")
        .arg(&path)
        .args(["--mode", "confirm", "--output"])
        .arg(root.join("lifecycle"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let packet = read(&root, "lifecycle");
    let invocations = packet["invocations"].as_array().unwrap();
    assert_eq!(invocations.len(), 15);
    let ns = |v: &Value| v.as_str().unwrap().parse::<u128>().unwrap();
    for invocation in invocations {
        assert_eq!(invocation["status"], "completed", "{invocation}");
        let observations = invocation["observations"].as_array().unwrap();
        assert!(
            observations
                .iter()
                .all(|o| o["phase"] != "lifecycle_unobserved"),
            "{invocation}"
        );
        let sampled: Vec<_> = observations
            .iter()
            .filter(|o| o.get("sample_completed_after_start_ns").is_some())
            .collect();
        assert!(!sampled.is_empty(), "{invocation}");
        let wall = (invocation["wall_seconds"].as_f64().unwrap() * 1e9) as u128;
        for o in sampled {
            let completed = ns(&o["sample_completed_after_start_ns"]);
            let alive = ns(&o["leader_last_alive_after_start_ns"]);
            let exit = ns(&o["leader_exit_observed_after_start_ns"]);
            assert!(alive < exit && exit <= wall, "{o}");
            match o["phase"].as_str().unwrap() {
                "during" => assert!(completed < exit && o["used_for_admission"] == true, "{o}"),
                "exit_transition" => assert!(completed >= exit, "{o}"),
                "start_transition" => {}
                phase => panic!("unexpected sampled phase {phase}: {o}"),
            }
            if o["used_for_admission"] == false {
                assert_eq!(o["ownership_independent_clean"], true, "{o}");
                assert_eq!(o["admission_waiver"], "cpu_load_magnitude", "{o}");
            } else {
                assert_eq!(o["used_for_admission"], true, "{o}");
            }
        }
    }
    assert!(
        packet["summaries"][0]["validity_issues"]
            .as_array()
            .unwrap()
            .contains(&json!("unverified CPU placement"))
    );
}
