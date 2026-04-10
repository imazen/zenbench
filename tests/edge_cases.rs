//! Edge case tests — exercise every API path including adversarial usage.
//!
//! These test the "asinine possibilities": empty groups, zero threads,
//! conflicting configs, etc. If it compiles, it shouldn't panic.

use std::sync::Mutex;
use zenbench::*;

fn disabled_gate() -> GateConfig {
    GateConfig::disabled()
}

// ── Empty / minimal cases ──────────────────────────────────────────

#[test]
fn empty_suite() {
    let result = run_gated(disabled_gate(), |_suite| {
        // no benchmarks at all
    });
    assert!(result.comparisons.is_empty());
}

#[test]
fn single_benchmark_in_group() {
    // warning printed but shouldn't panic
    let result = run_gated(disabled_gate(), |suite| {
        suite.compare("solo", |group| {
            group.config().max_rounds(5).auto_rounds(false);
            group.bench("only_one", |b| b.iter(|| black_box(42)));
        });
    });
    assert_eq!(result.comparisons[0].benchmarks.len(), 1);
    assert!(result.comparisons[0].analyses.is_empty());
}

#[test]
fn empty_group_skipped() {
    let result = run_gated(disabled_gate(), |suite| {
        suite.compare("empty", |_group| {
            // no benchmarks added
        });
    });
    assert!(result.comparisons.is_empty());
}

#[test]
fn single_bench_creates_group() {
    let result = run_gated(disabled_gate(), |suite| {
        suite.bench("single", |b| b.iter(|| black_box(1u64)));
    });
    let comp = result
        .comparisons
        .iter()
        .find(|c| c.group_name == "single")
        .unwrap();
    assert_eq!(comp.benchmarks.len(), 1);
    assert!(comp.benchmarks[0].summary.mean > 0.0);
}

// ── suite.bench() unification edge cases ──────────────────────────

#[test]
fn bench_group_name_equals_bench_name() {
    let result = run_gated(disabled_gate(), |suite| {
        suite.bench("same_name", |b| b.iter(|| black_box(1u64)));
    });
    let comp = &result.comparisons[0];
    assert_eq!(comp.group_name, "same_name");
    assert_eq!(comp.benchmarks[0].name, "same_name");
}

#[test]
fn bench_with_empty_name() {
    let result = run_gated(disabled_gate(), |suite| {
        suite.bench("", |b| b.iter(|| black_box(1u64)));
    });
    assert_eq!(result.comparisons.len(), 1);
    assert_eq!(result.comparisons[0].group_name, "");
    assert_eq!(result.comparisons[0].benchmarks[0].name, "");
}

#[test]
fn bench_with_special_chars_in_name() {
    let result = run_gated(disabled_gate(), |suite| {
        suite.bench("decode/jpeg (4K×2K)", |b| b.iter(|| black_box(1u64)));
    });
    let comp = &result.comparisons[0];
    assert_eq!(comp.group_name, "decode/jpeg (4K×2K)");
}

#[test]
fn bench_produces_no_analyses() {
    // Single-bench group has no pairs to analyze
    let result = run_gated(disabled_gate(), |suite| {
        suite.bench("solo", |b| b.iter(|| black_box(1u64)));
    });
    let comp = &result.comparisons[0];
    assert!(
        comp.analyses.is_empty(),
        "single-bench group should have no paired analyses"
    );
}

#[test]
#[allow(deprecated)]
fn standalones_field_always_empty_for_new_results() {
    let result = run_gated(disabled_gate(), |suite| {
        suite.bench("x", |b| b.iter(|| black_box(1u64)));
        suite.compare("y", |g| {
            g.config().max_rounds(5).auto_rounds(false);
            g.bench("a", |b| b.iter(|| black_box(2u64)));
        });
    });
    assert!(
        result.standalones.is_empty(),
        "standalones should always be empty for new results"
    );
    assert_eq!(result.comparisons.len(), 2);
}

#[test]
fn many_bench_calls_all_produce_groups() {
    let result = run_gated(disabled_gate(), |suite| {
        for i in 0..10 {
            let name = format!("bench_{i}");
            suite.bench(name, move |b| b.iter(|| black_box(i as u64)));
        }
    });
    assert_eq!(result.comparisons.len(), 10);
    for (i, comp) in result.comparisons.iter().enumerate() {
        assert_eq!(comp.group_name, format!("bench_{i}"));
        assert_eq!(comp.benchmarks.len(), 1);
    }
}

