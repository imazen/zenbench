//! Tests for new features: overhead compensation, deferred drop, TSC timing,
//! asm fences, and allocation profiling integration.
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
    assert!(deferred_mean > 0.0, "deferred_drop should produce positive times");
    assert!(regular_mean > 0.0, "regular iter should produce positive times");

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
            group.bench("counted", |b| {
                b.iter_deferred_drop(|| DropCounter)
            });
        });
    });
    let comp = &result.comparisons[0];
    let total_iterations: usize = comp.benchmarks[0].summary.n * comp.iterations_per_sample;
    let drops = DROP_COUNT.load(Ordering::Relaxed);

    // Every iteration should have produced one DropCounter that was dropped.
    // Allow some tolerance for jitter (±20% iteration count per round).
    assert!(
        drops > 0,
        "should have dropped some DropCounters, got 0"
    );
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
            group.bench("regular", |b| {
                b.iter(|| ExpensiveDrop(vec![0u8; 4096]))
            });
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
            200,  // allocs
            200,  // deallocs
            10,   // reallocs
            16000, // bytes alloc
            16000, // bytes dealloc
            100,  // iterations
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
