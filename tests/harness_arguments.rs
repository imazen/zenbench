//! Exercise the actual main! parser without invoking a timed callback.
use std::process::Command;

mod fixture {
    pub(super) fn run() {
        main();
    }
    zenbench::main!(|suite| {
        for name in ["selected group", "other"] {
            suite.group(name, move |group| {
                group.bench_with_witness(
                    "callback",
                    move |b| {
                        assert!(
                            std::env::args().any(|arg| arg == "--smoke" || arg == "--once"),
                            "dry-run invoked a callback"
                        );
                        assert_eq!(name, "selected group", "unselected callback ran");
                        b.iter(|| println!("SMOKE_CALLBACK"));
                    },
                    || {
                        assert!(std::env::args().any(|arg| arg == "--once"));
                        [0x0f; 32]
                    },
                );
            });
        }
    });
}

#[cfg(feature = "criterion-compat")]
mod compat_fixture {
    use zenbench::criterion_compat::Criterion;

    fn probe() -> Criterion {
        let mut criterion = Criterion::default();
        let local = 42;
        criterion.bench_function("compat", |b| b.iter(|| println!("COMPAT_{local}")));
        criterion
    }

    zenbench::criterion_main!(probe);

    pub(super) fn run() {
        main();
    }
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
        .env(
            "ZENBENCH_CPU_POLICY",
            if args.contains(&"--smoke") {
                "invalid"
            } else {
                "unrestricted"
            },
        )
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
        #[cfg(feature = "criterion-compat")]
        if std::env::args().any(|arg| arg == "--group=compat") {
            compat_fixture::run();
            return;
        }
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
    let smoke = child(&["--group", "selected group", "--smoke"]);
    assert!(smoke.status.success(), "{smoke:?}");
    let output = String::from_utf8(smoke.stdout).unwrap();
    assert_eq!(output.lines().filter(|l| *l == "SMOKE_CALLBACK").count(), 1);
    assert!(output.contains("Testing \"selected group\"/\"callback\" ... ok"));
    let once = child(&["--group=selected group", "--once"]);
    assert!(once.status.success(), "{once:?}");
    let output = String::from_utf8(once.stdout).unwrap();
    let mut lines = output.lines();
    assert_eq!(lines.next(), Some("SMOKE_CALLBACK"));
    let row: serde_json::Value = serde_json::from_str(lines.next().unwrap()).unwrap();
    assert!(lines.next().is_none());
    assert_eq!(row["iterations"], 1);
    assert_eq!(row["output_witness"], "0f".repeat(32));
    assert!(row["elapsed_ns"].is_u64());
    #[cfg(feature = "criterion-compat")]
    {
        let compat = child(&["--group=compat", "--smoke"]);
        assert!(compat.status.success(), "{compat:?}");
        let output = String::from_utf8(compat.stdout).unwrap();
        assert_eq!(
            output.lines().filter(|line| *line == "COMPAT_42").count(),
            1
        );
        assert!(output.contains("Testing \"compat\"/\"_\" ... ok"));
    }
    let panic = child(&["--group=other", "--smoke"]);
    assert!(!panic.status.success());
    assert!(String::from_utf8_lossy(&panic.stderr).contains("unselected callback ran"));
    let nothing = child(&["--group=never-matches", "--smoke"]);
    assert_eq!(nothing.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&nothing.stderr).contains("no benchmark callbacks matched"));
    for args in [
        vec!["--rounds"],
        vec!["--rounds", "5", "--rounds=7"],
        vec!["--scope", "--dry-run"],
        vec!["--unknown", "1"],
        vec!["--smoke", "--mode=confirm"],
        vec!["--smoke", "--save-baseline=wrong"],
        vec!["--smoke", "--mean-of-processes=3"],
        vec!["--once", "--smoke"],
        vec!["--once", "--save-baseline=wrong"],
    ] {
        let output = child(&args);
        assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
        assert!(output.stdout.is_empty());
    }
    println!("harness arguments: joined/separated plans match; invalid arguments refused");
}
