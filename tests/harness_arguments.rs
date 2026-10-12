//! Exercise the actual main! parser without invoking a timed callback.
use std::process::Command;

mod fixture {
    pub(super) fn run() {
        main();
    }
    zenbench::main!(|suite| {
        for name in ["selected group", "other"] {
            suite.group(name, |group| {
                group.bench("must-not-run", |_| panic!("dry-run invoked a callback"));
            });
        }
    });
}

fn child(args: &[&str]) -> std::process::Output {
    let executable = std::env::current_exe().unwrap();
    let mut command = if let Some(runner) = std::env::var_os("ZENBENCH_AFFINITY_TEST_RUNNER") {
        let mut command = Command::new(runner);
        command.arg(&executable);
        command
    } else {
        Command::new(&executable)
    };
    command
        .args(args)
        .env("ZENBENCH_ARGUMENT_CHILD", "1")
        .env("ZENBENCH_CPU_POLICY", "unrestricted")
        .env("ZENBENCH_MODE", "iterate")
        .env_remove("ZENBENCH_NO_SAVE")
        .env_remove("ZENBENCH_NO_BUSY_GATE")
        .env_remove("ZENBENCH_CPU_SET")
        .env_remove("ZENBENCH_PIN")
        .env_remove("ZENBENCH_SUBPROCESS")
        .output()
        .unwrap()
}

fn main() {
    if std::env::var_os("ZENBENCH_ARGUMENT_CHILD").is_some() {
        fixture::run();
        return;
    }
    let joined = child(&[
        "--mode=confirm",
        "--workload-id=inputs with spaces",
        "--scope=host/class",
        "--rounds=7",
        "--processes=3",
        "--margin=0.5",
        "--confidence=0.99",
        "--timeout=10",
        "--group=selected group",
        "--dry-run",
    ]);
    let separated = child(&[
        "--mode",
        "confirm",
        "--workload-id",
        "inputs with spaces",
        "--scope",
        "host/class",
        "--rounds",
        "7",
        "--processes",
        "3",
        "--margin",
        "0.5",
        "--confidence",
        "0.99",
        "--timeout",
        "10",
        "--group",
        "selected group",
        "--dry-run",
    ]);
    assert!(joined.status.success(), "{joined:?}");
    assert!(separated.status.success(), "{separated:?}");
    let joined: serde_json::Value = serde_json::from_slice(&joined.stdout).unwrap();
    let separated: serde_json::Value = serde_json::from_slice(&separated.stdout).unwrap();
    assert_eq!(joined, separated);
    assert_eq!(joined["plan"]["mode"], "confirm");
    assert_eq!(joined["plan"]["workload_id"], "inputs with spaces");
    assert_eq!(joined["groups"].as_array().unwrap().len(), 1);
    assert_eq!(joined["groups"][0]["name"], "selected group");
    for args in [
        vec!["--rounds"],
        vec!["--rounds", "5", "--rounds=7"],
        vec!["--scope", "--dry-run"],
        vec!["--unknown", "1"],
    ] {
        let output = child(&args);
        assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
        assert!(output.stdout.is_empty());
    }
    println!("harness arguments: joined/separated plans match; invalid arguments refused");
}