// ── Config edge cases ──────────────────────────────────────────────

#[test]
fn cold_start_forces_single_iter() {
    let result = run_gated(disabled_gate(), |suite| {
        suite.compare("cold", |group| {
            group
                .config()
                .cold_start(true)
                .max_rounds(5)
                .auto_rounds(false);
            group.bench("work", |b| {
                b.iter(|| {
                    let v: Vec<u8> = vec![0; 256];
                    black_box(v)
                })
            });
        });
    });
    let comp = &result.comparisons[0];
    assert_eq!(comp.iterations_per_sample, 1);
    assert!(comp.cache_firewall);
    assert!(comp.cold_start);
}

#[test]
fn min_rounds_respected() {
    let result = run_gated(disabled_gate(), |suite| {
        suite.compare("minr", |group| {
            group
                .config()
                .min_rounds(10)
                .max_rounds(10)
                .auto_rounds(false);
            group.bench("a", |b| b.iter(|| black_box(1)));
            group.bench("b", |b| b.iter(|| black_box(2)));
        });
    });
    assert!(result.comparisons[0].completed_rounds >= 10);
}

#[test]
fn max_wall_time_stops_measurement() {
    let result = run_gated(disabled_gate(), |suite| {
        suite.compare("wt", |group| {
            group
                .config()
                .max_rounds(100000)
                .max_wall_time(std::time::Duration::from_millis(200))
                .auto_rounds(false);
            group.bench("a", |b| b.iter(|| black_box(1)));
            group.bench("b", |b| b.iter(|| black_box(2)));
        });
    });
    // Should have stopped well before 100K rounds
    assert!(result.comparisons[0].completed_rounds < 100000);
}

#[test]
fn sort_by_speed_does_not_crash() {
    let result = run_gated(disabled_gate(), |suite| {
        suite.compare("sorted", |group| {
            group
                .config()
                .sort_by_speed(true)
                .max_rounds(5)
                .auto_rounds(false);
            group.bench("fast", |b| b.iter(|| black_box(1)));
            group.bench("slow", |b| {
                b.iter(|| {
                    for i in 0..100 {
                        black_box(i);
                    }
                })
            });
        });
    });
    assert_eq!(result.comparisons[0].benchmarks.len(), 2);
}

#[test]
fn expect_sub_ns_does_not_crash() {
    let result = run_gated(disabled_gate(), |suite| {
        suite.compare("subns", |group| {
            group
                .config()
                .expect_sub_ns(true)
                .max_rounds(5)
                .auto_rounds(false);
            group.bench("noop", |b| b.iter(|| black_box(())));
        });
    });
    assert!(result.comparisons[0].expect_sub_ns);
    assert!(result.comparisons[0].benchmarks[0].summary.mean > 0.0);
}

#[test]
fn baseline_only_explicit() {
    let result = run_gated(disabled_gate(), |suite| {
        suite.compare("bo", |group| {
            group
                .config()
                .baseline_only(true)
                .max_rounds(5)
                .auto_rounds(false);
            group.bench("a", |b| b.iter(|| black_box(1)));
            group.bench("b", |b| b.iter(|| black_box(2)));
            group.bench("c", |b| b.iter(|| black_box(3)));
        });
    });
    // Should only have baseline comparisons (a vs b, a vs c), not b vs c
    let comp = &result.comparisons[0];
    assert!(comp.baseline_only);
    let analyses_with_a: Vec<_> = comp
        .analyses
        .iter()
        .filter(|(base, _, _)| base == "a")
        .collect();
    assert_eq!(analyses_with_a.len(), 2);
    let analyses_without_a: Vec<_> = comp
        .analyses
        .iter()
        .filter(|(base, _, _)| base != "a")
        .collect();
    assert!(analyses_without_a.is_empty());
}

// ── Throughput ──────────────────────────────────────────────────────

#[test]
fn throughput_bytes() {
    let result = run_gated(disabled_gate(), |suite| {
        suite.compare("tp", |group| {
            group.config().max_rounds(5).auto_rounds(false);
            group.throughput(Throughput::Bytes(1024));
            group.bench("copy", |b| {
                b.iter(|| {
                    let v: Vec<u8> = vec![0; 1024];
                    black_box(v)
                })
            });
        });
    });
    let tp = result.comparisons[0].throughput.as_ref().unwrap();
    let (val, unit) = tp.compute(1_000_000.0, None); // 1ms
    assert!(val > 0.0);
    assert!(unit.contains("iB/s"));
}

