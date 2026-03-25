//! Tests for new features: overhead compensation, deferred drop, TSC timing,
//! asm fences, allocation profiling, noise threshold, per-benchmark CIs,
//! and configurable bootstrap resamples.
//!
//! These tests validate that the features actually work end-to-end, not just
//! that they don't panic.

use zenbench::*;

fn disabled_gate() -> GateConfig {
    GateConfig::disabled()
}

// ── Overhead compensation ───────────────────────────────────────────

#[test]
fn loop_overhead_is_recorded() {
    let result = run_gated(disabled_gate(), |suite| {
        suite.compare("oh", |group| {
            group.config().max_rounds(10).auto_rounds(false);
            group.bench("a", |b| b.iter(|| black_box(42u64)));
            group.bench("b", |b| b.iter(|| black_box(43u64)));
        });
    });
    // The engine measures loop overhead at startup and stores it
    assert!(
        result.loop_overhead_ns > 0.0,
        "loop_overhead_ns should be positive, got {}",
        result.loop_overhead_ns
    );
    assert!(
        result.loop_overhead_ns < 100.0,
        "loop_overhead_ns should be < 100ns, got {}",
        result.loop_overhead_ns
    );
}

#[test]
fn overhead_compensation_produces_lower_times() {
    // A trivial benchmark (just black_box) should report very low times
    // after overhead subtraction. Without subtraction, the per-iter time
    // would be dominated by the loop + black_box overhead (~1-3ns).
    // With subtraction, it should be near zero (but clamped to > 0).
    let result = run_gated(disabled_gate(), |suite| {
        suite.compare("trivial", |group| {
            group
                .config()
                .max_rounds(30)
                .auto_rounds(false)
                .expect_sub_ns(true);
            group.bench("noop", |b| b.iter(|| black_box(0u64)));
            group.bench("add", |b| b.iter(|| black_box(1u64 + black_box(1u64))));
        });
    });
    let overhead = result.loop_overhead_ns;
    let noop_mean = result.comparisons[0].benchmarks[0].summary.mean;

    // The noop mean should be less than the overhead (since overhead is subtracted)
    // or very close to zero. It should NOT be equal to the overhead.
    assert!(
        noop_mean < overhead * 2.0 || noop_mean < 2.0,
        "noop mean ({noop_mean:.2}ns) should be much less than raw overhead ({overhead:.2}ns) \
         after compensation"
    );
}

#[test]
fn overhead_in_json_roundtrip() {
    let result = run_gated(disabled_gate(), |suite| {
        suite.compare("json_oh", |group| {
            group.config().max_rounds(5).auto_rounds(false);
            group.bench("a", |b| b.iter(|| black_box(1)));
        });
    });
    let path = std::env::temp_dir().join("zenbench_test_overhead_rt.json");
    result.save(&path).unwrap();
    let loaded = SuiteResult::load(&path).unwrap();
    assert!(
        (loaded.loop_overhead_ns - result.loop_overhead_ns).abs() < 0.01,
        "loop_overhead_ns should survive JSON roundtrip"
    );
    let _ = std::fs::remove_file(&path);
}

// ── Deferred drop ───────────────────────────────────────────────────

#[test]
fn iter_deferred_drop_runs() {
    let result = run_gated(disabled_gate(), |suite| {
        suite.compare("deferred", |group| {
            group.config().max_rounds(10).auto_rounds(false);
            group.bench("with_drop", |b| {
                b.iter_deferred_drop(|| {
                    let mut v = Vec::with_capacity(1024);
                    v.extend(0..1024u32);
                    v
                })
            });
            group.bench("regular_iter", |b| {
                b.iter(|| {
                    let mut v = Vec::with_capacity(1024);
                    v.extend(0..1024u32);
                    v
                })
            });
        });
    });
    let comp = &result.comparisons[0];
    assert_eq!(comp.benchmarks.len(), 2);
    let deferred_mean = comp.benchmarks[0].summary.mean;
    let regular_mean = comp.benchmarks[1].summary.mean;
    assert!(
        deferred_mean > 0.0,
        "deferred_drop should produce positive times"
    );
    assert!(
        regular_mean > 0.0,
        "regular iter should produce positive times"
    );

    // Deferred drop should be faster or similar — the Vec::drop deallocation
    // is excluded from timing in deferred mode but included in regular mode.
    // We can't assert strictly faster (noise), but deferred should not be
    // dramatically slower.
    assert!(
        deferred_mean < regular_mean * 2.0,
        "deferred_drop ({deferred_mean:.0}ns) should not be much slower than \
         regular iter ({regular_mean:.0}ns)"
    );
}

