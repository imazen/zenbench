use crate::bench::Throughput;
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
    /// Custom unit name for Elements throughput (e.g., "checks" → "Gchecks/s").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub throughput_unit: Option<String>,
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
        // ANSI color codes
        const RESET: &str = "\x1b[0m";
        const BOLD: &str = "\x1b[1m";
        const DIM: &str = "\x1b[2m";
        const GREEN: &str = "\x1b[32m";
        const RED: &str = "\x1b[31m";
        const YELLOW: &str = "\x1b[33m";
        const CYAN: &str = "\x1b[36m";
        const BOLD_WHITE: &str = "\x1b[1;37m";

        eprintln!();
        eprintln!(
            "{BOLD_WHITE}═══════════════════════════════════════════════════════════════{RESET}"
        );
        eprintln!("{BOLD_WHITE}  zenbench{RESET}  {DIM}{}{RESET}", self.run_id);
        if let Some(hash) = &self.git_hash {
            eprintln!("  {DIM}git:{RESET} {hash}");
        }
        if let Some(ci) = &self.ci_environment {
            eprintln!("  {DIM}ci:{RESET}  {ci}");
        }
        eprintln!(
            "{BOLD_WHITE}═══════════════════════════════════════════════════════════════{RESET}"
        );

        for comp in &self.comparisons {
            eprintln!();

            // Group header with config info
            let mut meta_parts = vec![format!("{} rounds", comp.completed_rounds)];
            if comp.cache_firewall {
                meta_parts.push(format!(
                    "cache_firewall: {}",
                    format_bytes(comp.cache_firewall_bytes),
                ));
            }
            let meta = meta_parts.join(", ");
            eprintln!("  {BOLD}{}{RESET}  {DIM}({meta}){RESET}", comp.group_name);

            // Sort benchmarks by mean time (fastest first) for display
            let mut sorted_indices: Vec<usize> = (0..comp.benchmarks.len()).collect();
            sorted_indices.sort_by(|&a, &b| {
                comp.benchmarks[a]
                    .summary
                    .mean
                    .total_cmp(&comp.benchmarks[b].summary.mean)
            });

            let fastest_mean = sorted_indices
                .first()
                .map(|&i| comp.benchmarks[i].summary.mean)
                .unwrap_or(0.0);

            let has_throughput = comp.throughput.is_some();
            let tp_unit = comp.throughput_unit.as_deref();
            let has_cpu = comp.benchmarks.iter().any(|b| b.cpu_summary.is_some());

            // Compute column widths
            let name_w = comp
                .benchmarks
                .iter()
                .map(|b| b.name.len())
                .max()
                .unwrap_or(4)
                .max(9); // "benchmark"

            // Pre-format all cells to measure widths
            struct Row {
                name: String,
                mean: String,
                stddev: String,
                throughput: String,
                cpu: String,
                is_fastest: bool,
            }

            let rows: Vec<Row> = sorted_indices
                .iter()
                .map(|&i| {
                    let bench = &comp.benchmarks[i];
                    let is_fastest = (bench.summary.mean - fastest_mean).abs() < f64::EPSILON
                        && comp.benchmarks.len() > 1;
                    let tp_str = comp
                        .throughput
                        .as_ref()
                        .map(|tp| tp.format_named(bench.summary.mean, tp_unit))
                        .unwrap_or_default();
                    let cpu_str = bench
                        .cpu_summary
                        .as_ref()
                        .map(|cpu| {
                            let eff = if bench.summary.mean > 0.0 {
                                cpu.mean / bench.summary.mean * 100.0
                            } else {
                                0.0
                            };
                            format!("{} ({eff:.0}%)", format_ns(cpu.mean))
                        })
                        .unwrap_or_default();
                    Row {
                        name: bench.name.clone(),
                        mean: format_ns(bench.summary.mean),
                        stddev: format!("±{}", format_ns(bench.summary.std_dev())),
                        throughput: tp_str,
                        cpu: cpu_str,
                        is_fastest,
                    }
                })
                .collect();

            let mean_w = rows.iter().map(|r| r.mean.len()).max().unwrap_or(4).max(4);
            let sd_w = rows
                .iter()
                .map(|r| r.stddev.len())
                .max()
                .unwrap_or(6)
                .max(6);
            let tp_w = if has_throughput {
                rows.iter()
                    .map(|r| r.throughput.len())
                    .max()
                    .unwrap_or(10)
                    .max(10)
            } else {
                0
            };
            let cpu_w = if has_cpu {
                rows.iter().map(|r| r.cpu.len()).max().unwrap_or(3).max(3)
            } else {
                0
            };

            // Build table
            // Top border
            let mut top = format!("  ┌{}", "─".repeat(name_w + 2));
            let mut hdr = format!("  │ {:<name_w$}", "benchmark");
            let mut mid = format!("  ├{}", "─".repeat(name_w + 2));
            top.push_str(&format!("┬{}", "─".repeat(mean_w + 2)));
            hdr.push_str(&format!(" │ {:>mean_w$}", "mean"));
            mid.push_str(&format!("┼{}", "─".repeat(mean_w + 2)));
            top.push_str(&format!("┬{}", "─".repeat(sd_w + 2)));
            hdr.push_str(&format!(" │ {:>sd_w$}", "stddev"));
            mid.push_str(&format!("┼{}", "─".repeat(sd_w + 2)));
            if has_throughput {
                top.push_str(&format!("┬{}", "─".repeat(tp_w + 2)));
                hdr.push_str(&format!(" │ {:>tp_w$}", "throughput"));
                mid.push_str(&format!("┼{}", "─".repeat(tp_w + 2)));
            }
            if has_cpu {
                top.push_str(&format!("┬{}", "─".repeat(cpu_w + 2)));
                hdr.push_str(&format!(" │ {:>cpu_w$}", "cpu"));
                mid.push_str(&format!("┼{}", "─".repeat(cpu_w + 2)));
            }
            top.push('┐');
            hdr.push_str(" │");
            mid.push('┤');

            eprintln!("{DIM}{top}{RESET}");
            eprintln!("{DIM}{hdr}{RESET}");
            eprintln!("{DIM}{mid}{RESET}");

            // Data rows
            for row in &rows {
                let name_color = if row.is_fastest { GREEN } else { "" };
                let name_reset = if row.is_fastest { RESET } else { "" };

                let mut line = format!(
                    "  {DIM}│{RESET} {name_color}{:<name_w$}{name_reset} {DIM}│{RESET} {:>mean_w$} {DIM}│{RESET} {DIM}{:>sd_w$}{RESET}",
                    row.name, row.mean, row.stddev,
                );

                if has_throughput {
                    line.push_str(&format!(
                        " {DIM}│{RESET} {CYAN}{:>tp_w$}{RESET}",
                        row.throughput,
                    ));
                }
                if has_cpu {
                    line.push_str(&format!(" {DIM}│{RESET} {DIM}{:>cpu_w$}{RESET}", row.cpu,));
                }
                line.push_str(&format!(" {DIM}│{RESET}"));

                eprintln!("{line}");
            }

            // Bottom border
            let mut bot = format!("  └{}", "─".repeat(name_w + 2));
            bot.push_str(&format!("┴{}", "─".repeat(mean_w + 2)));
            bot.push_str(&format!("┴{}", "─".repeat(sd_w + 2)));
            if has_throughput {
                bot.push_str(&format!("┴{}", "─".repeat(tp_w + 2)));
            }
            if has_cpu {
                bot.push_str(&format!("┴{}", "─".repeat(cpu_w + 2)));
            }
            bot.push('┘');
            eprintln!("{DIM}{bot}{RESET}");

            // Paired comparisons
            for (base, cand, analysis) in &comp.analyses {
                let (color, arrow) = if analysis.pct_change < -1.0 {
                    (GREEN, "faster")
                } else if analysis.pct_change > 1.0 {
                    (RED, "slower")
                } else {
                    (DIM, "same")
                };
                let sig_marker = if analysis.significant {
                    format!(" {BOLD}*{RESET}")
                } else {
                    String::new()
                };

                let throughput_delta = if has_throughput {
                    let tp_pct = if analysis.pct_change > -100.0 {
                        -analysis.pct_change / (1.0 + analysis.pct_change / 100.0)
                    } else {
                        f64::INFINITY
                    };
                    let tp_color = if tp_pct > 1.0 {
                        GREEN
                    } else if tp_pct < -1.0 {
                        RED
                    } else {
                        DIM
                    };
                    format!("  {tp_color}throughput {:+.1}%{RESET}", tp_pct)
                } else {
                    String::new()
                };

                eprintln!(
                    "  {DIM}{base} vs {cand}:{RESET}  \
                     {color}{:+.2}% ({arrow}){RESET}{sig_marker}  \
                     {DIM}d={:.2}  p={:.4}  CI [{}, {}]{RESET}{throughput_delta}",
                    analysis.pct_change,
                    analysis.cohens_d,
                    analysis.wilcoxon_p,
                    format_ns(analysis.ci_lower),
                    format_ns(analysis.ci_upper),
                );

                if analysis.drift_correlation.abs() > 0.5 {
                    eprintln!(
                        "    {YELLOW}⚠ drift r={:.2}{RESET}",
                        analysis.drift_correlation,
                    );
                }
            }
        }

        // Standalone results
        if !self.standalones.is_empty() {
            eprintln!();
            eprintln!("  {BOLD}standalone{RESET}");

            let name_w = self
                .standalones
                .iter()
                .map(|b| b.name.len())
                .max()
                .unwrap_or(9)
                .max(9);

            let has_cpu = self.standalones.iter().any(|b| b.cpu_summary.is_some());

            // Pre-format
            struct StandaloneRow {
                name: String,
                mean: String,
                stddev: String,
                n: String,
                cpu: String,
            }
            let rows: Vec<StandaloneRow> = self
                .standalones
                .iter()
                .map(|bench| {
                    let cpu_str = bench
                        .cpu_summary
                        .as_ref()
                        .map(|cpu| {
                            let eff = if bench.summary.mean > 0.0 {
                                cpu.mean / bench.summary.mean * 100.0
                            } else {
                                0.0
                            };
                            format!("{} ({eff:.0}%)", format_ns(cpu.mean))
                        })
                        .unwrap_or_default();
                    StandaloneRow {
                        name: bench.name.clone(),
                        mean: format_ns(bench.summary.mean),
                        stddev: format!("±{}", format_ns(bench.summary.std_dev())),
                        n: format!("{}", bench.summary.n),
                        cpu: cpu_str,
                    }
                })
                .collect();

            let mean_w = rows.iter().map(|r| r.mean.len()).max().unwrap_or(4).max(4);
            let sd_w = rows
                .iter()
                .map(|r| r.stddev.len())
                .max()
                .unwrap_or(6)
                .max(6);
            let n_w = rows.iter().map(|r| r.n.len()).max().unwrap_or(1).max(1);
            let cpu_w = if has_cpu {
                rows.iter().map(|r| r.cpu.len()).max().unwrap_or(3).max(3)
            } else {
                0
            };

            let mut top = format!("  ┌{}", "─".repeat(name_w + 2));
            let mut mid = format!("  ├{}", "─".repeat(name_w + 2));
            let mut hdr = format!("  │ {:<name_w$}", "benchmark");
            top.push_str(&format!("┬{}", "─".repeat(mean_w + 2)));
            hdr.push_str(&format!(" │ {:>mean_w$}", "mean"));
            mid.push_str(&format!("┼{}", "─".repeat(mean_w + 2)));
            top.push_str(&format!("┬{}", "─".repeat(sd_w + 2)));
            hdr.push_str(&format!(" │ {:>sd_w$}", "stddev"));
            mid.push_str(&format!("┼{}", "─".repeat(sd_w + 2)));
            top.push_str(&format!("┬{}", "─".repeat(n_w + 2)));
            hdr.push_str(&format!(" │ {:>n_w$}", "n"));
            mid.push_str(&format!("┼{}", "─".repeat(n_w + 2)));
            if has_cpu {
                top.push_str(&format!("┬{}", "─".repeat(cpu_w + 2)));
                hdr.push_str(&format!(" │ {:>cpu_w$}", "cpu"));
                mid.push_str(&format!("┼{}", "─".repeat(cpu_w + 2)));
            }
            top.push('┐');
            hdr.push_str(" │");
            mid.push('┤');

            eprintln!("{DIM}{top}{RESET}");
            eprintln!("{DIM}{hdr}{RESET}");
            eprintln!("{DIM}{mid}{RESET}");

            for row in &rows {
                let mut line = format!(
                    "  {DIM}│{RESET} {:<name_w$} {DIM}│{RESET} {:>mean_w$} {DIM}│{RESET} {DIM}{:>sd_w$}{RESET} {DIM}│{RESET} {:>n_w$}",
                    row.name, row.mean, row.stddev, row.n,
                );
                if has_cpu {
                    line.push_str(&format!(" {DIM}│{RESET} {DIM}{:>cpu_w$}{RESET}", row.cpu,));
                }
                line.push_str(&format!(" {DIM}│{RESET}"));
                eprintln!("{line}");
            }

            let mut bot = format!("  └{}", "─".repeat(name_w + 2));
            bot.push_str(&format!("┴{}", "─".repeat(mean_w + 2)));
            bot.push_str(&format!("┴{}", "─".repeat(sd_w + 2)));
            bot.push_str(&format!("┴{}", "─".repeat(n_w + 2)));
            if has_cpu {
                bot.push_str(&format!("┴{}", "─".repeat(cpu_w + 2)));
            }
            bot.push('┘');
            eprintln!("{DIM}{bot}{RESET}");
        }

        eprintln!();
        eprintln!(
            "  {DIM}total: {:.1}s  waits: {} ({:.1}s){RESET}",
            self.total_time.as_secs_f64(),
            self.gate_waits,
            self.gate_wait_time.as_secs_f64(),
        );
        if self.unreliable {
            eprintln!("  {RED}{BOLD}⚠ UNRELIABLE: too many resource gate waits{RESET}");
        }
        eprintln!(
            "{BOLD_WHITE}═══════════════════════════════════════════════════════════════{RESET}"
        );
        eprintln!();
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
                        "| {} | {} | {} | {} | ±{} |\n",
                        bench.name,
                        format_ns(bench.summary.mean),
                        cpu_mean,
                        efficiency,
                        format_ns(bench.summary.std_dev()),
                    ));
                } else {
                    out.push_str(&format!(
                        "| {} | {} | {} | ±{} |\n",
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
                out.push_str(&format_bar_chart(
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
                    "| {} | {} | {} | ±{} |\n",
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

/// Format bytes as human-readable size.
fn format_bytes(bytes: usize) -> String {
    if bytes >= 1024 * 1024 {
        format!("{} MiB", bytes / (1024 * 1024))
    } else if bytes >= 1024 {
        format!("{} KiB", bytes / 1024)
    } else {
        format!("{bytes} B")
    }
}

/// Format nanoseconds as human-readable time.
pub fn format_ns(ns: f64) -> String {
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

/// Generate a text-based bar chart for a group of benchmarks.
///
/// Returns a fenced code block that renders as monospace in markdown.
fn format_bar_chart(
    benchmarks: &[BenchmarkResult],
    throughput: Option<&Throughput>,
    throughput_unit: Option<&str>,
) -> String {
    const BAR_WIDTH: usize = 30;
    let blocks = ['█', '▉', '▊', '▋', '▌', '▍', '▎', '▏'];

    if benchmarks.is_empty() {
        return String::new();
    }

    // Determine the metric to chart: throughput (higher=better) or time (lower=better)
    let (values, labels): (Vec<f64>, Vec<String>) = if let Some(tp) = throughput {
        // Chart throughput (higher is better)
        benchmarks
            .iter()
            .map(|b| {
                let (val, unit) = tp.compute_named(b.summary.mean, throughput_unit);
                (val, format!("{val:.1} {unit}"))
            })
            .unzip()
    } else {
        // Chart time (lower is better)
        benchmarks
            .iter()
            .map(|b| {
                let ns = b.summary.mean;
                (ns, format_ns(ns))
            })
            .unzip()
    };

    let max_val = values.iter().cloned().fold(0.0_f64, f64::max);
    if max_val <= 0.0 {
        return String::new();
    }

    let max_name_len = benchmarks.iter().map(|b| b.name.len()).max().unwrap_or(0);
    let max_label_len = labels.iter().map(|l| l.len()).max().unwrap_or(0);

    let mut out = String::from("```\n");

    for (i, bench) in benchmarks.iter().enumerate() {
        let frac = values[i] / max_val;
        let filled = frac * BAR_WIDTH as f64;
        let full_blocks = filled as usize;
        let partial = ((filled - full_blocks as f64) * 8.0) as usize;

        let mut bar = String::with_capacity(BAR_WIDTH);
        for _ in 0..full_blocks.min(BAR_WIDTH) {
            bar.push('█');
        }
        if full_blocks < BAR_WIDTH && partial > 0 {
            bar.push(blocks[8 - partial]);
        }
        // Pad to BAR_WIDTH
        while bar.chars().count() < BAR_WIDTH {
            bar.push(' ');
        }

        out.push_str(&format!(
            "  {:<width$}  |{bar}| {:>lwidth$}\n",
            bench.name,
            labels[i],
            width = max_name_len,
            lwidth = max_label_len,
        ));
    }

    out.push_str("```\n");
    out
}

/// Escape a string for CSV (double-quote if it contains comma, quote, or newline).
fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::Summary;
    use std::time::Duration;

    fn make_summary(mean_ns: f64) -> Summary {
        // Minimal summary for testing
        Summary::from_slice(&[mean_ns])
    }

    #[test]
    fn throughput_bytes_mibs() {
        let tp = Throughput::Bytes(1_048_576); // 1 MiB
        // 1 MiB in 1ms = 1000 MiB/s → should show GiB/s
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
                    },
                    BenchmarkResult {
                        name: "libdeflate".to_string(),
                        summary: make_summary(10_000_000.0), // 10ms
                        cpu_summary: None,
                        tags: vec![("library".to_string(), "libdeflate".to_string())],
                    },
                ],
                analyses: vec![],
                completed_rounds: 100,
                throughput: Some(Throughput::Bytes(1_048_576)), // 1 MiB
                cache_firewall: false,
                cache_firewall_bytes: 0,
                baseline_only: false,
                throughput_unit: None,
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
        assert!(md.contains('█'), "Missing bar characters");
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
        assert_eq!(format_ns(1_500.0), "1.50µs");
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
