//! Baseline persistence for CI regression testing.
//!
//! Save benchmark results as named baselines, then compare future runs
//! against them. Designed for CI pipelines where you need to gate merges
//! on performance regression detection.
//!
//! ```bash
//! # Save current results as the "main" baseline
//! cargo bench -- --save-baseline main
//!
//! # Compare against saved baseline (exit 1 if regressions)
//! cargo bench -- --baseline main
//! ```

use crate::results::SuiteResult;
use std::collections::HashMap;
use std::path::PathBuf;

/// Directory for baseline storage, relative to project root.
/// Uses PathBuf::join for cross-platform path separators.
fn baseline_dir() -> PathBuf {
    PathBuf::from(".zenbench").join("baselines")
}

/// Result of comparing a benchmark run against a saved baseline.
#[derive(Debug)]
pub struct BaselineComparison {
    /// Per-benchmark comparisons: (group, benchmark, analysis, regression?).
    pub benchmarks: Vec<BenchmarkDelta>,
    /// Number of benchmarks that regressed beyond the threshold.
    pub regressions: usize,
    /// Number of benchmarks that improved beyond the threshold.
    pub improvements: usize,
    /// Number of benchmarks within the noise threshold.
    pub unchanged: usize,
    /// Benchmarks in the new run but not in the baseline.
    pub new_benchmarks: Vec<String>,
    /// Benchmarks in the baseline but not in the new run.
    pub missing_benchmarks: Vec<String>,
    /// Warnings about the comparison (staleness, hardware mismatch, etc.).
    pub warnings: Vec<String>,
}

/// Comparison result for a single benchmark.
#[derive(Debug)]
pub struct BenchmarkDelta {
    pub group: String,
    pub name: String,
    /// Baseline mean (ns).
    pub baseline_mean: f64,
    /// New mean (ns).
    pub new_mean: f64,
    /// Percentage change (positive = slower).
    pub pct_change: f64,
    /// Whether this is a regression (exceeds threshold).
    pub regressed: bool,
    /// Whether this is an improvement (exceeds threshold in the good direction).
    pub improved: bool,
}

/// Save a `SuiteResult` as a named baseline.
pub fn save_baseline(result: &SuiteResult, name: &str) -> std::io::Result<PathBuf> {
    let dir = baseline_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{name}.json"));
    result.save(&path)?;
    Ok(path)
}

/// Load a named baseline.
pub fn load_baseline(name: &str) -> Result<SuiteResult, String> {
    let path = baseline_dir().join(format!("{name}.json"));
    if !path.exists() {
        return Err(format!(
            "baseline '{}' not found at {}",
            name,
            path.display()
        ));
    }
    SuiteResult::load(&path).map_err(|e| format!("failed to load baseline '{}': {}", name, e))
}

/// List all saved baselines.
pub fn list_baselines() -> Vec<String> {
    let dir = baseline_dir();
    let mut names = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            if let Some(name) = entry
                .path()
                .file_stem()
                .and_then(|s| s.to_str())
                .map(String::from)
                .filter(|_| entry.path().extension().is_some_and(|e| e == "json"))
            {
                names.push(name);
            }
        }
    }
    names.sort();
    names
}

/// Delete baselines older than `max_age_secs`. Returns number deleted.
pub fn prune_baselines(max_age_secs: u64) -> std::io::Result<usize> {
    let dir = baseline_dir();
    let mut deleted = 0;
    let now = std::time::SystemTime::now();
    let entries = std::fs::read_dir(&dir).into_iter().flatten().flatten();
    for entry in entries {
        let path = entry.path();
        let is_old = path.extension().is_some_and(|e| e == "json")
            && std::fs::metadata(&path)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| now.duration_since(t).ok())
                .is_some_and(|age| age.as_secs() > max_age_secs);
        if is_old && std::fs::remove_file(&path).is_ok() {
            deleted += 1;
        }
    }
    Ok(deleted)
}

/// Delete a named baseline.
pub fn delete_baseline(name: &str) -> std::io::Result<()> {
    let path = baseline_dir().join(format!("{name}.json"));
    std::fs::remove_file(path)
}

