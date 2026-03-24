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
    /// Visual subgroup label (display-only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subgroup: Option<String>,
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

            // Group header with separator line
            let iters = comp.iterations_per_sample;
            let iters_str = if iters >= 1_000_000 {
                format!("{}M", iters / 1_000_000)
            } else if iters >= 1_000 {
                format!("{}K", iters / 1_000)
            } else {
                format!("{iters}")
            };
            let calls_str = if iters == 1 {
                "1 call (cold start)".to_string()
            } else {
                format!("{iters_str} calls")
            };
            let mut meta = format!("{} rounds × {calls_str}", comp.completed_rounds);
            if comp.cache_firewall {
                meta.push_str(", clear-L2");
            }
            if comp.expect_sub_ns {
                meta.push_str(", sub-ns mode");
            }
            if comp.baseline_only && comp.benchmarks.len() > 1 {
                meta.push_str(&format!(
                    ", baseline-only ({} benchmarks)",
                    comp.benchmarks.len(),
                ));
            }
            let header_text = format!("{} ", comp.group_name);
            let separator_len = 63usize.saturating_sub(header_text.len() + 2);
            eprintln!(
                "  {BOLD}{header_text}{RESET}{DIM}{}{RESET}",
                "─".repeat(separator_len),
            );
            eprintln!("  {DIM}{meta}{RESET}");

            // Row order: definition order by default, speed sort when configured
            let mut display_indices: Vec<usize> = (0..comp.benchmarks.len()).collect();
            if comp.sort_by_speed {
                display_indices.sort_by(|&a, &b| {
                    comp.benchmarks[a]
                        .summary
                        .mean
                        .total_cmp(&comp.benchmarks[b].summary.mean)
                });
            }

            let fastest_mean = comp
                .benchmarks
                .iter()
                .map(|b| b.summary.mean)
                .fold(f64::INFINITY, f64::min);

            let has_throughput = comp.throughput.is_some();
            let tp_unit = comp.throughput_unit.as_deref();
            let has_cpu = comp.benchmarks.iter().any(|b| b.cpu_summary.is_some());
            let has_comparisons = comp.benchmarks.len() >= 2;

            // Find the baseline name (first entry in analyses, or first benchmark)
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

            // Build analysis lookup: candidate name → analysis (only baseline pairs)
            let baseline_analyses: std::collections::HashMap<&str, &crate::stats::PairedAnalysis> =
                comp.analyses
                    .iter()
                    .filter(|(base, _, _)| base == baseline_name)
                    .map(|(_, cand, analysis)| (cand.as_str(), analysis))
                    .collect();

            // Compute column widths
            let name_w = comp
                .benchmarks
                .iter()
                .map(|b| b.name.len())
                .max()
                .unwrap_or(4)
                .max(9); // "benchmark"

            // Footnote collector
            let mut footnotes: Vec<String> = Vec::new();
            let mut add_footnote = |msg: String| -> usize {
                footnotes.push(msg);
                footnotes.len()
            };

            // Pre-compute drift markers (need footnote numbers before rows)
            let mut comparison_markers: std::collections::HashMap<(&str, &str), String> =
                std::collections::HashMap::new();
            for (base, cand, analysis) in &comp.analyses {
                if analysis.drift_correlation.abs() > 0.5 {
                    let direction = if analysis.drift_correlation > 0.0 {
                        "later rounds slower (thermal?)"
                    } else {
                        "later rounds faster (warmup?)"
                    };
                    let n = add_footnote(format!(
                        "drift r={:.2} — {direction}",
                        analysis.drift_correlation,
                    ));
                    comparison_markers
                        .entry((base.as_str(), cand.as_str()))
                        .or_default()
                        .push_str(&format!("[{n}]"));
                }
            }

            // Two-pass row construction:
            // 1. Collect raw numeric data and determine column-wide formatting
            // 2. Format all cells with consistent dp/alignment

            struct Row {
                name: String,
                min_col: String,
                mean_col: String,
                sigma_col: String, // σ (stddev)
                throughput: String,
                cpu: String,
                vs_base: String,
                vs_base_color: &'static str,
                is_fastest: bool,
                markers: String,
                subgroup: Option<String>,
            }

            // Check for group-level issues first
            if comp.completed_rounds < 10 {
                add_footnote(format!(
                    "only {} rounds — need 30+ for reliable statistics",
                    comp.completed_rounds,
                ));
            }

            // Pass 1: determine column-wide formatting params
            // Mean column: unit/dp based on the baseline (or fastest) mean
            let reference_mean = comp
                .benchmarks
                .iter()
                .find(|b| b.name == baseline_name)
                .unwrap_or(&comp.benchmarks[0])
                .summary
                .mean;
            let (mean_divisor, mean_unit, mean_dp) = ns_unit(reference_mean.abs());

            // vs-base percentage column: dp based on max percentage magnitude
            // Percentage dp: use the most precise (smallest magnitude) value
            // in the column to set dp for all values. No precision loss.
            let min_pct_abs = comp
                .analyses
                .iter()
                .filter(|(base, _, _)| base == baseline_name)
                .flat_map(|(_, _, a)| {
                    let bm = a.baseline.mean;
                    if bm.abs() > f64::EPSILON {
                        vec![
                            (a.ci_lower / bm * 100.0).abs(),
                            a.pct_change.abs(),
                            (a.ci_upper / bm * 100.0).abs(),
                        ]
                    } else {
                        vec![]
                    }
                })
                .fold(f64::INFINITY, f64::min);
            let pct_dp: usize = if min_pct_abs >= 1000.0 { 0 } else { 1 };
            let pct_fmt = |v: f64| -> String { format!("{v:+.*}", pct_dp) };

            // Pass 2a: compute raw formatted parts for each row
            struct RawRow {
                bench_idx: usize,
                is_fastest: bool,
                min_str: String,
                mean_str: String,
                sigma_str: String,
                vs_vals: [String; 3], // values with unit (e.g. "260ns" or "+1.5%")
                vs_base_color: &'static str,
                throughput: String,
                cpu: String,
                markers: String,
            }

            let mut raw_rows: Vec<RawRow> = Vec::with_capacity(display_indices.len());
            for &i in &display_indices {
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

                // Raw min and mean strings (no padding yet)
                // min = fastest observed run (real floor, not parametric)
                // mean = typical performance
                let min_str = format!("{:.*}", mean_dp, bench.summary.min / mean_divisor);
                let mean_str = format!("{:.*}", mean_dp, bench.summary.mean / mean_divisor);
                let sigma_str = format!("{:.*}", mean_dp, bench.summary.std_dev() / mean_divisor);

                // vs baseline column: unit inside brackets, shared width
                // Baseline: [260ns  262ns  265ns]
                // Comparison: [-1.5%  -1.3%  -1.0%]
                // All values (ns and %) share the same inner width for alignment.
                let is_baseline = bench.name == baseline_name;
                let (vs_vals, vs_base_color) = if is_baseline {
                    let ci_half = bench.summary.std_err() * 1.96;
                    let lo = (bench.summary.mean - ci_half).max(0.0);
                    let hi = bench.summary.mean + ci_half;
                    let v0 = format!("{:.*}{mean_unit}", mean_dp, lo / mean_divisor);
                    let v1 = format!(
                        "{:.*}{mean_unit}",
                        mean_dp,
                        bench.summary.mean / mean_divisor
                    );
                    let v2 = format!("{:.*}{mean_unit}", mean_dp, hi / mean_divisor);
                    ([v0, v1, v2], "")
                } else if let Some(analysis) = baseline_analyses.get(bench.name.as_str()) {
                    let base_mean = analysis.baseline.mean;
                    if base_mean.abs() > f64::EPSILON {
                        let lo_pct = analysis.ci_lower / base_mean * 100.0;
                        let mid_pct = analysis.pct_change;
                        let hi_pct = analysis.ci_upper / base_mean * 100.0;
                        let color = if analysis.significant {
                            if mid_pct < 0.0 { GREEN } else { RED }
                        } else {
                            DIM
                        };
                        let v0 = format!("{}%", pct_fmt(lo_pct));
                        let v1 = format!("{}%", pct_fmt(mid_pct));
                        let v2 = format!("{}%", pct_fmt(hi_pct));
                        ([v0, v1, v2], color)
                    } else {
                        let v = format!("{}%", pct_fmt(analysis.pct_change));
                        ([v, String::new(), String::new()], DIM)
                    }
                } else {
                    ([String::new(), String::new(), String::new()], DIM)
                };

                // Footnotes and markers (collected in this pass)
                let mut markers = String::new();

                if let Some(analysis) = baseline_analyses.get(bench.name.as_str()) {
                    if !analysis.significant {
                        let ci_lo_pct = if analysis.baseline.mean.abs() > f64::EPSILON {
                            format!(
                                "{:+.1}%",
                                analysis.ci_lower / analysis.baseline.mean * 100.0
                            )
                        } else {
                            format_ns(analysis.ci_lower)
                        };
                        let ci_hi_pct = if analysis.baseline.mean.abs() > f64::EPSILON {
                            format!(
                                "{:+.1}%",
                                analysis.ci_upper / analysis.baseline.mean * 100.0
                            )
                        } else {
                            format_ns(analysis.ci_upper)
                        };
                        let n = add_footnote(format!(
                            "CI [{ci_lo_pct} .. {ci_hi_pct}] crosses zero — \
                             cannot confirm a difference",
                        ));
                        markers.push_str(&format!("[{n}]"));
                    }
                    if analysis.significant && analysis.cohens_d.abs() < 0.2 {
                        let n = add_footnote(format!(
                            "real but tiny (effect {:.2}) — unlikely to matter",
                            analysis.cohens_d,
                        ));
                        markers.push_str(&format!("[{n}]"));
                    }
                    // Drift marker
                    if let Some(dm) = comparison_markers.get(&(baseline_name, bench.name.as_str()))
                    {
                        markers.push_str(dm);
                    }
                }
                let cv = bench.summary.cv();
                if cv > 0.20 && bench.summary.n > 10 {
                    let n = add_footnote(format!(
                        "CV={:.0}% — noisy, try a quieter system",
                        cv * 100.0,
                    ));
                    markers.push_str(&format!("[{n}]"));
                }
                if bench.summary.mean < 1.0
                    && bench.summary.n > 0
                    && (cv < 0.01 || bench.summary.variance < f64::EPSILON)
                {
                    let n = add_footnote(
                        "sub-ns with near-zero variance — likely optimized away".to_string(),
                    );
                    markers.push_str(&format!("[{n}]"));
                }

                raw_rows.push(RawRow {
                    bench_idx: i,
                    is_fastest,
                    min_str,
                    mean_str,
                    sigma_str,
                    vs_vals,
                    vs_base_color,
                    throughput: tp_str,
                    cpu: cpu_str,
                    markers,
                });
            }

            // Pass 2b: compute column-wide max widths
            let min_val_w = raw_rows.iter().map(|r| r.min_str.len()).max().unwrap_or(1);
            let mean_val_w = raw_rows.iter().map(|r| r.mean_str.len()).max().unwrap_or(1);
            let sigma_val_w = raw_rows
                .iter()
                .map(|r| r.sigma_str.len())
                .max()
                .unwrap_or(1);
            // Show σ column when there's room (no throughput), or always if no comparisons
            let show_sigma = !has_throughput || !has_comparisons;
            // vs_val_w from ALL rows (ns and % share the same inner width)
            let vs_val_w = raw_rows
                .iter()
                .flat_map(|r| r.vs_vals.iter())
                .filter(|s| !s.is_empty())
                .map(|s| s.len())
                .max()
                .unwrap_or(1);

            // Pass 2c: build final Row structs using uniform widths
            let mut rows: Vec<Row> = Vec::with_capacity(raw_rows.len());
            for raw in raw_rows {
                let bench = &comp.benchmarks[raw.bench_idx];
                let min_col = format!("{:>min_val_w$}{mean_unit}", raw.min_str);
                let mean_col = format!("{:>mean_val_w$}{mean_unit}", raw.mean_str);
                let sigma_col = format!("±{:>sigma_val_w$}{mean_unit}", raw.sigma_str);
                let vs_base = if !raw.vs_vals[0].is_empty() {
                    format!(
                        "[{:>vs_val_w$}  {:>vs_val_w$}  {:>vs_val_w$}]",
                        raw.vs_vals[0], raw.vs_vals[1], raw.vs_vals[2],
                    )
                } else {
                    String::new()
                };
                rows.push(Row {
                    name: bench.name.clone(),
                    min_col,
                    mean_col,
                    sigma_col,
                    throughput: raw.throughput,
                    cpu: raw.cpu,
                    vs_base,
                    vs_base_color: raw.vs_base_color,
                    is_fastest: raw.is_fastest,
                    markers: raw.markers,
                    subgroup: bench.subgroup.clone(),
                });
            }

            let min_w = rows
                .iter()
                .map(|r| r.min_col.len())
                .max()
                .unwrap_or(3)
                .max(3);
            let mean_w = rows
                .iter()
                .map(|r| r.mean_col.len())
                .max()
                .unwrap_or(4)
                .max(4);
            let sigma_w = rows
                .iter()
                .map(|r| r.sigma_col.len())
                .max()
                .unwrap_or(1)
                .max(1);
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
            let vs_w = if has_comparisons {
                rows.iter()
                    .map(|r| r.vs_base.len())
                    .max()
                    .unwrap_or(8)
                    .max(20) // header: [p5 · mean · p95] vs base
            } else {
                0
            };

            // Helper to build a table line with consistent column structure
            let add_col = |line: &mut String, width: usize, corner: char| {
                line.push_str(&format!("{corner}{}", "─".repeat(width + 2)));
            };

            // Build table borders and header
            let mut top = String::from("  ");
            let mut hdr = String::from("  ");
            let mut mid = String::from("  ");

            // Name column
            add_col(&mut top, name_w, '┌');
            hdr.push_str(&format!("│ {:<name_w$}", "benchmark"));
            add_col(&mut mid, name_w, '├');

            // Min column
            add_col(&mut top, min_w, '┬');
            hdr.push_str(&format!(" │ {:>min_w$}", "min"));
            add_col(&mut mid, min_w, '┼');

            // Mean column
            add_col(&mut top, mean_w, '┬');
            hdr.push_str(&format!(" │ {:>mean_w$}", "mean"));
            add_col(&mut mid, mean_w, '┼');

            // σ column
            if show_sigma {
                add_col(&mut top, sigma_w, '┬');
                hdr.push_str(&format!(" │ {:>sigma_w$}", "σ"));
                add_col(&mut mid, sigma_w, '┼');
            }

            // vs base column
            if has_comparisons {
                add_col(&mut top, vs_w, '┬');
                hdr.push_str(&format!(" │ {:^vs_w$}", "[lo · mean · hi] 95%ci"));
                add_col(&mut mid, vs_w, '┼');
            }

            if has_throughput {
                add_col(&mut top, tp_w, '┬');
                hdr.push_str(&format!(" │ {:>tp_w$}", "throughput"));
                add_col(&mut mid, tp_w, '┼');
            }
            if has_cpu {
                add_col(&mut top, cpu_w, '┬');
                hdr.push_str(&format!(" │ {:>cpu_w$}", "cpu"));
                add_col(&mut mid, cpu_w, '┼');
            }
            top.push('┐');
            hdr.push_str(" │");
            mid.push('┤');

            eprintln!("{DIM}{top}{RESET}");
            eprintln!("{DIM}{hdr}{RESET}");
            eprintln!("{DIM}{mid}{RESET}");

            let has_subgroups = rows.iter().any(|r| r.subgroup.is_some());
            // Inner width = top line chars minus "  ┌" prefix and "┐" suffix
            // top is like "  ┌───┬───┐" — inner is everything between ┌ and ┐
            let table_inner_w = top.chars().count().saturating_sub(4); // "  ┌" + "┐"

            // Data rows
            let mut current_subgroup: Option<&str> = None;
            for row in &rows {
                // Subgroup separator
                if has_subgroups {
                    let row_sg = row.subgroup.as_deref();
                    if row_sg != current_subgroup {
                        current_subgroup = row_sg;
                        if let Some(label) = row_sg {
                            // ├─ label ─────────┤  must span table_inner_w chars
                            // "─ " + label + " " + dashes = table_inner_w
                            let label_len = label.chars().count();
                            let used = 2 + label_len + 1; // "─ " + label + " "
                            let pad = table_inner_w.saturating_sub(used);
                            eprintln!(
                                "  {DIM}├─ {RESET}{BOLD}{label}{RESET}{DIM} {}┤{RESET}",
                                "─".repeat(pad),
                            );
                        }
                    }
                }
                let name_color = if row.is_fastest { GREEN } else { "" };
                let name_reset = if row.is_fastest { RESET } else { "" };

                let mut line = format!(
                    "  {DIM}│{RESET} {name_color}{:<name_w$}{name_reset}",
                    row.name,
                );

                line.push_str(&format!(
                    " {DIM}│{RESET} {:>min_w$} {DIM}│{RESET} {:>mean_w$}",
                    row.min_col, row.mean_col,
                ));
                if show_sigma {
                    line.push_str(&format!(
                        " {DIM}│{RESET} {DIM}{:>sigma_w$}{RESET}",
                        row.sigma_col,
                    ));
                }

                if has_comparisons {
                    let vc = row.vs_base_color;
                    let vr = if vc.is_empty() { "" } else { RESET };
                    line.push_str(&format!(" {DIM}│{RESET} {vc}{:>vs_w$}{vr}", row.vs_base));
                }

                if has_throughput {
                    line.push_str(&format!(
                        " {DIM}│{RESET} {CYAN}{:>tp_w$}{RESET}",
                        row.throughput,
                    ));
                }
                if has_cpu {
                    line.push_str(&format!(" {DIM}│{RESET} {DIM}{:>cpu_w$}{RESET}", row.cpu));
                }
                line.push_str(&format!(" {DIM}│{RESET}"));

                if !row.markers.is_empty() {
                    line.push_str(&format!(" {YELLOW}{}{RESET}", row.markers));
                }

                eprintln!("{line}");
            }

            // Bottom border
            let mut bot = String::from("  └");
            bot.push_str(&"─".repeat(name_w + 2));
            bot.push_str(&format!("┴{}", "─".repeat(min_w + 2)));
            bot.push_str(&format!("┴{}", "─".repeat(mean_w + 2)));
            if show_sigma {
                bot.push_str(&format!("┴{}", "─".repeat(sigma_w + 2)));
            }
            if has_comparisons {
                bot.push_str(&format!("┴{}", "─".repeat(vs_w + 2)));
            }
            if has_throughput {
                bot.push_str(&format!("┴{}", "─".repeat(tp_w + 2)));
            }
            if has_cpu {
                bot.push_str(&format!("┴{}", "─".repeat(cpu_w + 2)));
            }
            bot.push('┘');
            eprintln!("{DIM}{bot}{RESET}");

            // Terminal bar chart — always sorted fastest-first
            if rows.len() >= 2 {
                // Sort indices by speed for the bar chart regardless of table order
                let mut bar_indices: Vec<usize> = (0..comp.benchmarks.len()).collect();
                bar_indices.sort_by(|&a, &b| {
                    comp.benchmarks[a]
                        .summary
                        .mean
                        .total_cmp(&comp.benchmarks[b].summary.mean)
                });

                // Detect terminal width, default 80, cap bar to avoid wrapping
                let term_width = terminal_width().unwrap_or(80);
                // Layout: 2 indent + name_w + 2 gap + bar + 1 space + label
                // We need to figure out the label width first
                let bar_labels: Vec<String> = if has_throughput {
                    bar_indices
                        .iter()
                        .map(|&i| {
                            let b = &comp.benchmarks[i];
                            comp.throughput
                                .as_ref()
                                .map(|tp| tp.format_named(b.summary.mean, tp_unit))
                                .unwrap_or_default()
                        })
                        .collect()
                } else {
                    bar_indices
                        .iter()
                        .map(|&i| format_ns(comp.benchmarks[i].summary.mean))
                        .collect()
                };
                let label_w = bar_labels.iter().map(|l| l.len()).max().unwrap_or(0);
                let overhead = 2 + name_w + 2 + 1 + label_w;
                let bar_max = if term_width > overhead + 4 {
                    term_width - overhead
                } else {
                    20 // minimum bar width
                };

                // Scale: for throughput bars, higher = longer (better).
                // For time bars, higher = longer (worse — but fastest is highlighted green).
                let max_mean = comp
                    .benchmarks
                    .iter()
                    .map(|b| b.summary.mean)
                    .fold(0.0_f64, f64::max);

                if max_mean > 0.0 {
                    eprintln!();
                    for (idx, &bench_i) in bar_indices.iter().enumerate() {
                        let bench = &comp.benchmarks[bench_i];
                        let is_fastest = (bench.summary.mean - fastest_mean).abs() < f64::EPSILON;

                        let frac = bench.summary.mean / max_mean;
                        let bar_len = (frac * bar_max as f64).round().max(1.0) as usize;
                        let bar: String = "█".repeat(bar_len);

                        let name_color = if is_fastest { GREEN } else { "" };
                        let name_reset = if is_fastest { RESET } else { "" };
                        let bar_color = if is_fastest { GREEN } else { CYAN };

                        eprintln!(
                            "  {name_color}{:<name_w$}{name_reset}  {bar_color}{bar}{RESET} {DIM}{}{RESET}",
                            bench.name, bar_labels[idx],
                        );
                    }
                }
            }

            // Print footnotes for this group
            if !footnotes.is_empty() {
                eprintln!();
                for (i, note) in footnotes.iter().enumerate() {
                    eprintln!("  {YELLOW}[{}]{RESET} {DIM}{note}{RESET}", i + 1);
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
                mean_range: String,
                n: String,
                cpu: String,
            }
            let rows: Vec<StandaloneRow> = self
                .standalones
                .iter()
                .map(|bench| {
                    let ci_half = bench.summary.std_err() * 1.96;
                    let lo = (bench.summary.mean - ci_half).max(0.0);
                    let hi = bench.summary.mean + ci_half;
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
                        mean_range: format_ns_range(lo, bench.summary.mean, hi),
                        n: format!("{}", bench.summary.n),
                        cpu: cpu_str,
                    }
                })
                .collect();

            let mean_w = rows
                .iter()
                .map(|r| r.mean_range.len())
                .max()
                .unwrap_or(10)
                .max(10);
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
            hdr.push_str(&format!(" │ {:^mean_w$}", "lo  mean  hi"));
            mid.push_str(&format!("┼{}", "─".repeat(mean_w + 2)));
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
                    "  {DIM}│{RESET} {:<name_w$} {DIM}│{RESET} {:>mean_w$} {DIM}│{RESET} {:>n_w$}",
                    row.name, row.mean_range, row.n,
                );
                if has_cpu {
                    line.push_str(&format!(" {DIM}│{RESET} {DIM}{:>cpu_w$}{RESET}", row.cpu,));
                }
                line.push_str(&format!(" {DIM}│{RESET}"));
                eprintln!("{line}");
            }

            let mut bot = format!("  └{}", "─".repeat(name_w + 2));
            bot.push_str(&format!("┴{}", "─".repeat(mean_w + 2)));
            bot.push_str(&format!("┴{}", "─".repeat(n_w + 2)));
            if has_cpu {
                bot.push_str(&format!("┴{}", "─".repeat(cpu_w + 2)));
            }
            bot.push('┘');
            eprintln!("{DIM}{bot}{RESET}");
        }

        eprintln!();
        eprintln!(
            "  {DIM}total: {:.1}s  gate waits: {} ({:.1}s){RESET}",
            self.total_time.as_secs_f64(),
            self.gate_waits,
            self.gate_wait_time.as_secs_f64(),
        );
        if self.unreliable {
            eprintln!("  {RED}{BOLD}⚠ UNRELIABLE: too many resource gate waits{RESET}");
        }
        eprintln!("  {DIM}gate checks: CPU load, free RAM, CPU temp, heavy processes{RESET}",);
        eprintln!(
            "  {DIM}not checked: disk I/O, network, frequency scaling, VM/container noise{RESET}",
        );
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
/// Detect terminal width. Checks `COLUMNS` env var, falls back to 80.
fn terminal_width() -> Option<usize> {
    if let Ok(cols) = std::env::var("COLUMNS") {
        if let Ok(w) = cols.parse::<usize>() {
            if w > 0 {
                return Some(w);
            }
        }
    }
    None
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

/// Pick unit and decimal places for a nanosecond value.
/// Returns (divisor, unit_str, decimal_places).
fn ns_unit(mean_abs: f64) -> (f64, &'static str, usize) {
    if mean_abs >= 1_000_000_000.0 {
        (1_000_000_000.0, "s", 2)
    } else if mean_abs >= 1_000_000.0 {
        (1_000_000.0, "ms", 1)
    } else if mean_abs >= 1_000.0 {
        (1_000.0, "µs", 1)
    } else if mean_abs >= 100.0 {
        (1.0, "ns", 0)
    } else if mean_abs >= 10.0 {
        (1.0, "ns", 1)
    } else {
        (1.0, "ns", 2)
    }
}

/// Format a [lo mean hi] range with shared unit and aligned columns.
fn format_ns_range(lo: f64, mean: f64, hi: f64) -> String {
    let (divisor, unit, dp) = ns_unit(mean.abs());
    let vals: Vec<String> = [lo, mean, hi]
        .iter()
        .map(|&v| format!("{:.*}", dp, v / divisor))
        .collect();
    let w = vals.iter().map(|s| s.len()).max().unwrap_or(1);
    format!("[{:>w$}  {:>w$}  {:>w$}]{unit}", vals[0], vals[1], vals[2],)
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
            subgroup: None,
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
                    },
                    BenchmarkResult {
                        name: "libdeflate".to_string(),
                        summary: make_summary(10_000_000.0), // 10ms
                        cpu_summary: None,
                        tags: vec![("library".to_string(), "libdeflate".to_string())],
                        subgroup: None,
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