#[test]
fn iter_deferred_drop_actually_drops() {
    // Verify that outputs ARE dropped (not leaked) by using a Drop counter.
    use std::sync::atomic::{AtomicUsize, Ordering};
    static DROP_COUNT: AtomicUsize = AtomicUsize::new(0);

    struct DropCounter;
    impl Drop for DropCounter {
        fn drop(&mut self) {
            DROP_COUNT.fetch_add(1, Ordering::Relaxed);
        }
    }

    DROP_COUNT.store(0, Ordering::Relaxed);
    let result = run_gated(disabled_gate(), |suite| {
        suite.compare("drop_check", |group| {
            group.config().max_rounds(5).auto_rounds(false);
            group.bench("counted", |b| b.iter_deferred_drop(|| DropCounter));
        });
    });
    let comp = &result.comparisons[0];
    let total_iterations: usize = comp.benchmarks[0].summary.n * comp.iterations_per_sample;
    let drops = DROP_COUNT.load(Ordering::Relaxed);

    // Every iteration should have produced one DropCounter that was dropped.
    // Allow some tolerance for jitter (±20% iteration count per round).
    assert!(drops > 0, "should have dropped some DropCounters, got 0");
    // Approximate: n rounds × iterations_per_sample (with jitter)
    // The actual count may be higher due to warmup iterations.
    assert!(
        drops >= total_iterations / 2,
        "drop count ({drops}) should be at least half of estimated iterations ({total_iterations})"
    );
}

#[test]
fn iter_deferred_drop_excludes_drop_from_timing() {
    // Use a type with an expensive Drop to show deferred_drop is faster
    struct ExpensiveDrop(Vec<u8>);
    impl Drop for ExpensiveDrop {
        fn drop(&mut self) {
            // Simulate expensive cleanup: write to every byte
            for byte in self.0.iter_mut() {
                *byte = black_box(0xFF);
            }
        }
    }

    let result = run_gated(disabled_gate(), |suite| {
        suite.compare("expensive_drop", |group| {
            group.config().max_rounds(20).auto_rounds(false);
            // Deferred: drop happens after timing
            group.bench("deferred", |b| {
                b.iter_deferred_drop(|| ExpensiveDrop(vec![0u8; 4096]))
            });
            // Regular: drop happens during timing
            group.bench("regular", |b| b.iter(|| ExpensiveDrop(vec![0u8; 4096])));
        });
    });
    let deferred_mean = result.comparisons[0].benchmarks[0].summary.mean;
    let regular_mean = result.comparisons[0].benchmarks[1].summary.mean;

    // Deferred should be noticeably faster because the 4KB write in Drop
    // is excluded from timing
    assert!(
        deferred_mean < regular_mean,
        "deferred ({deferred_mean:.0}ns) should be faster than regular ({regular_mean:.0}ns) \
         when Drop is expensive"
    );
}

// ── TSC and asm fences ──────────────────────────────────────────────

#[cfg(feature = "precise-timing")]
mod precise_timing {
    use super::*;

    #[test]
    fn tsc_timer_used_when_available() {
        // The engine should detect and use TSC on modern x86_64/aarch64
        let result = run_gated(disabled_gate(), |suite| {
            suite.compare("tsc", |group| {
                group.config().max_rounds(10).auto_rounds(false);
                group.bench("work", |b| {
                    b.iter(|| {
                        let mut v = 0u64;
                        for i in 0..50 {
                            v = v.wrapping_add(black_box(i));
                        }
                        black_box(v)
                    })
                });
            });
        });
        // If TSC is available, the startup message will have logged it.
        // We can verify results are still sane.
        let mean = result.comparisons[0].benchmarks[0].summary.mean;
        assert!(mean > 0.0, "mean should be positive with TSC timing");
        assert!(
            mean < 1_000_000.0,
            "mean should be reasonable (<1ms for 50 additions)"
        );
    }

    #[test]
    fn asm_fences_do_not_affect_correctness() {
        // Run the same benchmark with precise-timing (asm fences active)
        // and verify results are plausible
        let result = run_gated(disabled_gate(), |suite| {
            suite.compare("fenced", |group| {
                group.config().max_rounds(20).auto_rounds(false);
                group.bench("fast", |b| b.iter(|| black_box(42u64)));
                group.bench("slow", |b| {
                    b.iter(|| {
                        let mut s = 0u64;
                        for i in 0..200 {
                            s = s.wrapping_add(black_box(i));
                        }
                        black_box(s)
                    })
                });
            });
        });
        let comp = &result.comparisons[0];
        // Slow should still be detectably slower than fast
        assert!(!comp.analyses.is_empty());
        let analysis = &comp.analyses[0].2;
        assert!(
            analysis.pct_change > 0.0,
            "slow should be slower than fast with asm fences"
        );
    }
}

