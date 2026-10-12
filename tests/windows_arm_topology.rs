//! Native Windows ARM64 discovery must match a separately implemented OS oracle.
#![cfg(all(target_os = "windows", target_arch = "aarch64"))]

use serde_json::Value;
use std::collections::BTreeSet;
use std::process::Command;
use zenbench::platform::{CpuPolicy, PlacementStatus, TopologyKind};

#[test]
fn discovery_matches_native_core_and_cache_oracle() {
    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/examples/windows_topology_probe.ps1"
        ))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let oracle: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(oracle["process_architecture"], "Arm64");
    assert_eq!(oracle["processor_groups"].as_array().unwrap().len(), 1);
    let topology = zenbench::platform::detect_cpu_topology();
    assert_ne!(topology.kind, TopologyKind::Unknown, "{topology:?}");
    assert_eq!(topology.source, "windows_native_topology");
    let mut expected_cpus = BTreeSet::new();
    for core in oracle["core_records"].as_array().unwrap() {
        let rank = core["efficiency_class"].as_u64().unwrap() as u32;
        assert_eq!(core["affinity"]["group"], 0);
        for cpu in core["affinity"]["logical_ids_in_group"].as_array().unwrap() {
            let cpu = cpu.as_u64().unwrap() as usize;
            assert!(expected_cpus.insert(cpu));
            let class = topology
                .classes
                .iter()
                .find(|c| c.cpus.contains(&cpu))
                .unwrap();
            assert_eq!(class.performance_rank, Some(rank));
            let mut caches = BTreeSet::new();
            for cache in oracle["cache_records"].as_array().unwrap() {
                let kind = cache["type_code"].as_u64().unwrap();
                if kind != 0 && kind != 2 {
                    continue;
                }
                let shared = cache["sharing_masks"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|mask| {
                        mask["group"] == 0
                            && mask["logical_ids_in_group"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .any(|id| id.as_u64() == Some(cpu as u64))
                    });
                if shared {
                    caches.insert((
                        cache["level"].as_u64().unwrap(),
                        cache["size_bytes"].as_u64().unwrap(),
                    ));
                }
            }
            let last = caches.last().unwrap().0;
            let mut name = format!("windows_efficiency{rank}");
            for (level, size) in caches.into_iter().filter(|(level, _)| *level == last) {
                name.push_str(&format!("_l{level}bytes{size}"));
            }
            assert_eq!(class.name, name);
        }
    }
    let actual: Vec<_> = topology
        .classes
        .iter()
        .flat_map(|c| c.cpus.iter().copied())
        .collect();
    assert_eq!(actual.len(), expected_cpus.len());
    assert_eq!(actual.into_iter().collect::<BTreeSet<_>>(), expected_cpus);
}

#[test]
fn native_class_selection_is_enforced() {
    let topology = zenbench::platform::detect_cpu_topology();
    assert_ne!(topology.kind, TopologyKind::Unknown, "{topology:?}");
    if let Ok(mode) = std::env::var("ZENBENCH_WINARM_CONTROL") {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .build()
            .unwrap();
        // Materialize workers before process placement, as user-owned pools do.
        pool.broadcast(|_| zenbench::platform::current_thread_affinity().unwrap());
        let before = affinity::get_process_affinity().unwrap();
        let expected = match mode.as_str() {
            "performance" => topology.select(CpuPolicy::Performance),
            "efficiency" => topology.select(CpuPolicy::Efficiency),
            "explicit" => Ok(std::env::var("ZENBENCH_CPU_SET")
                .unwrap()
                .split(',')
                .map(|n| n.parse().unwrap())
                .collect()),
            "auto" => {
                let placement = zenbench::platform::check_cpu_placement();
                if topology
                    .classes
                    .iter()
                    .any(|c| before.iter().all(|cpu| c.cpus.contains(cpu)))
                {
                    assert!(placement.is_ok(), "{placement:?}");
                } else {
                    assert!(placement.is_err(), "{placement:?}");
                }
                return;
            }
            _ => panic!("unknown control mode"),
        };
        let placement = zenbench::platform::check_cpu_placement();
        let expected = match expected {
            Ok(cpus) => cpus,
            Err(_) => {
                assert!(placement.is_err(), "ambiguous class must refuse");
                return;
            }
        };
        let placement = placement.unwrap();
        assert_eq!(placement.status, PlacementStatus::EnforcedUniform);
        assert_eq!(placement.effective_cpus, expected);
        assert_eq!(affinity::get_process_affinity().unwrap(), expected);
        pool.broadcast(|_| {
            let _ = affinity::set_thread_affinity(&before);
            assert_eq!(
                zenbench::platform::current_thread_affinity().unwrap(),
                expected
            );
        });
        let _ = affinity::set_process_affinity(&before);
        assert_eq!(affinity::get_process_affinity().unwrap(), expected);
        return;
    }
    for (mode, cpus) in ["auto", "performance", "efficiency"]
        .into_iter()
        .map(|mode| (mode, None))
        .chain(topology.classes.iter().map(|c| ("explicit", Some(&c.cpus))))
    {
        let mut child = Command::new(std::env::current_exe().unwrap());
        child
            .args([
                "--exact",
                "native_class_selection_is_enforced",
                "--nocapture",
            ])
            .env_remove("ZENBENCH_PIN")
            .env_remove("ZENBENCH_CPU_SET")
            .env(
                "ZENBENCH_CPU_POLICY",
                if mode == "explicit" { "auto" } else { mode },
            )
            .env("ZENBENCH_WINARM_CONTROL", mode);
        if let Some(cpus) = cpus {
            child.env(
                "ZENBENCH_CPU_SET",
                cpus.iter()
                    .map(usize::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
            );
        }
        let output = child.output().unwrap();
        assert!(
            output.status.success(),
            "{mode}: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
    }
}
