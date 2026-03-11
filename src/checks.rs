//! Common benchmarking error detection.
//!
//! These checks help users avoid the most frequent microbenchmarking mistakes.

use crate::stats::Summary;

/// Warnings produced by post-run analysis.
#[derive(Debug, Clone)]
pub struct BenchWarning {
    pub benchmark: String,
    pub kind: WarningKind,
    pub message: String,
}

#[derive(Debug, Clone)]
pub enum WarningKind {
    /// Measurement is near or below timer resolution.
    BelowTimerResolution,
    /// Very high coefficient of variation suggests noisy benchmark.
    HighVariance,
    /// All measurements identical — likely optimized away.
    ZeroVariance,
    /// Measurement suspiciously fast (< 1ns) — likely dead code elimination.
    LikelyOptimizedAway,
    /// Systematic drift detected across rounds.
    Drift,
    /// Too few rounds for reliable statistics.
    TooFewRounds,
    /// Multiple comparisons inflate false-positive risk.
    MultipleComparisons,
}

impl std::fmt::Display for BenchWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.benchmark, self.message)
    }
}

/// Check a benchmark result for common issues.
pub fn check_benchmark(name: &str, summary: &Summary, n_rounds: usize) -> Vec<BenchWarning> {
    let mut warnings = Vec::new();

    // Check for sub-nanosecond measurements (likely optimized away)
    if summary.mean < 1.0 && summary.n > 0 {
        warnings.push(BenchWarning {
            benchmark: name.to_string(),
            kind: WarningKind::LikelyOptimizedAway,
            message: format!(
                "mean time {:.3}ns is below 1ns — the benchmark is likely optimized away. \
                 Use zenbench::black_box() on inputs and outputs.",
                summary.mean
            ),
        });
    }

    // Check for near-zero variance (all identical measurements)
    if summary.variance < f64::EPSILON && summary.n > 5 {
        warnings.push(BenchWarning {
            benchmark: name.to_string(),
            kind: WarningKind::ZeroVariance,
            message: "all measurements are identical — this may indicate the benchmark \
                     is being const-folded or the function is not actually being called."
                .to_string(),
        });
    }

    // Check timer resolution (~15-100ns on most systems)
    if summary.mean > 0.0 && summary.mean < 50.0 && summary.n > 0 {
        warnings.push(BenchWarning {
            benchmark: name.to_string(),
            kind: WarningKind::BelowTimerResolution,
            message: format!(
                "mean time {:.1}ns is near typical timer resolution (15-100ns). \
                 Results may be dominated by timer overhead. Consider increasing iterations.",
                summary.mean
            ),
        });
    }

    // Check coefficient of variation
    if summary.cv() > 0.20 && summary.n > 10 {
        warnings.push(BenchWarning {
            benchmark: name.to_string(),
            kind: WarningKind::HighVariance,
            message: format!(
                "coefficient of variation is {:.0}% (>20%). Results are noisy. \
                 Try running on a quieter system or increasing iteration count.",
                summary.cv() * 100.0
            ),
        });
    }

    // Check for too few rounds
    if n_rounds < 10 {
        warnings.push(BenchWarning {
            benchmark: name.to_string(),
            kind: WarningKind::TooFewRounds,
            message: format!(
                "only {} rounds completed — need at least 30 for reliable statistics. \
                 Increase rounds or max_time.",
                n_rounds
            ),
        });
    }

    warnings
}

/// Check for multiple-comparison inflation.
///
/// With k benchmarks in a group, there are k*(k-1)/2 pairwise comparisons.
/// At 99% confidence per test, the family-wise error rate is roughly
/// 1 - (1 - 0.01)^n_comparisons. Warn when this exceeds 10%.
pub fn check_multiple_comparisons(group_name: &str, n_benchmarks: usize) -> Option<BenchWarning> {
    if n_benchmarks <= 2 {
        return None;
    }
    let n_comparisons = n_benchmarks * (n_benchmarks - 1) / 2;
    // Family-wise error rate at per-test alpha = 0.01
    let fwer = 1.0 - (1.0 - 0.01f64).powi(n_comparisons as i32);
    if fwer > 0.10 {
        Some(BenchWarning {
            benchmark: group_name.to_string(),
            kind: WarningKind::MultipleComparisons,
            message: format!(
                "{n_comparisons} pairwise comparisons — family-wise error rate is ~{:.0}%. \
                 Consider Bonferroni correction (alpha/{n_comparisons} = {:.4}) \
                 or split into smaller groups.",
                fwer * 100.0,
                0.01 / n_comparisons as f64,
            ),
        })
    } else {
        None
    }
}

/// Check paired analysis for drift.
pub fn check_drift(
    base_name: &str,
    cand_name: &str,
    drift_correlation: f64,
) -> Option<BenchWarning> {
    if drift_correlation.abs() > 0.5 {
        Some(BenchWarning {
            benchmark: format!("{base_name} vs {cand_name}"),
            kind: WarningKind::Drift,
            message: format!(
                "systematic drift detected (Spearman r={:.2}). \
                 This suggests thermal throttling, load changes, or other \
                 time-dependent effects are corrupting results. {}",
                drift_correlation,
                if drift_correlation > 0.0 {
                    "Later rounds are slower — possible thermal throttling."
                } else {
                    "Later rounds are faster — possible warmup effect."
                }
            ),
        })
    } else {
        None
    }
}