#[test]
fn throughput_elements_with_unit() {
    let result = run_gated(disabled_gate(), |suite| {
        suite.compare("tp", |group| {
            group.config().max_rounds(5).auto_rounds(false);
            group.throughput(Throughput::Elements(100));
            group.throughput_unit("pixels");
            group.bench("process", |b| b.iter(|| black_box(42)));
        });
    });
    let comp = &result.comparisons[0];
    assert_eq!(comp.throughput_unit.as_deref(), Some("pixels"));
    let tp = comp.throughput.as_ref().unwrap();
    let (_, unit) = tp.compute(1_000_000.0, Some("pixels"));
    assert!(unit.contains("pixels"));
}

#[test]
fn element_count() {
    assert_eq!(Throughput::Elements(42).element_count(), Some(42));
    assert_eq!(Throughput::Bytes(42).element_count(), None);
}

// ── Subgroups and baseline ─────────────────────────────────────────

#[test]
fn subgroups_work() {
    let result = run_gated(disabled_gate(), |suite| {
        suite.compare("sg", |group| {
            group.config().max_rounds(5).auto_rounds(false);
            group.subgroup("alpha");
            group.bench("a1", |b| b.iter(|| black_box(1)));
            group.subgroup("beta");
            group.bench("b1", |b| b.iter(|| black_box(2)));
        });
    });
    let comp = &result.comparisons[0];
    assert_eq!(comp.benchmarks[0].subgroup.as_deref(), Some("alpha"));
    assert_eq!(comp.benchmarks[1].subgroup.as_deref(), Some("beta"));
}

#[test]
fn explicit_baseline() {
    let result = run_gated(disabled_gate(), |suite| {
        suite.compare("bl", |group| {
            group.config().max_rounds(5).auto_rounds(false);
            group.baseline("second");
            group.bench("first", |b| b.iter(|| black_box(1)));
            group.bench("second", |b| b.iter(|| black_box(2)));
        });
    });
    // "second" should be the baseline in analyses
    let comp = &result.comparisons[0];
    if !comp.analyses.is_empty() {
        assert_eq!(comp.analyses[0].0, "second");
    }
}

// ── Threading ──────────────────────────────────────────────────────

#[test]
fn bench_contended_zero_threads_becomes_one() {
    // threads=0 should be clamped to 1, not panic
    let result = run_gated(disabled_gate(), |suite| {
        suite.compare("c0", |group| {
            group.config().max_rounds(5).auto_rounds(false);
            group.bench_contended(
                "zero",
                0,
                || Mutex::new(0u64),
                |b, shared, _tid| {
                    b.iter(|| {
                        *shared.lock().unwrap() += 1;
                    })
                },
            );
        });
    });
    assert_eq!(result.comparisons[0].benchmarks[0].name, "zero");
}

#[test]
fn bench_parallel_one_thread() {
    let result = run_gated(disabled_gate(), |suite| {
        suite.compare("p1", |group| {
            group.config().max_rounds(5).auto_rounds(false);
            group.bench_parallel("single", 1, |b, _tid| b.iter(|| black_box(42)));
        });
    });
    assert_eq!(
        result.comparisons[0].benchmarks[0].tag("threads"),
        Some("1")
    );
}

#[test]
fn bench_tagged() {
    let result = run_gated(disabled_gate(), |suite| {
        suite.compare("tags", |group| {
            group.config().max_rounds(5).auto_rounds(false);
            group.bench_tagged("tagged", &[("lib", "foo"), ("level", "3")], |b| {
                b.iter(|| black_box(42))
            });
        });
    });
    let bench = &result.comparisons[0].benchmarks[0];
    assert_eq!(bench.tag("lib"), Some("foo"));
    assert_eq!(bench.tag("level"), Some("3"));
    assert_eq!(bench.tag("missing"), None);
}

// ── with_input ─────────────────────────────────────────────────────

#[test]
fn with_input_excludes_setup() {
    let result = run_gated(disabled_gate(), |suite| {
        suite.compare("wi", |group| {
            group.config().max_rounds(5).auto_rounds(false);
            group.bench("expensive_setup", |b| {
                b.with_input(|| {
                    // Expensive setup — should NOT be timed
                    (0..10000).collect::<Vec<i32>>()
                })
                .run(|mut v| {
                    v.sort();
                    black_box(v)
                })
            });
        });
    });
    assert!(result.comparisons[0].benchmarks[0].summary.mean > 0.0);
}

