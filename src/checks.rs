//! Common benchmarking warning types.
//!
//! These types represent issues that may be flagged during post-run analysis.
//! Currently unused — the footnote system in report.rs handles warnings inline.
//! Kept for future programmatic warning API.

#![allow(dead_code)]

/// Warnings produced by post-run analysis.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct BenchWarning {
    pub benchmark: String,
    pub kind: WarningKind,
    pub message: String,
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum WarningKind {
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