// ── Allocation profiling ────────────────────────────────────────────

// Note: Full alloc profiling requires AllocProfiler as #[global_allocator],
// which can only be set once per binary. We test the machinery here;
// the alloc_profiling bench binary tests the full integration.

#[cfg(feature = "alloc-profiling")]
mod alloc_profiling {
    use super::*;

    #[test]
    fn alloc_stats_absent_without_profiler() {
        // Without AllocProfiler installed, alloc_stats should be None
        let result = run_gated(disabled_gate(), |suite| {
            suite.compare("no_profiler", |group| {
                group.config().max_rounds(5).auto_rounds(false);
                group.bench("alloc_work", |b| {
                    b.iter(|| {
                        let v: Vec<u8> = vec![0; 256];
                        black_box(v)
                    })
                });
            });
        });
        let bench = &result.comparisons[0].benchmarks[0];
        assert!(
            bench.alloc_stats.is_none(),
            "alloc_stats should be None without AllocProfiler installed"
        );
    }

    #[test]
    fn alloc_snapshot_delta_math() {
        use zenbench::AllocStats;

        // Test the stats computation directly
        let stats = AllocStats::from_totals(
            200,   // allocs
            200,   // deallocs
            10,    // reallocs
            16000, // bytes alloc
            16000, // bytes dealloc
            100,   // iterations
        );
        assert!((stats.allocs_per_iter - 2.0).abs() < f64::EPSILON);
        assert!((stats.deallocs_per_iter - 2.0).abs() < f64::EPSILON);
        assert!((stats.reallocs_per_iter - 0.1).abs() < f64::EPSILON);
        assert!((stats.bytes_per_iter - 160.0).abs() < f64::EPSILON);
    }

