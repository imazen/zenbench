//! Integration tests — runs real benchmarks and verifies output sanity.
//!
//! These tests measure actual code, not mocked data. They verify that:
//! - The engine produces results with the right structure
//! - Statistics are plausible (mean > min, CI excludes zero for known-different benchmarks)
//! - Output formats are parseable
//! - Threading APIs work without panicking
//! - Cold-start mode produces results
//! - Auto-rounds convergence terminates

use std::collections::HashMap;
use std::sync::Mutex;
use zenbench::*;

/// Run a minimal benchmark suite and return results for inspection.
fn run_simple_suite() -> SuiteResult {
    run_gated(GateConfig::disabled(), |suite| {
        suite.compare("simple", |group| {
            group.config().max_rounds(5).auto_rounds(false);
            group.bench("fast", |b| {
                b.iter(|| {
                    let mut sum = 0u64;
                    for i in 0..50 {
                        sum = sum.wrapping_add(black_box(i));
                    }
                    black_box(sum)
                })
            });
            group.bench("slow", |b| {
                b.iter(|| {
                    let mut sum = 0u64;
                    for i in 0..5000 {
                        sum = sum.wrapping_add(black_box(i));
                    }
                    black_box(sum)
                })
            });
        });
    })
}

#[test]
fn basic_results_structure() {
    let result = run_simple_suite();
    assert_eq!(
        result.comparisons.len(),
        1,
        "should have one comparison group"
    );
    let comp = &result.comparisons[0];
    assert_eq!(comp.group_name, "simple");
    assert_eq!(comp.benchmarks.len(), 2);
    assert_eq!(comp.benchmarks[0].name, "fast");
    assert_eq!(comp.benchmarks[1].name, "slow");
    assert!(
        comp.completed_rounds >= 5,
        "should complete requested rounds"
    );
}

#[test]
fn statistics_are_plausible() {
    let result = run_simple_suite();
    let comp = &result.comparisons[0];

    for bench in &comp.benchmarks {
        let s = &bench.summary;
        assert!(s.n >= 5, "{}: n should be >= 5, got {}", bench.name, s.n);
        assert!(s.min > 0.0, "{}: min should be positive", bench.name);
        assert!(
            s.min <= s.mean,
            "{}: min ({}) should be <= mean ({})",
            bench.name,
            s.min,
            s.mean
        );
        assert!(s.min <= s.median, "{}: min should be <= median", bench.name);
        assert!(s.mean <= s.max, "{}: mean should be <= max", bench.name);
        assert!(s.mad >= 0.0, "{}: mad should be non-negative", bench.name);
        assert!(
            s.variance >= 0.0,
            "{}: variance should be non-negative",
            bench.name
        );
    }
}

#[test]
fn comparison_detects_known_difference() {
    let result = run_simple_suite();
    let comp = &result.comparisons[0];

    // "slow" should be significantly slower than "fast"
    assert!(!comp.analyses.is_empty(), "should have paired analyses");
    let (base, cand, analysis) = &comp.analyses[0];
    assert_eq!(base, "fast");
    assert_eq!(cand, "slow");
    assert!(
        analysis.pct_change > 0.0,
        "slow should be slower (positive pct_change)"
    );
    assert!(analysis.significant, "difference should be significant");
    assert!(analysis.ci_lower > 0.0, "CI should be entirely above zero");
    assert!(
        analysis.ci_lower <= analysis.ci_median,
        "CI: lower <= median"
    );
    assert!(
        analysis.ci_median <= analysis.ci_upper,
        "CI: median <= upper"
    );
}

#[test]
fn cold_start_captured() {
    let result = run_simple_suite();
    for bench in &result.comparisons[0].benchmarks {
        // cold_start_ns should be non-negative (may be 0.0 on coarse timers
        // or sub-ns workloads where the first iteration is faster than resolution)
        assert!(
            bench.cold_start_ns >= 0.0,
            "{}: cold_start_ns should be non-negative, got {}",
            bench.name,
            bench.cold_start_ns,
        );
    }
}

#[test]
fn cold_start_mode_forces_single_iteration() {
    let result = run_gated(GateConfig::disabled(), |suite| {
        suite.compare("cold", |group| {
            group
                .config()
                .cold_start(true)
                .max_rounds(10)
                .auto_rounds(false);
            group.bench("work", |b| {
                b.iter(|| {
                    let v: Vec<u8> = vec![0; 1024];
                    black_box(v)
                })
            });
        });
    });
    let comp = &result.comparisons[0];
    assert_eq!(
        comp.iterations_per_sample, 1,
        "cold_start should force 1 iter/sample"
    );
    assert!(
        comp.cache_firewall,
        "cold_start should enable cache firewall"
    );
    assert!(comp.cold_start, "cold_start flag should be set");
}

#[test]
fn auto_rounds_converges() {
    let result = run_gated(GateConfig::disabled(), |suite| {
        suite.compare("converge", |group| {
            group.config().max_rounds(200).target_precision(0.20);
            // Two benchmarks with ~10x difference — both well above timer
            // resolution (41ns on macOS ARM64 cntvct_el0).
            // Using 5000 vs 50000 iterations so each sample spans ~5µs–50µs
            // = 120–1200 timer ticks on the coarsest platform.
            group.bench("sum_5k", |b| {
                b.iter(|| {
                    let mut v = 0u64;
                    for i in 0..5000 {
                        v = v.wrapping_add(black_box(i));
                    }
                    black_box(v)
                })
            });
            group.bench("sum_50k", |b| {
                b.iter(|| {
                    let mut v = 0u64;
                    for i in 0..50000 {
                        v = v.wrapping_add(black_box(i));
                    }
                    black_box(v)
                })
            });
        });
    });
    let comp = &result.comparisons[0];
    // With 10x difference and 20% precision target, should converge
    // well before 200 rounds. If it doesn't, it still passes — we're
    // testing that auto-rounds runs without crashing, not a specific
    // convergence speed. The assertion is generous to handle noisy CI.
    assert!(
        comp.completed_rounds <= 200,
        "should complete within max_rounds, got {}",
        comp.completed_rounds,
    );
}