/// Compare a new run against a saved baseline.
///
/// `max_regression_pct`: maximum allowed regression as a percentage (e.g., 5.0 = 5%).
/// Benchmarks that regress by more than this are flagged.
///
/// Returns the comparison result. The caller decides whether to fail CI based on
/// `comparison.regressions > 0`.
pub fn compare_against_baseline(
    baseline: &SuiteResult,
    current: &SuiteResult,
    max_regression_pct: f64,
) -> BaselineComparison {
    let mut warnings = Vec::new();

    // Staleness checks
    if let (Some(base_hash), Some(curr_hash)) = (&baseline.git_hash, &current.git_hash) {
        if base_hash != curr_hash {
            warnings.push(format!(
                "git hash differs: baseline={} current={}",
                &base_hash[..base_hash.len().min(8)],
                &curr_hash[..curr_hash.len().min(8)],
            ));
        }
    }

    if let (Some(base_tb), Some(curr_tb)) = (&baseline.testbed, &current.testbed) {
        if base_tb.cpu_model != curr_tb.cpu_model {
            warnings.push(format!(
                "CPU changed: baseline='{}' current='{}'",
                base_tb.cpu_model, curr_tb.cpu_model,
            ));
        }
        if base_tb.arch != curr_tb.arch || base_tb.os != curr_tb.os {
            warnings.push(format!(
                "platform changed: baseline={}/{} current={}/{}",
                base_tb.arch, base_tb.os, curr_tb.arch, curr_tb.os,
            ));
        }
    }

    // Per-benchmark data: (mean, variance, n)
    struct BenchData {
        mean: f64,
        variance: f64,
        n: usize,
    }

    let mut baseline_map: HashMap<(String, String), BenchData> = HashMap::new();
    for comp in &baseline.comparisons {
        for bench in &comp.benchmarks {
            baseline_map.insert(
                (comp.group_name.clone(), bench.name.clone()),
                BenchData {
                    mean: bench.summary.mean,
                    variance: bench.summary.variance,
                    n: bench.summary.n,
                },
            );
        }
    }

    let mut current_map: HashMap<(String, String), BenchData> = HashMap::new();
    for comp in &current.comparisons {
        for bench in &comp.benchmarks {
            current_map.insert(
                (comp.group_name.clone(), bench.name.clone()),
                BenchData {
                    mean: bench.summary.mean,
                    variance: bench.summary.variance,
                    n: bench.summary.n,
                },
            );
        }
    }

    let mut benchmarks = Vec::new();
    let mut regressions = 0;
    let mut improvements = 0;
    let mut unchanged = 0;
    let mut new_benchmarks = Vec::new();
    let mut missing_benchmarks = Vec::new();

    for (key, base) in &baseline_map {
        if let Some(curr) = current_map.get(key) {
            let pct_change = if base.mean.abs() > f64::EPSILON {
                (curr.mean - base.mean) / base.mean * 100.0
            } else {
                0.0
            };

            // Statistical gating: require BOTH percentage threshold AND
            // statistical significance (pooled t-test, p < 0.05 ≈ t > 2.0).
            // This prevents noisy CI runners from triggering false regressions.
            let statistically_significant = if base.n >= 2 && curr.n >= 2 {
                let se_sq = base.variance / base.n as f64 + curr.variance / curr.n as f64;
                if se_sq > 0.0 {
                    let t = (curr.mean - base.mean).abs() / se_sq.sqrt();
                    t > 2.0 // approximate p < 0.05
                } else {
                    true // zero variance = deterministic = always significant
                }
            } else {
                true // not enough data to gate — trust the percentage
            };

            let regressed = pct_change > max_regression_pct && statistically_significant;
            let improved = pct_change < -max_regression_pct && statistically_significant;

            if regressed {
                regressions += 1;
            } else if improved {
                improvements += 1;
            } else {
                unchanged += 1;
            }

            benchmarks.push(BenchmarkDelta {
                group: key.0.clone(),
                name: key.1.clone(),
                baseline_mean: base.mean,
                new_mean: curr.mean,
                pct_change,
                regressed,
                improved,
            });
        } else {
            missing_benchmarks.push(format!("{}::{}", key.0, key.1));
        }
    }

    for key in current_map.keys() {
        if !baseline_map.contains_key(key) {
            new_benchmarks.push(format!("{}::{}", key.0, key.1));
        }
    }

    benchmarks.sort_by(|a, b| {
        b.pct_change
            .partial_cmp(&a.pct_change)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    BaselineComparison {
        benchmarks,
        regressions,
        improvements,
        unchanged,
        new_benchmarks,
        missing_benchmarks,
        warnings,
    }
}

/// Print a baseline comparison report to stderr.
pub fn print_comparison_report(comparison: &BaselineComparison) {
    eprintln!();
    eprintln!("  Baseline comparison");
    eprintln!("  ───────────────────");

    for w in &comparison.warnings {
        eprintln!("  \x1b[33m⚠ {w}\x1b[0m");
    }

    if comparison.benchmarks.is_empty() {
        eprintln!("  No matching benchmarks found.");
        return;
    }

    for delta in &comparison.benchmarks {
        let marker = if delta.regressed {
            "\x1b[31m▲ REGRESSION\x1b[0m"
        } else if delta.improved {
            "\x1b[32m▼ improved\x1b[0m"
        } else {
            "  unchanged"
        };

        eprintln!(
            "  {:<30} {:>9.1}ns → {:>9.1}ns  {:>+7.2}%  {}",
            format!("{}::{}", delta.group, delta.name),
            delta.baseline_mean,
            delta.new_mean,
            delta.pct_change,
            marker,
        );
    }

    eprintln!();
    if !comparison.new_benchmarks.is_empty() {
        eprintln!(
            "  New (not in baseline): {}",
            comparison.new_benchmarks.join(", ")
        );
    }
    if !comparison.missing_benchmarks.is_empty() {
        eprintln!(
            "  Missing (in baseline, not in run): {}",
            comparison.missing_benchmarks.join(", ")
        );
    }

    eprintln!(
        "  Summary: {} regressions, {} improvements, {} unchanged",
        comparison.regressions, comparison.improvements, comparison.unchanged,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_result(benchmarks: &[(&str, &str, f64)]) -> SuiteResult {
        use crate::results::*;
        use crate::stats::Summary;

        let mut comparisons = Vec::new();
        let mut groups: HashMap<String, Vec<BenchmarkResult>> = HashMap::new();

        for &(group, name, mean) in benchmarks {
            groups
                .entry(group.to_string())
                .or_default()
                .push(BenchmarkResult {
                    name: name.to_string(),
                    summary: Summary::from_slice(&[mean]),
                    ..Default::default()
                });
        }

        for (group_name, benches) in groups {
            comparisons.push(ComparisonResult {
                group_name,
                benchmarks: benches,
                completed_rounds: 100,
                iterations_per_sample: 1000,
                ..Default::default()
            });
        }

        SuiteResult {
            run_id: RunId("test".to_string()),
            comparisons,
            total_time: std::time::Duration::from_secs(1),
            timer_resolution_ns: 10,
            loop_overhead_ns: 0.5,
            ..Default::default()
        }
    }

    #[test]
    fn detect_regression() {
        let baseline = make_result(&[("sort", "std_sort", 100.0), ("sort", "unstable", 80.0)]);
        let current = make_result(&[("sort", "std_sort", 112.0), ("sort", "unstable", 78.0)]);

        let comparison = compare_against_baseline(&baseline, &current, 5.0);
        assert_eq!(comparison.regressions, 1); // std_sort: +12% exceeds 5%
        assert_eq!(comparison.improvements, 0); // unstable: -2.5%, within ±5%
        assert_eq!(comparison.unchanged, 1); // unstable: within threshold
    }

    #[test]
    fn detect_improvement() {
        let baseline = make_result(&[("g", "fast", 100.0)]);
        let current = make_result(&[("g", "fast", 85.0)]);

        let comparison = compare_against_baseline(&baseline, &current, 5.0);
        assert_eq!(comparison.improvements, 1); // -15%
        assert_eq!(comparison.regressions, 0);
    }

    #[test]
    fn detect_new_and_missing() {
        let baseline = make_result(&[("g", "old_bench", 100.0)]);
        let current = make_result(&[("g", "new_bench", 100.0)]);

        let comparison = compare_against_baseline(&baseline, &current, 5.0);
        assert_eq!(comparison.missing_benchmarks.len(), 1);
        assert_eq!(comparison.new_benchmarks.len(), 1);
    }

    #[test]
    fn save_and_load_roundtrip() {
        let result = make_result(&[("g", "bench_a", 42.0)]);
        let _ = std::fs::create_dir_all(baseline_dir());
        let path = save_baseline(&result, "test_roundtrip").unwrap();
        let loaded = load_baseline("test_roundtrip").unwrap();
        assert_eq!(loaded.comparisons[0].benchmarks[0].summary.mean, 42.0);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn list_baselines_works() {
        let _ = std::fs::create_dir_all(baseline_dir());
        // Just verify it doesn't panic
        let _ = list_baselines();
    }
}