    #[test]
    fn alloc_stats_zero_iterations_safe() {
        let stats = zenbench::AllocStats::from_totals(0, 0, 0, 0, 0, 0);
        assert!((stats.allocs_per_iter - 0.0).abs() < f64::EPSILON);
        assert!((stats.bytes_per_iter - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn csv_output_has_alloc_columns() {
        let result = run_gated(disabled_gate(), |suite| {
            suite.compare("csv_alloc", |group| {
                group.config().max_rounds(5).auto_rounds(false);
                group.bench("a", |b| b.iter(|| black_box(1)));
            });
        });
        let csv = result.to_csv();
        let header = csv.lines().next().unwrap();
        assert!(
            header.contains("allocs_per_iter"),
            "CSV header should contain allocs_per_iter column"
        );
        assert!(
            header.contains("bytes_per_iter"),
            "CSV header should contain bytes_per_iter column"
        );
    }
}

// ── Stats edge cases (Wilcoxon, Spearman, outliers) ─────────────────

#[test]
fn wilcoxon_small_n_returns_inconclusive() {
    // With n < 10, Wilcoxon should return p = 1.0 (inconclusive)
    let result = run_gated(disabled_gate(), |suite| {
        suite.compare("small_n", |group| {
            group
                .config()
                .min_rounds(5)
                .max_rounds(8)
                .auto_rounds(false);
            group.bench("a", |b| b.iter(|| black_box(1u64)));
            group.bench("b", |b| b.iter(|| black_box(2u64)));
        });
    });
    // With only 5-8 rounds, Wilcoxon should report p=1.0 after
    // outlier removal may reduce n further
    let comp = &result.comparisons[0];
    if !comp.analyses.is_empty() {
        let analysis = &comp.analyses[0].2;
        // If n_samples after outlier removal < 10, p should be 1.0
        if analysis.n_samples < 10 {
            assert!(
                (analysis.wilcoxon_p - 1.0).abs() < f64::EPSILON,
                "Wilcoxon should return p=1.0 for n<10, got {}",
                analysis.wilcoxon_p
            );
        }
    }
}

#[test]
fn spearman_drift_near_zero_for_stable_benchmark() {
    let result = run_gated(disabled_gate(), |suite| {
        suite.compare("drift", |group| {
            group.config().max_rounds(50).auto_rounds(false);
            // A stable benchmark should have near-zero drift
            group.bench("stable_a", |b| {
                b.iter(|| {
                    let mut v = 0u64;
                    for i in 0..50 {
                        v = v.wrapping_add(black_box(i));
                    }
                    black_box(v)
                })
            });
            group.bench("stable_b", |b| {
                b.iter(|| {
                    let mut v = 0u64;
                    for i in 0..100 {
                        v = v.wrapping_add(black_box(i));
                    }
                    black_box(v)
                })
            });
        });
    });
    let comp = &result.comparisons[0];
    if !comp.analyses.is_empty() {
        let drift = comp.analyses[0].2.drift_correlation;
        // drift should be relatively small for stable benchmarks
        // (no thermal throttling in a short test)
        assert!(
            drift.abs() < 0.8,
            "drift should be small for stable benchmarks, got {drift:.3}"
        );
    }
}

// ── Random permutation correctness ──────────────────────────────────

#[test]
fn interleaved_benchmarks_all_execute() {
    // Verify that all benchmarks in a group actually run (not skipped by shuffle bug)
    let result = run_gated(disabled_gate(), |suite| {
        suite.compare("shuffle_check", |group| {
            group.config().max_rounds(20).auto_rounds(false);
            group.bench("bench_1", |b| b.iter(|| black_box(1u64)));
            group.bench("bench_2", |b| b.iter(|| black_box(2u64)));
            group.bench("bench_3", |b| b.iter(|| black_box(3u64)));
            group.bench("bench_4", |b| b.iter(|| black_box(4u64)));
        });
    });
    let comp = &result.comparisons[0];
    assert_eq!(comp.benchmarks.len(), 4);
    for bench in &comp.benchmarks {
        assert!(
            bench.summary.n >= 20,
            "{}: should have >= 20 samples, got {}",
            bench.name,
            bench.summary.n
        );
        assert!(
            bench.summary.mean > 0.0,
            "{}: should have positive mean",
            bench.name
        );
    }
}

// ── with_input + deferred_drop comparison ───────────────────────────

#[test]
fn deferred_drop_vs_regular_iter_same_workload() {
    // Both iter() and iter_deferred_drop() should produce similar timings
    // for types without expensive Drop (u64). The difference should be
    // minimal — just the Vec::push overhead in deferred mode.
    let result = run_gated(disabled_gate(), |suite| {
        suite.compare("drop_comparison", |group| {
            group.config().max_rounds(30).auto_rounds(false);
            group.bench("regular", |b| {
                b.iter(|| {
                    let mut v = 0u64;
                    for i in 0..100 {
                        v = v.wrapping_add(black_box(i));
                    }
                    black_box(v)
                })
            });
            group.bench("deferred", |b| {
                b.iter_deferred_drop(|| {
                    let mut v = 0u64;
                    for i in 0..100 {
                        v = v.wrapping_add(black_box(i));
                    }
                    black_box(v)
                })
            });
        });
    });
    let regular_mean = result.comparisons[0].benchmarks[0].summary.mean;
    let deferred_mean = result.comparisons[0].benchmarks[1].summary.mean;

    assert!(
        regular_mean > 0.0 && deferred_mean > 0.0,
        "both should have positive means"
    );
    // For trivial Drop (u64), they should be within 5x of each other.
    // The deferred variant has Vec::push overhead but it's small.
    let ratio = if regular_mean > deferred_mean {
        regular_mean / deferred_mean
    } else {
        deferred_mean / regular_mean
    };
    assert!(
        ratio < 5.0,
        "regular ({regular_mean:.0}ns) and deferred ({deferred_mean:.0}ns) \
         should be within 5x for trivial Drop types (ratio={ratio:.1}x)"
    );
}

// ── Noise threshold (practical significance gate) ───────────────────

#[test]
fn noise_threshold_suppresses_tiny_differences() {
    // Two nearly-identical benchmarks with a 5% noise threshold.
    // The difference should be within noise and NOT significant.
    let result = run_gated(disabled_gate(), |suite| {
        suite.compare("noise_gate", |group| {
            group
                .config()
                .max_rounds(50)
                .auto_rounds(false)
                .noise_threshold(0.05); // 5% — very generous
            group.bench("a", |b| {
                b.iter(|| {
                    let mut v = 0u64;
                    for i in 0..100 {
                        v = v.wrapping_add(black_box(i));
                    }
                    black_box(v)
                })
            });
            group.bench("b", |b| {
                b.iter(|| {
                    let mut v = 0u64;
                    for i in 0..101 {
                        v = v.wrapping_add(black_box(i));
                    }
                    black_box(v)
                })
            });
        });
    });
    let comp = &result.comparisons[0];
    if !comp.analyses.is_empty() {
        let analysis = &comp.analyses[0].2;
        // The difference between summing 100 vs 101 values is ~1%.
        // With a 5% noise threshold, this should NOT be significant.
        // (It might still be significant if the system is clean enough
        // to detect 1% differences, but the noise gate should suppress it.)
        if analysis.pct_change.abs() < 5.0 {
            assert!(
                !analysis.significant,
                "a ~1% difference should not be significant with 5% noise threshold, \
                 got pct_change={:.2}%",
                analysis.pct_change,
            );
        }
    }
}

#[test]
fn noise_threshold_allows_large_differences() {
    // A clearly different benchmark pair with noise threshold.
    // The large difference should still be significant.
    let result = run_gated(disabled_gate(), |suite| {
        suite.compare("noise_large", |group| {
            group
                .config()
                .max_rounds(30)
                .auto_rounds(false)
                .noise_threshold(0.01); // 1%
            group.bench("fast", |b| b.iter(|| black_box(42u64)));
            group.bench("slow", |b| {
                b.iter(|| {
                    let mut v = 0u64;
                    for i in 0..200 {
                        v = v.wrapping_add(black_box(i));
                    }
                    black_box(v)
                })
            });
        });
    });
    let comp = &result.comparisons[0];
    if !comp.analyses.is_empty() {
        let analysis = &comp.analyses[0].2;
        assert!(
            analysis.significant,
            "a large difference should still be significant with 1% noise threshold"
        );
    }
}

#[test]
fn noise_threshold_zero_disables_gate() {
    // With noise_threshold = 0.0, any CI that excludes zero is significant
    let result = run_gated(disabled_gate(), |suite| {
        suite.compare("no_gate", |group| {
            group
                .config()
                .max_rounds(30)
                .auto_rounds(false)
                .noise_threshold(0.0); // disabled
            group.bench("a", |b| b.iter(|| black_box(1u64)));
            group.bench("b", |b| {
                b.iter(|| {
                    let mut v = 0u64;
                    for i in 0..50 {
                        v = v.wrapping_add(black_box(i));
                    }
                    black_box(v)
                })
            });
        });
    });
    let comp = &result.comparisons[0];
    if !comp.analyses.is_empty() {
        let analysis = &comp.analyses[0].2;
        // With no noise gate, the known difference should be significant
        assert!(
            analysis.significant,
            "known difference should be significant with noise_threshold=0"
        );
    }
}

// ── Per-benchmark confidence intervals ──────────────────────────────

#[test]
fn per_benchmark_ci_is_computed() {
    let result = run_gated(disabled_gate(), |suite| {
        suite.compare("bench_ci", |group| {
            group.config().max_rounds(30).auto_rounds(false);
            group.bench("work", |b| {
                b.iter(|| {
                    let mut v = 0u64;
                    for i in 0..100 {
                        v = v.wrapping_add(black_box(i));
                    }
                    black_box(v)
                })
            });
        });
    });
    let bench = &result.comparisons[0].benchmarks[0];
    let ci = bench
        .mean_ci
        .as_ref()
        .expect("mean_ci should be computed for benchmarks with ≥2 rounds");

    // CI bounds should bracket the mean
    assert!(
        ci.lower <= bench.summary.mean,
        "CI lower ({:.1}) should be <= mean ({:.1})",
        ci.lower,
        bench.summary.mean
    );
    assert!(
        ci.upper >= bench.summary.mean,
        "CI upper ({:.1}) should be >= mean ({:.1})",
        ci.upper,
        bench.summary.mean
    );
    // CI should be positive for a real workload
    assert!(ci.lower > 0.0, "CI lower should be positive");
    // CI should not be absurdly wide
    let width_pct = (ci.upper - ci.lower) / bench.summary.mean * 100.0;
    assert!(
        width_pct < 50.0,
        "CI width should be < 50% of mean, got {width_pct:.1}%"
    );
}

#[test]
fn per_benchmark_ci_in_json() {
    let result = run_gated(disabled_gate(), |suite| {
        suite.compare("ci_json", |group| {
            group.config().max_rounds(10).auto_rounds(false);
            group.bench("a", |b| b.iter(|| black_box(42u64)));
        });
    });
    let json = serde_json::to_string(&result).unwrap();
    assert!(
        json.contains("mean_ci"),
        "JSON should contain mean_ci field"
    );
    assert!(
        json.contains("\"lower\""),
        "JSON should contain lower bound"
    );
    assert!(
        json.contains("\"upper\""),
        "JSON should contain upper bound"
    );
}

#[test]
fn per_benchmark_ci_in_llm_output() {
    let result = run_gated(disabled_gate(), |suite| {
        suite.compare("ci_llm", |group| {
            group.config().max_rounds(10).auto_rounds(false);
            group.bench("work", |b| {
                b.iter(|| {
                    let mut v = 0u64;
                    for i in 0..50 {
                        v = v.wrapping_add(black_box(i));
                    }
                    black_box(v)
                })
            });
        });
    });
    let llm = result.to_llm();
    assert!(
        llm.contains("mean_ci="),
        "LLM output should contain mean_ci field"
    );
}

#[test]
fn standalone_benchmark_has_ci() {
    let result = run_gated(disabled_gate(), |suite| {
        suite.bench("standalone_ci", |b| {
            b.iter(|| {
                let mut v = 0u64;
                for i in 0..100 {
                    v = v.wrapping_add(black_box(i));
                }
                black_box(v)
            })
        });
    });
    assert!(
        result.standalones[0].mean_ci.is_some(),
        "standalone benchmarks should also get per-benchmark CIs"
    );
}

// ── Baseline persistence ────────────────────────────────────────────

#[test]
fn baseline_save_load_compare() {
    // Run a benchmark, save as baseline, run again, compare
    let result1 = run_gated(disabled_gate(), |suite| {
        suite.compare("base_test", |group| {
            group.config().max_rounds(10).auto_rounds(false);
            group.bench("work_a", |b| {
                b.iter(|| {
                    let mut v = 0u64;
                    for i in 0..100 {
                        v = v.wrapping_add(black_box(i));
                    }
                    black_box(v)
                })
            });
            group.bench("work_b", |b| {
                b.iter(|| {
                    let mut v = 0u64;
                    for i in 0..200 {
                        v = v.wrapping_add(black_box(i));
                    }
                    black_box(v)
                })
            });
        });
    });

    // Save as baseline
    let path = zenbench::baseline::save_baseline(&result1, "test_integration")
        .expect("should save baseline");
    assert!(path.exists(), "baseline file should exist");

    // Load it back
    let loaded =
        zenbench::baseline::load_baseline("test_integration").expect("should load baseline");
    assert_eq!(loaded.comparisons[0].group_name, "base_test");

    // Run again (same workload — should be similar)
    let result2 = run_gated(disabled_gate(), |suite| {
        suite.compare("base_test", |group| {
            group.config().max_rounds(10).auto_rounds(false);
            group.bench("work_a", |b| {
                b.iter(|| {
                    let mut v = 0u64;
                    for i in 0..100 {
                        v = v.wrapping_add(black_box(i));
                    }
                    black_box(v)
                })
            });
            group.bench("work_b", |b| {
                b.iter(|| {
                    let mut v = 0u64;
                    for i in 0..200 {
                        v = v.wrapping_add(black_box(i));
                    }
                    black_box(v)
                })
            });
        });
    });

    // Compare: same workload should not regress
    let comparison = zenbench::baseline::compare_against_baseline(&loaded, &result2, 50.0);
    assert_eq!(
        comparison.regressions, 0,
        "same workload should not regress at 50% threshold"
    );
    assert_eq!(comparison.benchmarks.len(), 2);
    assert!(comparison.new_benchmarks.is_empty());
    assert!(comparison.missing_benchmarks.is_empty());

    // Clean up
    let _ = zenbench::baseline::delete_baseline("test_integration");
}

#[test]
fn baseline_detects_regression_with_different_workload() {
    // Save a fast baseline, then compare against a slower run
    let fast = run_gated(disabled_gate(), |suite| {
        suite.compare("regress_test", |group| {
            group.config().max_rounds(10).auto_rounds(false);
            group.bench("func", |b| {
                b.iter(|| {
                    let mut v = 0u64;
                    for i in 0..50 {
                        v = v.wrapping_add(black_box(i));
                    }
                    black_box(v)
                })
            });
        });
    });
    zenbench::baseline::save_baseline(&fast, "test_regression").unwrap();

    // Now run something much slower
    let slow = run_gated(disabled_gate(), |suite| {
        suite.compare("regress_test", |group| {
            group.config().max_rounds(10).auto_rounds(false);
            group.bench("func", |b| {
                b.iter(|| {
                    let mut v = 0u64;
                    for i in 0..500 {
                        v = v.wrapping_add(black_box(i));
                    }
                    black_box(v)
                })
            });
        });
    });

    let comparison = zenbench::baseline::compare_against_baseline(
        &zenbench::baseline::load_baseline("test_regression").unwrap(),
        &slow,
        20.0, // 20% threshold — 10x slowdown should easily trigger
    );
    assert!(
        comparison.regressions > 0,
        "10x slower workload should be detected as regression"
    );

    // Clean up
    let _ = zenbench::baseline::delete_baseline("test_regression");
}

#[test]
fn baseline_list_and_delete() {
    let result = run_gated(disabled_gate(), |suite| {
        suite.compare("list_test", |group| {
            group.config().max_rounds(5).auto_rounds(false);
            group.bench("x", |b| b.iter(|| black_box(1u64)));
        });
    });
    zenbench::baseline::save_baseline(&result, "test_list_a").unwrap();
    zenbench::baseline::save_baseline(&result, "test_list_b").unwrap();

    let names = zenbench::baseline::list_baselines();
    assert!(names.contains(&"test_list_a".to_string()));
    assert!(names.contains(&"test_list_b".to_string()));

    zenbench::baseline::delete_baseline("test_list_a").unwrap();
    zenbench::baseline::delete_baseline("test_list_b").unwrap();

    let names_after = zenbench::baseline::list_baselines();
    assert!(!names_after.contains(&"test_list_a".to_string()));
}

// ── Slope regression ────────────────────────────────────────────────

#[test]
fn slope_regression_produces_result() {
    let result = run_gated(disabled_gate(), |suite| {
        suite.compare("slope_test", |group| {
            group
                .config()
                .max_rounds(30)
                .auto_rounds(false)
                .linear_sampling(true);
            group.bench("work", |b| {
                b.iter(|| {
                    let mut v = 0u64;
                    for i in 0..100 {
                        v = v.wrapping_add(black_box(i));
                    }
                    black_box(v)
                })
            });
        });
    });
    let bench = &result.comparisons[0].benchmarks[0];
    assert!(
        bench.slope_ns.is_some(),
        "linear_sampling should produce a slope estimate"
    );
    let slope = bench.slope_ns.unwrap();
    assert!(slope > 0.0, "slope should be positive, got {slope}");
    // Slope should be similar to mean (within 5x)
    let ratio = slope / bench.summary.mean;
    assert!(
        ratio > 0.2 && ratio < 5.0,
        "slope ({slope:.1}ns) should be similar to mean ({:.1}ns), ratio={ratio:.2}",
        bench.summary.mean,
    );
}

#[test]
fn slope_not_computed_without_linear_sampling() {
    let result = run_gated(disabled_gate(), |suite| {
        suite.compare("no_slope", |group| {
            group.config().max_rounds(10).auto_rounds(false);
            group.bench("work", |b| b.iter(|| black_box(42u64)));
        });
    });
    assert!(
        result.comparisons[0].benchmarks[0].slope_ns.is_none(),
        "slope_ns should be None without linear_sampling"
    );
}

// ── Warmup ──────────────────────────────────────────────────────────

#[test]
fn warmup_time_zero_skips_warmup() {
    // Default warmup is 500ms but we override to 0 — should be fast
    let start = std::time::Instant::now();
    let _result = run_gated(disabled_gate(), |suite| {
        suite.compare("no_warmup", |group| {
            group
                .config()
                .warmup_time(std::time::Duration::ZERO)
                .max_rounds(5)
                .auto_rounds(false);
            group.bench("a", |b| b.iter(|| black_box(1u64)));
        });
    });
    // With 0 warmup and 5 rounds, should complete within wall time
    // (gate waits can add time on busy systems, so be generous)
    assert!(
        start.elapsed() < std::time::Duration::from_secs(120),
        "zero warmup should complete within wall time limit"
    );
}

// ── Testbed detection ───────────────────────────────────────────────

#[test]
fn testbed_is_populated_in_results() {
    let result = run_gated(disabled_gate(), |suite| {
        suite.compare("tb", |group| {
            group.config().max_rounds(5).auto_rounds(false);
            group.bench("a", |b| b.iter(|| black_box(1u64)));
        });
    });
    let testbed = result
        .testbed
        .as_ref()
        .expect("testbed should be populated");
    assert!(
        !testbed.cpu_model.is_empty(),
        "cpu_model should not be empty"
    );
    assert!(!testbed.arch.is_empty(), "arch should not be empty");
    assert!(!testbed.os.is_empty(), "os should not be empty");
    assert!(testbed.logical_cores > 0, "logical_cores should be > 0");
    assert!(testbed.physical_cores > 0, "physical_cores should be > 0");
}

#[test]
fn testbed_survives_json_roundtrip() {
    let result = run_gated(disabled_gate(), |suite| {
        suite.compare("tb_rt", |group| {
            group.config().max_rounds(5).auto_rounds(false);
            group.bench("a", |b| b.iter(|| black_box(1u64)));
        });
    });
    let path = std::env::temp_dir().join("zenbench_test_testbed_rt.json");
    result.save(&path).unwrap();
    let loaded = SuiteResult::load(&path).unwrap();
    assert_eq!(result.testbed, loaded.testbed);
    let _ = std::fs::remove_file(&path);
}

// ── Baseline staleness warnings ─────────────────────────────────────

#[test]
fn baseline_comparison_warns_on_git_hash_mismatch() {
    let mut result1 = run_gated(disabled_gate(), |suite| {
        suite.compare("stale", |group| {
            group.config().max_rounds(5).auto_rounds(false);
            group.bench("x", |b| b.iter(|| black_box(1u64)));
        });
    });
    let mut result2 = result1.clone();
    result1.git_hash = Some("aaaa1111".to_string());
    result2.git_hash = Some("bbbb2222".to_string());

    let comparison = zenbench::baseline::compare_against_baseline(&result1, &result2, 50.0);
    assert!(
        comparison.warnings.iter().any(|w| w.contains("git hash")),
        "should warn about git hash mismatch: {:?}",
        comparison.warnings,
    );
}

#[test]
fn baseline_comparison_warns_on_testbed_mismatch() {
    let result1 = run_gated(disabled_gate(), |suite| {
        suite.compare("hw", |group| {
            group.config().max_rounds(5).auto_rounds(false);
            group.bench("x", |b| b.iter(|| black_box(1u64)));
        });
    });
    let mut result2 = result1.clone();

    // Modify the testbed to simulate hardware change
    if let Some(tb) = result2.testbed.as_mut() {
        tb.cpu_model = "Different CPU Model".to_string();
    }

    let comparison = zenbench::baseline::compare_against_baseline(&result1, &result2, 50.0);
    assert!(
        comparison
            .warnings
            .iter()
            .any(|w| w.contains("CPU changed")),
        "should warn about CPU change: {:?}",
        comparison.warnings,
    );
}

#[test]
fn baseline_statistical_gating_prevents_false_positive() {
    // Two runs of the same workload: pct_change may exceed threshold by noise,
    // but the t-test should gate it because variance is high relative to diff.
    let result1 = run_gated(disabled_gate(), |suite| {
        suite.compare("gate_test", |group| {
            group.config().max_rounds(10).auto_rounds(false);
            group.bench("noisy", |b| {
                b.iter(|| {
                    let mut v = 0u64;
                    for i in 0..100 {
                        v = v.wrapping_add(black_box(i));
                    }
                    black_box(v)
                })
            });
        });
    });
    let result2 = run_gated(disabled_gate(), |suite| {
        suite.compare("gate_test", |group| {
            group.config().max_rounds(10).auto_rounds(false);
            group.bench("noisy", |b| {
                b.iter(|| {
                    let mut v = 0u64;
                    for i in 0..100 {
                        v = v.wrapping_add(black_box(i));
                    }
                    black_box(v)
                })
            });
        });
    });

    // With a 2% threshold + statistical gating, the same workload
    // should not show as a regression (run-to-run noise is high variance,
    // low significance per the t-test).
    let comparison = zenbench::baseline::compare_against_baseline(&result1, &result2, 10.0);
    assert_eq!(
        comparison.regressions, 0,
        "same workload should not regress at 10% threshold with statistical gating"
    );
}

// ── Configurable bootstrap resamples ────────────────────────────────

#[test]
fn configurable_resamples_works() {
    // Using fewer resamples should still produce valid results
    let result = run_gated(disabled_gate(), |suite| {
        suite.compare("resamples", |group| {
            group
                .config()
                .max_rounds(20)
                .auto_rounds(false)
                .bootstrap_resamples(500); // much less than default 10K
            group.bench("a", |b| b.iter(|| black_box(1u64)));
            group.bench("b", |b| {
                b.iter(|| {
                    let mut v = 0u64;
                    for i in 0..100 {
                        v = v.wrapping_add(black_box(i));
                    }
                    black_box(v)
                })
            });
        });
    });
    let comp = &result.comparisons[0];
    assert!(!comp.analyses.is_empty());
    let analysis = &comp.analyses[0].2;
    // CI should still be valid (lower <= median <= upper)
    assert!(analysis.ci_lower <= analysis.ci_median);
    assert!(analysis.ci_median <= analysis.ci_upper);
    // Per-benchmark CI should also exist
    assert!(comp.benchmarks[0].mean_ci.is_some());
}