// ── Output formats ─────────────────────────────────────────────────

#[test]
fn save_and_load_roundtrip() {
    let result = run_gated(disabled_gate(), |suite| {
        suite.compare("rt", |group| {
            group.config().max_rounds(5).auto_rounds(false);
            group.bench("a", |b| b.iter(|| black_box(1)));
        });
    });
    let path = std::env::temp_dir().join("zenbench_test_roundtrip.json");
    result.save(&path).unwrap();
    let loaded = SuiteResult::load(&path).unwrap();
    assert_eq!(loaded.comparisons[0].group_name, "rt");
    assert_eq!(loaded.comparisons[0].benchmarks[0].name, "a");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn group_by_tag_works() {
    let result = run_gated(disabled_gate(), |suite| {
        suite.compare("gbt", |group| {
            group.config().max_rounds(5).auto_rounds(false);
            group.bench_tagged("a", &[("lib", "x")], |b| b.iter(|| black_box(1)));
            group.bench_tagged("b", &[("lib", "y")], |b| b.iter(|| black_box(2)));
        });
    });
    let grouped = result.group_by_tag("lib");
    assert!(grouped.contains_key("x"));
    assert!(grouped.contains_key("y"));
}

// ── Gate configs ───────────────────────────────────────────────────

#[test]
fn gate_disabled_runs_without_waits() {
    let result = run_gated(GateConfig::disabled(), |suite| {
        suite.compare("gd", |group| {
            group.config().max_rounds(5).auto_rounds(false);
            group.bench("a", |b| b.iter(|| black_box(1)));
        });
    });
    assert_eq!(result.gate_waits, 0);
}

#[test]
fn gate_ci_config_does_not_panic() {
    let result = run_gated(GateConfig::ci(), |suite| {
        suite.compare("gci", |group| {
            group.config().max_rounds(5).auto_rounds(false);
            group.bench("a", |b| b.iter(|| black_box(1)));
        });
    });
    assert!(!result.comparisons.is_empty());
}

#[test]
fn gate_strict_config_does_not_panic() {
    // strict mode won't actually flag anything in a 5-round test
    let result = run_gated(GateConfig::strict(), |suite| {
        suite.compare("gs", |group| {
            group.config().max_rounds(5).auto_rounds(false);
            group.bench("a", |b| b.iter(|| black_box(1)));
        });
    });
    assert!(!result.comparisons.is_empty());
}

// ── Summary API ────────────────────────────────────────────────────

#[test]
fn summary_push_and_stats() {
    let mut s = Summary::new();
    for v in [10.0, 20.0, 30.0, 40.0, 50.0] {
        s.push(v);
    }
    assert_eq!(s.n, 5);
    assert!((s.mean - 30.0).abs() < 0.01);
    assert!(s.std_dev() > 0.0);
    assert!(s.std_err() > 0.0);
    assert!(s.cv() > 0.0);
    assert_eq!(s.min, 10.0);
    assert_eq!(s.max, 50.0);
}

#[test]
fn summary_from_slice_empty() {
    let s = Summary::from_slice(&[]);
    assert_eq!(s.n, 0);
}

#[test]
fn summary_from_slice_single() {
    let s = Summary::from_slice(&[42.0]);
    assert_eq!(s.n, 1);
    assert!((s.mean - 42.0).abs() < 0.01);
    assert!((s.std_dev() - 0.0).abs() < 0.01);
}

// ── Group filter ───────────────────────────────────────────────────

#[test]
fn group_filter_skips_non_matching() {
    let result = run_gated(disabled_gate(), |suite| {
        suite.set_group_filter("match_me".to_string());
        suite.compare("match_me", |group| {
            group.config().max_rounds(5).auto_rounds(false);
            group.bench("a", |b| b.iter(|| black_box(1)));
        });
        suite.compare("skip_me", |group| {
            group.config().max_rounds(5).auto_rounds(false);
            group.bench("b", |b| b.iter(|| black_box(2)));
        });
    });
    assert_eq!(result.comparisons.len(), 1);
    assert_eq!(result.comparisons[0].group_name, "match_me");
}