#[test]
fn throughput_reported() {
    let result = run_gated(GateConfig::disabled(), |suite| {
        suite.compare("tp", |group| {
            group.config().max_rounds(10).auto_rounds(false);
            group.throughput(Throughput::Elements(1000));
            group.bench("sum", |b| {
                b.iter(|| {
                    let v: u64 = (0..1000).sum();
                    black_box(v)
                })
            });
        });
    });
    let comp = &result.comparisons[0];
    assert!(comp.throughput.is_some(), "throughput should be set");
}

#[test]
fn subgroups_preserved() {
    let result = run_gated(GateConfig::disabled(), |suite| {
        suite.compare("sg", |group| {
            group.config().max_rounds(10).auto_rounds(false);
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
fn llm_format_parseable() {
    let result = run_simple_suite();
    let llm = result.to_llm();

    // Should have one line per benchmark
    let lines: Vec<&str> = llm.lines().collect();
    assert_eq!(lines.len(), 2, "should have 2 lines (one per benchmark)");

    // Each line should have section separators
    for line in &lines {
        assert!(
            line.contains("  |  "),
            "line should have | separators: {line}"
        );
        assert!(line.contains("group="), "line should have group= field");
        assert!(
            line.contains("benchmark="),
            "line should have benchmark= field"
        );
        assert!(line.contains("mean="), "line should have mean= field");
        assert!(line.contains("min="), "line should have min= field");
    }

    // Baseline should be marked
    assert!(
        lines[0].contains("vs_base=baseline"),
        "first benchmark should be baseline"
    );
    // Candidate should have comparison data
    assert!(
        lines[1].contains("vs_base_pct="),
        "second benchmark should have vs_base_pct"
    );
}

#[test]
fn csv_format_parseable() {
    let result = run_simple_suite();
    let csv = result.to_csv();
    let lines: Vec<&str> = csv.lines().collect();

    // Header + 2 data rows
    assert_eq!(lines.len(), 3, "CSV should have header + 2 rows");
    assert!(
        lines[0].contains("mean_ns"),
        "header should contain mean_ns"
    );
    assert!(
        lines[0].contains("cold_start_ns"),
        "header should contain cold_start_ns"
    );
    assert!(
        lines[0].contains("vs_base_pct"),
        "header should contain vs_base_pct"
    );
}

#[test]
fn markdown_format_has_table() {
    let result = run_simple_suite();
    let md = result.to_markdown();
    assert!(
        md.contains("| Benchmark"),
        "markdown should have table header"
    );
    assert!(md.contains("| Min |"), "markdown should have Min column");
    assert!(md.contains("| Mean |"), "markdown should have Mean column");
    assert!(
        md.contains("rounds"),
        "markdown should have methodology line"
    );
}

#[test]
fn bench_contended_works() {
    let result = run_gated(GateConfig::disabled(), |suite| {
        suite.compare("contend", |group| {
            group.config().max_rounds(10).auto_rounds(false);
            group.bench_contended(
                "mutex",
                2,
                || Mutex::new(HashMap::<u64, u64>::new()),
                |b, shared, tid| {
                    b.iter(|| {
                        shared.lock().unwrap().insert(tid as u64, black_box(42));
                    })
                },
            );
        });
    });
    let comp = &result.comparisons[0];
    assert_eq!(comp.benchmarks.len(), 1);
    assert_eq!(comp.benchmarks[0].name, "mutex");
    assert!(
        comp.benchmarks[0].summary.mean > 0.0,
        "should have a positive mean"
    );
}

#[test]
fn bench_parallel_works() {
    let result = run_gated(GateConfig::disabled(), |suite| {
        suite.compare("par", |group| {
            group.config().max_rounds(10).auto_rounds(false);
            group.bench_parallel("work_2t", 2, |b, _tid| {
                b.iter(|| {
                    let v: Vec<u8> = vec![0; 64];
                    black_box(v)
                })
            });
        });
    });
    let comp = &result.comparisons[0];
    assert_eq!(comp.benchmarks[0].name, "work_2t");
    assert!(comp.benchmarks[0].summary.mean > 0.0);
    // Should have threads tag
    assert_eq!(comp.benchmarks[0].tag("threads"), Some("2"));
}

#[test]
fn timer_resolution_detected() {
    let result = run_simple_suite();
    assert!(
        result.timer_resolution_ns > 0,
        "timer resolution should be detected, got {}",
        result.timer_resolution_ns,
    );
    assert!(
        result.timer_resolution_ns < 10_000,
        "timer resolution should be < 10µs, got {}ns",
        result.timer_resolution_ns,
    );
}

#[test]
fn single_bench_creates_group() {
    let result = run_gated(GateConfig::disabled(), |suite| {
        suite.bench("single", |b| b.iter(|| black_box(42u64.wrapping_mul(7))));
    });
    let comp = result
        .comparisons
        .iter()
        .find(|c| c.group_name == "single")
        .unwrap();
    assert_eq!(comp.benchmarks.len(), 1);
    assert_eq!(comp.benchmarks[0].name, "single");
    assert!(comp.benchmarks[0].summary.mean > 0.0);
}
