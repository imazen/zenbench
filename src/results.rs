use crate::stats::{PairedAnalysis, Summary};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;

/// Unique identifier for a benchmark run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunId(pub String);

impl RunId {
    pub fn generate() -> Self {
        // Timestamp-based ID with random suffix
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        // Use process ID for uniqueness within the same second
        let pid = std::process::id();
        RunId(format!("{}-{:x}", now.as_secs(), pid))
    }
}

impl std::fmt::Display for RunId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Result of a single benchmark (standalone or within a group).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkResult {
    pub name: String,
    pub summary: Summary,
}

/// Result of a comparison group (multiple interleaved benchmarks).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparisonResult {
    pub group_name: String,
    pub benchmarks: Vec<BenchmarkResult>,
    /// Paired analyses: (baseline_name, candidate_name, analysis).
    pub analyses: Vec<(String, String, PairedAnalysis)>,
    pub completed_rounds: usize,
}

/// Complete results of a benchmark suite run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuiteResult {
    pub run_id: RunId,
    pub timestamp: String,
    pub git_hash: Option<String>,
    pub ci_environment: Option<String>,
    pub comparisons: Vec<ComparisonResult>,
    pub standalones: Vec<BenchmarkResult>,
    #[serde(with = "duration_serde")]
    pub total_time: Duration,
    pub gate_waits: usize,
    #[serde(with = "duration_serde")]
    pub gate_wait_time: Duration,
    pub unreliable: bool,
}

impl SuiteResult {
    /// Save results to a JSON file.
    pub fn save(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(self)
            .map_err(std::io::Error::other)?;
        std::fs::write(path, json)
    }

    /// Load results from a JSON file.
    pub fn load(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let json = std::fs::read_to_string(path)?;
        serde_json::from_str(&json)
            .map_err(std::io::Error::other)
    }

    /// Print a human-readable report to stderr.
    pub fn print_report(&self) {
        eprintln!();
        eprintln!("═══════════════════════════════════════════════════════════════");
        eprintln!("  zenbench results  ({})", self.run_id);
        if let Some(hash) = &self.git_hash {
            eprintln!("  git: {}", hash);
        }
        if let Some(ci) = &self.ci_environment {
            eprintln!("  ci: {}", ci);
        }
        eprintln!("═══════════════════════════════════════════════════════════════");

        for comp in &self.comparisons {
            eprintln!();
            eprintln!(
                "  group: {}  ({} rounds)",
                comp.group_name, comp.completed_rounds
            );
            eprintln!("  ───────────────────────────────────────────────────────────");

            // Individual results
            for bench in &comp.benchmarks {
                eprintln!(
                    "    {:<30}  {:>10}  ±{:>10}",
                    bench.name,
                    format_ns(bench.summary.mean),
                    format_ns(bench.summary.std_dev()),
                );
            }

            // Paired comparisons
            if !comp.analyses.is_empty() {
                eprintln!();
            }
            for (base, cand, analysis) in &comp.analyses {
                let arrow = if analysis.pct_change < 0.0 {
                    "\x1b[32m" // green
                } else if analysis.pct_change > 0.0 {
                    "\x1b[31m" // red
                } else {
                    ""
                };
                let reset = if arrow.is_empty() { "" } else { "\x1b[0m" };
                let sig = if analysis.significant { "*" } else { " " };

                eprintln!(
                    "    {} vs {}:  {}{:+.2}%{}{}  (d={:.2}, CI [{}, {}])",
                    base,
                    cand,
                    arrow,
                    analysis.pct_change,
                    reset,
                    sig,
                    analysis.cohens_d,
                    format_ns(analysis.ci_lower),
                    format_ns(analysis.ci_upper),
                );

                if analysis.drift_correlation.abs() > 0.5 {
                    eprintln!(
                        "    \x1b[33m⚠ drift detected (r={:.2})\x1b[0m",
                        analysis.drift_correlation
                    );
                }
            }
        }

        // Standalone results
        for bench in &self.standalones {
            eprintln!();
            eprintln!(
                "  {:<30}  {:>10}  ±{:>10}  (n={})",
                bench.name,
                format_ns(bench.summary.mean),
                format_ns(bench.summary.std_dev()),
                bench.summary.n,
            );
        }

        eprintln!();
        eprintln!(
            "  total: {:?}  waits: {} ({:?})",
            self.total_time, self.gate_waits, self.gate_wait_time
        );
        if self.unreliable {
            eprintln!("  \x1b[31m⚠ UNRELIABLE: too many resource gate waits\x1b[0m");
        }
        eprintln!("═══════════════════════════════════════════════════════════════");
        eprintln!();
    }
}

/// Format nanoseconds as human-readable time.
fn format_ns(ns: f64) -> String {
    let abs = ns.abs();
    let sign = if ns < 0.0 { "-" } else { "" };
    if abs >= 1_000_000_000.0 {
        format!("{}{:.2}s", sign, abs / 1_000_000_000.0)
    } else if abs >= 1_000_000.0 {
        format!("{}{:.2}ms", sign, abs / 1_000_000.0)
    } else if abs >= 1_000.0 {
        format!("{}{:.2}µs", sign, abs / 1_000.0)
    } else if abs >= 0.01 {
        format!("{}{:.1}ns", sign, abs)
    } else {
        format!("{}{:.3}ns", sign, abs)
    }
}

/// Serde support for Duration via millis.
mod duration_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;

    #[derive(Serialize, Deserialize)]
    struct DurationMs {
        millis: u64,
    }

    pub fn serialize<S: Serializer>(dur: &Duration, s: S) -> Result<S::Ok, S::Error> {
        DurationMs {
            millis: dur.as_millis() as u64,
        }
        .serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
        let ms = DurationMs::deserialize(d)?;
        Ok(Duration::from_millis(ms.millis))
    }
}
