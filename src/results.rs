use crate::bench::Throughput;
use crate::format::csv_escape;
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
    /// CPU time summary (user time). Present when `cpu-time` feature is enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_summary: Option<Summary>,
    /// Key-value tags for multi-dimensional reporting.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<(String, String)>,
    /// Visual subgroup label (display-only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subgroup: Option<String>,
    /// Cold-start time in nanoseconds (first single-iteration call during warmup).
    /// This is the coldest measurement we can capture without process isolation.
    #[serde(default)]
    pub cold_start_ns: f64,
}

impl BenchmarkResult {
    /// Get a tag value by key.
    pub fn tag(&self, key: &str) -> Option<&str> {
        self.tags
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

/// Result of a comparison group (multiple interleaved benchmarks).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparisonResult {
    pub group_name: String,
    pub benchmarks: Vec<BenchmarkResult>,
    /// Paired analyses: (baseline_name, candidate_name, analysis).
    pub analyses: Vec<(String, String, PairedAnalysis)>,
    pub completed_rounds: usize,
    /// Throughput declaration for this group (if set).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub throughput: Option<Throughput>,
    /// Whether cache firewall was enabled for this group.
    #[serde(default)]
    pub cache_firewall: bool,
    /// Cache firewall size in bytes (when enabled).
    #[serde(default)]
    pub cache_firewall_bytes: usize,
    /// Whether only baseline comparisons are shown in reports.
    #[serde(default)]
    pub baseline_only: bool,
    /// Custom unit name for Elements throughput (e.g., "checks" -> "Gchecks/s").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub throughput_unit: Option<String>,
    /// Whether benchmarks are sorted by speed in report output.
    #[serde(default)]
    pub sort_by_speed: bool,
    /// Whether sub-ns warnings are suppressed.
    #[serde(default)]
    pub expect_sub_ns: bool,
    /// Base iterations per sample (before jitter). 0 if unknown.
    #[serde(default)]
    pub iterations_per_sample: usize,
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
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, json)
    }

    /// Load results from a JSON file.
    pub fn load(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let json = std::fs::read_to_string(path)?;
        serde_json::from_str(&json).map_err(std::io::Error::other)
    }

    /// Print a human-readable report to stderr (with ANSI colors).
    pub fn print_report(&self) {
        crate::report::print_report(self);
    }

    /// Generate key-value format optimized for LLM consumption and grep.
    ///
    /// One line per benchmark. Every field explicitly named. No positional
    /// parsing, no ANSI, no box drawing. Greppable:
    /// ```text
    /// cargo bench -- --format=llm | grep 'benchmark="my_func"'
    /// ```
    pub fn to_llm(&self) -> String {
        let mut out = String::new();

        for comp in &self.comparisons {
            let baseline_name = comp
                .analyses
                .first()
                .map(|(base, _, _)| base.as_str())
                .unwrap_or_else(|| {
                    comp.benchmarks
                        .first()
                        .map(|b| b.name.as_str())
                        .unwrap_or("")
                });

            // Build analysis lookup
            let analyses: std::collections::HashMap<&str, &PairedAnalysis> = comp
                .analyses
                .iter()
                .filter(|(base, _, _)| base == baseline_name)
                .map(|(_, cand, a)| (cand.as_str(), a))
                .collect();

            for bench in &comp.benchmarks {
                let s = &bench.summary;
                let mut kv = vec![format!("group={}", llm_quote(&comp.group_name))];
                if let Some(sg) = &bench.subgroup {
                    kv.push(format!("subgroup={}", llm_quote(sg)));
                }
                kv.push(format!("benchmark={}", llm_quote(&bench.name)));
                kv.push(format!("median_ns={:.2}", s.median));
                kv.push(format!("mean_ns={:.2}", s.mean));
                kv.push(format!("min_ns={:.2}", s.min));
                kv.push(format!("max_ns={:.2}", s.max));
                kv.push(format!("stddev_ns={:.2}", s.std_dev()));
                kv.push(format!("mad_ns={:.2}", s.mad));
                kv.push(format!("n={}", s.n));
                kv.push(format!("cv_pct={:.1}", s.cv() * 100.0));

                if bench.cold_start_ns > 0.0 {
                    kv.push(format!("cold_start_ns={:.2}", bench.cold_start_ns));
                }

                // Comparison data
                if bench.name == baseline_name {
                    kv.push("vs_base=baseline".to_string());
                } else if let Some(analysis) = analyses.get(bench.name.as_str()) {
                    let base_mean = analysis.baseline.mean;
                    kv.push(format!("vs_base_pct={:+.2}", analysis.pct_change));
                    kv.push(format!("vs_base_ci_lo_ns={:.2}", analysis.ci_lower));
                    kv.push(format!("vs_base_ci_median_ns={:.2}", analysis.ci_median));
                    kv.push(format!("vs_base_ci_hi_ns={:.2}", analysis.ci_upper));
                    if base_mean.abs() > f64::EPSILON {
                        kv.push(format!(
                            "vs_base_ci_lo_pct={:+.2}",
                            analysis.ci_lower / base_mean * 100.0,
                        ));
                        kv.push(format!(
                            "vs_base_ci_hi_pct={:+.2}",
                            analysis.ci_upper / base_mean * 100.0,
                        ));
                    }
                    kv.push(format!("significant={}", analysis.significant));
                    kv.push(format!("cohens_d={:.2}", analysis.cohens_d));
                    kv.push(format!("wilcoxon_p={:.6}", analysis.wilcoxon_p));
                    kv.push(format!("drift_r={:.2}", analysis.drift_correlation));
                }

                // Throughput
                if let Some(tp) = &comp.throughput {
                    let (val, unit) = tp.compute_named(s.mean, comp.throughput_unit.as_deref());
                    kv.push(format!("throughput={:.2}", val));
                    kv.push(format!("throughput_unit={unit}"));
                }

                // Tags
                for (k, v) in &bench.tags {
                    kv.push(format!("tag_{k}={}", llm_quote(v)));
                }

                // Config context
                kv.push(format!("rounds={}", comp.completed_rounds));
                kv.push(format!("iters_per_sample={}", comp.iterations_per_sample));

                out.push_str(&kv.join(" "));
                out.push('\n');
            }
        }

        // Standalone benchmarks
        for bench in &self.standalones {
            let s = &bench.summary;
            let mut kv = vec![
                format!("benchmark={}", llm_quote(&bench.name)),
                format!("median_ns={:.2}", s.median),
                format!("mean_ns={:.2}", s.mean),
                format!("min_ns={:.2}", s.min),
                format!("max_ns={:.2}", s.max),
                format!("stddev_ns={:.2}", s.std_dev()),
                format!("mad_ns={:.2}", s.mad),
                format!("n={}", s.n),
                format!("cv_pct={:.1}", s.cv() * 100.0),
            ];
            if bench.cold_start_ns > 0.0 {
                kv.push(format!("cold_start_ns={:.2}", bench.cold_start_ns));
            }
            out.push_str(&kv.join(" "));
            out.push('\n');
        }

        out
    }

    /// Generate a markdown report with tables.
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();

        // Header
        out.push_str("# Benchmark Results\n\n");
        if let Some(hash) = &self.git_hash {
            out.push_str(&format!("**git:** `{hash}`  \n"));
        }
        out.push_str(&format!(
            "**total:** {:.1}s  **waits:** {} ({:.1}s)\n\n",
            self.total_time.as_secs_f64(),
            self.gate_waits,
            self.gate_wait_time.as_secs_f64()
        ));

        // Comparison groups
        for comp in &self.comparisons {
            out.push_str(&format!(
                "## {} ({} rounds)\n\n",
                comp.group_name, comp.completed_rounds
            ));

            // Build table columns
            let has_throughput = comp.throughput.is_some();
            let has_cpu = comp.benchmarks.iter().any(|b| b.cpu_summary.is_some());
            if has_throughput && has_cpu {
                out.push_str("| Benchmark | Mean | CPU | Eff% | Throughput |\n");
                out.push_str("|-----------|------|-----|------|------------|\n");
            } else if has_throughput {
                out.push_str("| Benchmark | Mean | Median | Throughput |\n");
                out.push_str("|-----------|------|--------|------------|\n");
            } else if has_cpu {
                out.push_str("| Benchmark | Mean | CPU | Eff% | StdDev |\n");
                out.push_str("|-----------|------|-----|------|--------|\n");
            } else {
                out.push_str("| Benchmark | Mean | Median | StdDev |\n");
                out.push_str("|-----------|------|--------|--------|\n");
            }

            for bench in &comp.benchmarks {
                let cpu_mean = bench
                    .cpu_summary
                    .as_ref()
                    .map(|c| format_ns(c.mean))
                    .unwrap_or_default();
                let efficiency = bench
                    .cpu_summary
                    .as_ref()
                    .map(|c| {
                        if bench.summary.mean > 0.0 {
                            format!("{:.0}%", c.mean / bench.summary.mean * 100.0)
                        } else {
                            String::new()
                        }
                    })
                    .unwrap_or_default();

                if has_throughput && has_cpu {
                    let tp = comp
                        .throughput
                        .as_ref()
                        .map(|t| {
                            t.format_named(bench.summary.mean, comp.throughput_unit.as_deref())
                        })
                        .unwrap_or_default();
                    out.push_str(&format!(
                        "| {} | {} | {} | {} | {} |\n",
                        bench.name,
                        format_ns(bench.summary.mean),
                        cpu_mean,
                        efficiency,
                        tp,
                    ));
                } else if has_throughput {
                    let tp = comp
                        .throughput
                        .as_ref()
                        .map(|t| {
                            t.format_named(bench.summary.mean, comp.throughput_unit.as_deref())
                        })
                        .unwrap_or_default();
                    out.push_str(&format!(
                        "| {} | {} | {} | {} |\n",
                        bench.name,
                        format_ns(bench.summary.mean),
                        format_ns(bench.summary.median),
                        tp,
                    ));
                } else if has_cpu {
                    out.push_str(&format!(
                        "| {} | {} | {} | {} | \u{b1}{} |\n",
                        bench.name,
                        format_ns(bench.summary.mean),
                        cpu_mean,
                        efficiency,
                        format_ns(bench.summary.std_dev()),
                    ));
                } else {
                    out.push_str(&format!(
                        "| {} | {} | {} | \u{b1}{} |\n",
                        bench.name,
                        format_ns(bench.summary.mean),
                        format_ns(bench.summary.median),
                        format_ns(bench.summary.std_dev()),
                    ));
                }
            }

            // Paired comparisons
            if !comp.analyses.is_empty() {
                out.push('\n');
                for (base, cand, analysis) in &comp.analyses {
                    let sig = if analysis.significant { " **" } else { "" };
                    out.push_str(&format!(
                        "- **{base}** vs **{cand}**: {:+.2}%{sig} (d={:.2}, p={:.4})\n",
                        analysis.pct_change, analysis.cohens_d, analysis.wilcoxon_p,
                    ));
                }
            }

            // Bar chart
            if !comp.benchmarks.is_empty() {
                out.push('\n');
                out.push_str(&crate::report::format_bar_chart(
                    &comp.benchmarks,
                    comp.throughput.as_ref(),
                    comp.throughput_unit.as_deref(),
                ));
            }

            out.push('\n');
        }

        // Standalone benchmarks
        if !self.standalones.is_empty() {
            out.push_str("## Standalone\n\n");
            out.push_str("| Benchmark | Mean | Median | StdDev |\n");
            out.push_str("|-----------|------|--------|--------|\n");
            for bench in &self.standalones {
                out.push_str(&format!(
                    "| {} | {} | {} | \u{b1}{} |\n",
                    bench.name,
                    format_ns(bench.summary.mean),
                    format_ns(bench.summary.median),
                    format_ns(bench.summary.std_dev()),
                ));
            }
        }

        out
    }

    /// Generate CSV output with one row per benchmark.
    pub fn to_csv(&self) -> String {
        let mut out = String::new();

        // Header
        out.push_str(
            "group,benchmark,mean_ns,std_dev_ns,median_ns,mad_ns,min_ns,max_ns,n,cv,cpu_mean_ns,cpu_efficiency,throughput_value,throughput_unit\n",
        );

        // Comparison groups
        for comp in &self.comparisons {
            for bench in &comp.benchmarks {
                let (tp_val, tp_unit) = comp
                    .throughput
                    .as_ref()
                    .map(|t| {
                        let (v, u) =
                            t.compute_named(bench.summary.mean, comp.throughput_unit.as_deref());
                        (format!("{v:.4}"), u)
                    })
                    .unwrap_or_else(|| (String::new(), String::new()));

                let (cpu_mean, cpu_eff) = bench
                    .cpu_summary
                    .as_ref()
                    .map(|c| {
                        let eff = if bench.summary.mean > 0.0 {
                            c.mean / bench.summary.mean
                        } else {
                            0.0
                        };
                        (format!("{:.2}", c.mean), format!("{eff:.4}"))
                    })
                    .unwrap_or_else(|| (String::new(), String::new()));

                out.push_str(&format!(
                    "{},{},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{},{:.4},{},{},{},{}\n",
                    csv_escape(&comp.group_name),
                    csv_escape(&bench.name),
                    bench.summary.mean,
                    bench.summary.std_dev(),
                    bench.summary.median,
                    bench.summary.mad,
                    bench.summary.min,
                    bench.summary.max,
                    bench.summary.n,
                    bench.summary.cv(),
                    cpu_mean,
                    cpu_eff,
                    tp_val,
                    tp_unit,
                ));
            }
        }

        // Standalone benchmarks
        for bench in &self.standalones {
            let (cpu_mean, cpu_eff) = bench
                .cpu_summary
                .as_ref()
                .map(|c| {
                    let eff = if bench.summary.mean > 0.0 {
                        c.mean / bench.summary.mean
                    } else {
                        0.0
                    };
                    (format!("{:.2}", c.mean), format!("{eff:.4}"))
                })
                .unwrap_or_else(|| (String::new(), String::new()));

            out.push_str(&format!(
                ",{},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{},{:.4},{},{},,\n",
                csv_escape(&bench.name),
                bench.summary.mean,
                bench.summary.std_dev(),
                bench.summary.median,
                bench.summary.mad,
                bench.summary.min,
                bench.summary.max,
                bench.summary.n,
                bench.summary.cv(),
                cpu_mean,
                cpu_eff,
            ));
        }

        out
    }

    /// Group benchmarks across all comparison results by a tag key.
    ///
    /// Returns a map from tag value to list of (group_name, benchmark_result) pairs.
    pub fn group_by_tag(
        &self,
        tag_key: &str,
    ) -> std::collections::BTreeMap<String, Vec<(&str, &BenchmarkResult)>> {
        let mut groups = std::collections::BTreeMap::new();
        for comp in &self.comparisons {
            for bench in &comp.benchmarks {
                if let Some(val) = bench.tag(tag_key) {
                    groups
                        .entry(val.to_string())
                        .or_insert_with(Vec::new)
                        .push((comp.group_name.as_str(), bench));
                }
            }
        }
        groups
    }
}

// Re-export format_ns so that `results::format_ns` still resolves.
pub use crate::format::format_ns;

/// Quote a value for LLM key-value format. Wraps in double quotes if it
/// contains spaces, quotes, or equals signs.
fn llm_quote(s: &str) -> String {
    if s.contains(' ') || s.contains('"') || s.contains('=') {
        format!("\"{}\"", s.replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::csv_escape;
    use crate::stats::Summary;
    use std::time::Duration;

    fn make_summary(mean_ns: f64) -> Summary {
        // Minimal summary for testing
        Summary::from_slice(&[mean_ns])
    }

    #[test]
    fn throughput_bytes_mibs() {
        let tp = Throughput::Bytes(1_048_576); // 1 MiB
        // 1 MiB in 1ms = 1000 MiB/s -> should show GiB/s
        let (val, unit) = tp.compute(1_000_000.0); // 1ms in ns
        assert_eq!(unit, "MiB/s");
        assert!((val - 1000.0).abs() < 0.1);
    }

    #[test]
    fn throughput_bytes_gibs() {
        let tp = Throughput::Bytes(1_073_741_824); // 1 GiB
        // 1 GiB in 1ms = 1000 GiB/s
        let (val, unit) = tp.compute(1_000_000.0); // 1ms in ns
        assert_eq!(unit, "GiB/s");
        assert!((val - 1000.0).abs() < 0.1);
    }

    #[test]
    fn throughput_elements() {
        let tp = Throughput::Elements(1000);
        // 1000 elements in 1ms = 1M ops/s
        let (val, unit) = tp.compute(1_000_000.0);
        assert_eq!(unit, "Mops/s");
        assert!((val - 1.0).abs() < 0.001);
    }

    #[test]
    fn throughput_format() {
        let tp = Throughput::Bytes(153 * 1024 * 1024); // 153 MiB
        // 153 MiB in 531ms = 288 MiB/s
        let s = tp.format(531_000_000.0);
        assert!(s.contains("MiB/s"), "Expected MiB/s, got: {s}");
        assert!(s.contains("288"), "Expected ~288, got: {s}");
    }

    #[test]
    fn benchmark_result_tags() {
        let br = BenchmarkResult {
            name: "test".to_string(),
            summary: make_summary(100.0),
            cpu_summary: None,
            tags: vec![
                ("library".to_string(), "zenflate".to_string()),
                ("level".to_string(), "L6".to_string()),
            ],
            subgroup: None,
            cold_start_ns: 0.0,
        };
        assert_eq!(br.tag("library"), Some("zenflate"));
        assert_eq!(br.tag("level"), Some("L6"));
        assert_eq!(br.tag("missing"), None);
    }

    fn make_suite_result() -> SuiteResult {
        SuiteResult {
            run_id: RunId("test-123".to_string()),
            timestamp: "2026-03-11T00:00:00Z".to_string(),
            git_hash: Some("abc123".to_string()),
            ci_environment: None,
            comparisons: vec![ComparisonResult {
                group_name: "compress".to_string(),
                benchmarks: vec![
                    BenchmarkResult {
                        name: "zenflate".to_string(),
                        summary: make_summary(5_000_000.0), // 5ms
                        cpu_summary: None,
                        tags: vec![("library".to_string(), "zenflate".to_string())],
                        subgroup: None,
                        cold_start_ns: 0.0,
                    },
                    BenchmarkResult {
                        name: "libdeflate".to_string(),
                        summary: make_summary(10_000_000.0), // 10ms
                        cpu_summary: None,
                        tags: vec![("library".to_string(), "libdeflate".to_string())],
                        subgroup: None,
                        cold_start_ns: 0.0,
                    },
                ],
                analyses: vec![],
                completed_rounds: 100,
                throughput: Some(Throughput::Bytes(1_048_576)), // 1 MiB
                cache_firewall: false,
                cache_firewall_bytes: 0,
                baseline_only: false,
                throughput_unit: None,
                sort_by_speed: false,
                expect_sub_ns: false,
                iterations_per_sample: 1000,
            }],
            standalones: vec![],
            total_time: Duration::from_secs(5),
            gate_waits: 0,
            gate_wait_time: Duration::ZERO,
            unreliable: false,
        }
    }

    #[test]
    fn markdown_output_contains_table() {
        let result = make_suite_result();
        let md = result.to_markdown();
        assert!(md.contains("| Benchmark |"), "Missing table header");
        assert!(md.contains("zenflate"), "Missing benchmark name");
        assert!(md.contains("MiB/s"), "Missing throughput");
    }

    #[test]
    fn markdown_output_contains_bar_chart() {
        let result = make_suite_result();
        let md = result.to_markdown();
        assert!(md.contains("```"), "Missing code block");
        assert!(md.contains('\u{2588}'), "Missing bar characters");
    }

    #[test]
    fn csv_output_has_header_and_rows() {
        let result = make_suite_result();
        let csv = result.to_csv();
        let lines: Vec<&str> = csv.lines().collect();
        assert!(lines[0].starts_with("group,benchmark,"));
        assert_eq!(lines.len(), 3); // header + 2 benchmarks
        assert!(lines[1].contains("zenflate"));
        assert!(lines[2].contains("libdeflate"));
        assert!(lines[1].contains("MiB/s"));
    }

    #[test]
    fn group_by_tag() {
        let result = make_suite_result();
        let grouped = result.group_by_tag("library");
        assert_eq!(grouped.len(), 2);
        assert!(grouped.contains_key("zenflate"));
        assert!(grouped.contains_key("libdeflate"));
    }

    #[test]
    fn format_ns_ranges() {
        assert_eq!(format_ns(500.0), "500.0ns");
        assert_eq!(format_ns(1_500.0), "1.50\u{b5}s");
        assert_eq!(format_ns(1_500_000.0), "1.50ms");
        assert_eq!(format_ns(1_500_000_000.0), "1.50s");
        assert_eq!(format_ns(-1_500_000.0), "-1.50ms");
    }

    #[test]
    fn csv_escape_special_chars() {
        assert_eq!(csv_escape("simple"), "simple");
        assert_eq!(csv_escape("has,comma"), "\"has,comma\"");
        assert_eq!(csv_escape("has\"quote"), "\"has\"\"quote\"");
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
