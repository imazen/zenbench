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
#[non_exhaustive]
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
    /// Bootstrap 95% confidence interval for this benchmark's mean time.
    ///
    /// Computed when there are enough samples (≥ 2 rounds). Tells you how
    /// confident you can be in the absolute mean — "this function takes
    /// 245 ± 3ns" rather than just "245ns."
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mean_ci: Option<crate::stats::MeanCi>,
    /// OLS slope estimate (ns/iter) from linear sampling mode.
    /// More accurate than mean for sub-100ns benchmarks — separates per-iteration
    /// cost from constant overhead (timer, black_box, dispatch).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slope_ns: Option<f64>,
    /// Timer ticks per sample: `(mean_ns * iterations_per_sample) / timer_resolution_ns`.
    /// Values below ~50 mean the measurement is quantization-limited — differences
    /// smaller than `timer_res / iterations` are undetectable.
    #[serde(default)]
    pub timer_ticks_per_sample: f64,
    /// Allocation statistics (when `alloc-profiling` feature is active and
    /// `AllocProfiler` is installed as the global allocator).
    #[cfg(feature = "alloc-profiling")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alloc_stats: Option<crate::alloc::AllocStats>,
}

impl Default for BenchmarkResult {
    fn default() -> Self {
        Self {
            name: String::new(),
            summary: Summary::new(),
            cpu_summary: None,
            tags: Vec::new(),
            subgroup: None,
            cold_start_ns: 0.0,
            mean_ci: None,
            slope_ns: None,
            timer_ticks_per_sample: 0.0,
            #[cfg(feature = "alloc-profiling")]
            alloc_stats: None,
        }
    }
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
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[non_exhaustive]
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
    /// Whether cold-start mode was used.
    #[serde(default)]
    pub cold_start: bool,
    /// Base iterations per sample (before jitter). 0 if unknown.
    #[serde(default)]
    pub iterations_per_sample: usize,
}

/// Complete results of a benchmark suite run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
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
    /// Timer resolution in nanoseconds (measured at startup).
    #[serde(default)]
    pub timer_resolution_ns: u64,
    /// Per-iteration loop overhead in nanoseconds (subtracted from measurements).
    ///
    /// Measures the cost of the benchmark harness itself: loop control flow,
    /// `black_box` barrier, branch prediction overhead. Subtracted from all
    /// sample times so reported values reflect only the user's code.
    #[serde(default)]
    pub loop_overhead_ns: f64,
    /// Hardware fingerprint for testbed identification.
    /// Used by baseline comparison to detect hardware changes between runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub testbed: Option<crate::platform::Testbed>,
    /// Hardware calibration results from built-in workloads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calibration: Option<crate::calibration::Calibration>,
}

impl Default for SuiteResult {
    fn default() -> Self {
        Self {
            run_id: RunId(String::new()),
            timestamp: String::new(),
            git_hash: None,
            ci_environment: None,
            comparisons: Vec::new(),
            standalones: Vec::new(),
            total_time: Duration::ZERO,
            gate_waits: 0,
            gate_wait_time: Duration::ZERO,
            unreliable: false,
            timer_resolution_ns: 0,
            loop_overhead_ns: 0.0,
            testbed: None,
            calibration: None,
        }
    }
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

    /// Generate a self-contained HTML report with inline SVG bar charts.
    ///
    /// Each group is a collapsible `<details>` section with a table and
    /// bar chart. No external dependencies or JavaScript required.
    pub fn to_html(&self) -> String {
        crate::html::to_html(self)
    }

    /// Save each comparison group's SVG bar chart as a standalone `.svg` file.
    ///
    /// Files are named `{group_name}.svg` (with `/` replaced by `_`).
    /// Matrix-structured groups (benchmark names with `variant/param` format)
    /// automatically render as grouped bar charts with per-section scaling.
    pub fn save_charts(&self, dir: impl AsRef<Path>) -> std::io::Result<()> {
        let dir = dir.as_ref();
        std::fs::create_dir_all(dir)?;
        for comp in &self.comparisons {
            let svg = crate::html::render_chart_standalone(comp);
            if svg.is_empty() {
                continue;
            }
            let filename = comp.group_name.replace(['/', ' '], "_");
            std::fs::write(dir.join(format!("{filename}.svg")), &svg)?;
        }
        Ok(())
    }

    /// Save publication-quality SVG charts via charts-rs.
    ///
    /// Requires the `charts` feature. Produces polished bar charts with
    /// proper fonts, gridlines, legends, and value labels. Supports
    /// `"light"`, `"dark"`, `"grafana"`, `"vintage"`, and other charts-rs themes.
    ///
    /// For matrix-structured groups (benchmarks named `variant/param`),
    /// produces grouped bar charts with one series per variant.
    #[cfg(feature = "charts")]
    pub fn save_publication_charts(
        &self,
        dir: impl AsRef<Path>,
        config: &crate::charts::ChartConfig,
    ) -> std::io::Result<()> {
        crate::charts::save_charts(self, dir.as_ref(), config)
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

            let analyses: std::collections::HashMap<&str, &PairedAnalysis> = comp
                .analyses
                .iter()
                .filter(|(base, _, _)| base == baseline_name)
                .map(|(_, cand, a)| (cand.as_str(), a))
                .collect();

            for bench in &comp.benchmarks {
                let s = &bench.summary;

                // Section 1: Identity
                let mut identity = vec![format!("group={}", llm_quote(&comp.group_name))];
                if let Some(sg) = &bench.subgroup {
                    identity.push(format!("subgroup={}", llm_quote(sg)));
                }
                identity.push(format!("benchmark={}", llm_quote(&bench.name)));

                // Section 2: Comparison (the number you care about most)
                let mut comparison = Vec::new();
                if bench.name == baseline_name {
                    comparison.push("vs_base=baseline".to_string());
                } else if let Some(analysis) = analyses.get(bench.name.as_str()) {
                    let base_mean = analysis.baseline.mean;
                    comparison.push(format!("vs_base_pct={:+.2}", analysis.pct_change));
                    if base_mean.abs() > f64::EPSILON {
                        comparison.push(format!(
                            "ci=[{:+.2}% {:+.2}%]",
                            analysis.ci_lower / base_mean * 100.0,
                            analysis.ci_upper / base_mean * 100.0,
                        ));
                    }
                    comparison.push(format!("significant={}", analysis.significant));
                    if analysis.resolution_limited {
                        comparison.push("resolution_limited=true".to_string());
                    }
                    comparison.push(format!("effect={:.2}", analysis.cohens_d));
                    comparison.push(format!("p={:.4}", analysis.wilcoxon_p));
                }

                // Section 3: Measurement
                let measurement = [
                    format!("min={}", crate::format::format_ns(s.min)),
                    format!("mean={}", crate::format::format_ns(s.mean)),
                    format!("median={}", crate::format::format_ns(s.median)),
                    format!("mad={}", crate::format::format_ns(s.mad)),
                ];

                // Section 4: Throughput (if set)
                let mut throughput = Vec::new();
                if let Some(tp) = &comp.throughput {
                    let (val, unit) = tp.compute(s.mean, comp.throughput_unit.as_deref());
                    throughput.push(format!("throughput={val:.2} {unit}"));
                }

                // Section 5: Metadata
                let mut meta = vec![
                    format!("n={}", s.n),
                    format!("cv={:.1}%", s.cv() * 100.0),
                    format!("rounds={}", comp.completed_rounds),
                    format!("calls={}", comp.iterations_per_sample),
                ];
                if let Some(ci) = &bench.mean_ci {
                    meta.push(format!(
                        "mean_ci=[{} {}]",
                        crate::format::format_ns(ci.lower),
                        crate::format::format_ns(ci.upper),
                    ));
                }
                if bench.cold_start_ns > 0.0 {
                    meta.push(format!(
                        "cold={}",
                        crate::format::format_ns(bench.cold_start_ns),
                    ));
                }
                if bench.timer_ticks_per_sample < 50.0 {
                    meta.push(format!(
                        "timer_ticks={:.0} (resolution-limited)",
                        bench.timer_ticks_per_sample,
                    ));
                }
                for (k, v) in &bench.tags {
                    meta.push(format!("{k}={}", llm_quote(v)));
                }

                // Allocation stats (when profiler is active)
                #[cfg(feature = "alloc-profiling")]
                if let Some(alloc) = &bench.alloc_stats {
                    meta.push(format!("allocs/iter={:.1}", alloc.allocs_per_iter));
                    meta.push(format!("bytes/iter={:.0}", alloc.bytes_per_iter));
                    if alloc.reallocs_per_iter > 0.0 {
                        meta.push(format!("reallocs/iter={:.1}", alloc.reallocs_per_iter));
                    }
                }

                // Join sections with " | " for visual grouping
                let mut sections: Vec<String> = Vec::new();
                sections.push(identity.join(" "));
                if !comparison.is_empty() {
                    sections.push(comparison.join(" "));
                }
                sections.push(measurement.join(" "));
                if !throughput.is_empty() {
                    sections.push(throughput.join(" "));
                }
                sections.push(meta.join(" "));

                out.push_str(&sections.join("  |  "));
                out.push('\n');
            }
        }

        // Standalone benchmarks
        for bench in &self.standalones {
            let s = &bench.summary;
            let identity = format!("benchmark={}", llm_quote(&bench.name));
            let measurement = format!(
                "min={} mean={} median={} mad={}",
                crate::format::format_ns(s.min),
                crate::format::format_ns(s.mean),
                crate::format::format_ns(s.median),
                crate::format::format_ns(s.mad),
            );
            let mut meta = format!("n={} cv={:.1}%", s.n, s.cv() * 100.0);
            if bench.cold_start_ns > 0.0 {
                meta.push_str(&format!(
                    " cold={}",
                    crate::format::format_ns(bench.cold_start_ns),
                ));
            }
            out.push_str(&format!("{identity}  |  {measurement}  |  {meta}\n"));
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
            out.push_str(&format!("## {}\n\n", comp.group_name));

            // Methodology line
            let calls_str = if comp.iterations_per_sample == 1 {
                "1 call (cold start)".to_string()
            } else {
                format!("{} calls", comp.iterations_per_sample)
            };
            out.push_str(&format!(
                "*{} rounds \u{d7} {}*\n\n",
                comp.completed_rounds, calls_str
            ));

            let has_throughput = comp.throughput.is_some();

            // Find baseline name
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

            // Build analysis lookup: candidate name -> analysis (only baseline pairs)
            let analyses: std::collections::HashMap<&str, &PairedAnalysis> = comp
                .analyses
                .iter()
                .filter(|(base, _, _)| base == baseline_name)
                .map(|(_, cand, a)| (cand.as_str(), a))
                .collect();

            let has_comparisons = comp.benchmarks.len() >= 2;

            // Table header
            if has_throughput && has_comparisons {
                out.push_str("| Benchmark | Min | Mean | vs Base | Throughput |\n");
                out.push_str("|-----------|-----|------|---------|------------|\n");
            } else if has_throughput {
                out.push_str("| Benchmark | Min | Mean | Throughput |\n");
                out.push_str("|-----------|-----|------|------------|\n");
            } else if has_comparisons {
                out.push_str("| Benchmark | Min | Mean | vs Base |\n");
                out.push_str("|-----------|-----|------|----------|\n");
            } else {
                out.push_str("| Benchmark | Min | Mean |\n");
                out.push_str("|-----------|-----|------|\n");
            }

            let has_subgroups = comp.benchmarks.iter().any(|b| b.subgroup.is_some());
            let mut current_subgroup: Option<&str> = None;

            for bench in &comp.benchmarks {
                // Subgroup header row
                if has_subgroups {
                    let row_sg = bench.subgroup.as_deref();
                    if row_sg != current_subgroup {
                        current_subgroup = row_sg;
                        if let Some(label) = row_sg {
                            let n_cols = if has_throughput && has_comparisons {
                                5
                            } else if has_throughput || has_comparisons {
                                4
                            } else {
                                3
                            };
                            let empty_cols = " |".repeat(n_cols - 1);
                            out.push_str(&format!("| **{label}** |{empty_cols}\n"));
                        }
                    }
                }

                let min_str = format_ns(bench.summary.min);
                let mean_str = format_ns(bench.summary.mean);

                // vs Base column
                let res_flag = analyses
                    .get(bench.name.as_str())
                    .is_some_and(|a| a.resolution_limited);
                let vs_base = if has_comparisons {
                    if bench.name == baseline_name {
                        mean_str.clone()
                    } else if let Some(analysis) = analyses.get(bench.name.as_str()) {
                        let base_mean = analysis.baseline.mean;
                        let ci_str = if base_mean.abs() > f64::EPSILON {
                            let lo_pct = analysis.ci_lower / base_mean * 100.0;
                            let mid_pct = analysis.ci_median / base_mean * 100.0;
                            let hi_pct = analysis.ci_upper / base_mean * 100.0;
                            format!("[{:+.1}%  {:+.1}%  {:+.1}%]", lo_pct, mid_pct, hi_pct)
                        } else {
                            format!("{:+.1}%", analysis.pct_change)
                        };
                        if res_flag {
                            format!("{ci_str} \u{26a0}")
                        } else {
                            ci_str
                        }
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                };

                let tp_str = if has_throughput {
                    comp.throughput
                        .as_ref()
                        .map(|t| t.format(bench.summary.mean, comp.throughput_unit.as_deref()))
                        .unwrap_or_default()
                } else {
                    String::new()
                };

                if has_throughput && has_comparisons {
                    out.push_str(&format!(
                        "| {} | {} | {} | {} | {} |\n",
                        bench.name, min_str, mean_str, vs_base, tp_str,
                    ));
                } else if has_throughput {
                    out.push_str(&format!(
                        "| {} | {} | {} | {} |\n",
                        bench.name, min_str, mean_str, tp_str,
                    ));
                } else if has_comparisons {
                    out.push_str(&format!(
                        "| {} | {} | {} | {} |\n",
                        bench.name, min_str, mean_str, vs_base,
                    ));
                } else {
                    out.push_str(&format!(
                        "| {} | {} | {} |\n",
                        bench.name, min_str, mean_str,
                    ));
                }
            }

            // Resolution warning
            let any_res_limited = comp.analyses.iter().any(|(_, _, a)| a.resolution_limited);
            if any_res_limited {
                out.push_str(
                    "\n> \u{26a0} Some comparisons are below timer resolution \
                     and cannot be distinguished by this hardware.\n",
                );
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
            out.push_str("| Benchmark | Min | Mean |\n");
            out.push_str("|-----------|-----|------|\n");
            for bench in &self.standalones {
                out.push_str(&format!(
                    "| {} | {} | {} |\n",
                    bench.name,
                    format_ns(bench.summary.min),
                    format_ns(bench.summary.mean),
                ));
            }
        }

        out
    }

    /// Generate CSV output with one row per benchmark.
    pub fn to_csv(&self) -> String {
        let mut out = String::new();

        // Header
        let mut header = "group,benchmark,subgroup,mean_ns,std_dev_ns,median_ns,mad_ns,min_ns,max_ns,n,cv,\
             cold_start_ns,cpu_mean_ns,cpu_efficiency,throughput_value,throughput_unit,\
             vs_base_pct,vs_base_ci_lo_pct,vs_base_ci_hi_pct,significant,resolution_limited,cohens_d,wilcoxon_p,drift_r,timer_ticks_per_sample"
            .to_string();
        #[cfg(feature = "alloc-profiling")]
        header.push_str(",allocs_per_iter,deallocs_per_iter,reallocs_per_iter,bytes_per_iter");
        header.push('\n');
        out.push_str(&header);

        // Comparison groups
        for comp in &self.comparisons {
            // Find baseline name
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

            // Build analysis lookup: candidate name -> analysis (only baseline pairs)
            let analyses: std::collections::HashMap<&str, &PairedAnalysis> = comp
                .analyses
                .iter()
                .filter(|(base, _, _)| base == baseline_name)
                .map(|(_, cand, a)| (cand.as_str(), a))
                .collect();

            for bench in &comp.benchmarks {
                let (tp_val, tp_unit) = comp
                    .throughput
                    .as_ref()
                    .map(|t| {
                        let (v, u) = t.compute(bench.summary.mean, comp.throughput_unit.as_deref());
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

                let cold = if bench.cold_start_ns > 0.0 {
                    format!("{:.2}", bench.cold_start_ns)
                } else {
                    String::new()
                };

                let subgroup = bench
                    .subgroup
                    .as_deref()
                    .map(csv_escape)
                    .unwrap_or_default();

                // vs-base columns
                let (
                    vs_pct,
                    vs_ci_lo,
                    vs_ci_hi,
                    significant,
                    res_limited,
                    cohens_d,
                    wilcoxon_p,
                    drift_r,
                ) = if bench.name == baseline_name {
                    // baseline row: no comparison values
                    (
                        String::new(),
                        String::new(),
                        String::new(),
                        String::new(),
                        String::new(),
                        String::new(),
                        String::new(),
                        String::new(),
                    )
                } else if let Some(analysis) = analyses.get(bench.name.as_str()) {
                    let base_mean = analysis.baseline.mean;
                    let (ci_lo, ci_hi) = if base_mean.abs() > f64::EPSILON {
                        (
                            format!("{:.4}", analysis.ci_lower / base_mean * 100.0),
                            format!("{:.4}", analysis.ci_upper / base_mean * 100.0),
                        )
                    } else {
                        (String::new(), String::new())
                    };
                    (
                        format!("{:.4}", analysis.pct_change),
                        ci_lo,
                        ci_hi,
                        format!("{}", analysis.significant),
                        format!("{}", analysis.resolution_limited),
                        format!("{:.4}", analysis.cohens_d),
                        format!("{:.6}", analysis.wilcoxon_p),
                        format!("{:.4}", analysis.drift_correlation),
                    )
                } else {
                    (
                        String::new(),
                        String::new(),
                        String::new(),
                        String::new(),
                        String::new(),
                        String::new(),
                        String::new(),
                        String::new(),
                    )
                };

                out.push_str(&format!(
                    "{},{},{},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{},{:.4},{},{},{},{},{},{},{},{},{},{},{},{},{},{:.0}",
                    csv_escape(&comp.group_name),
                    csv_escape(&bench.name),
                    subgroup,
                    bench.summary.mean,
                    bench.summary.std_dev(),
                    bench.summary.median,
                    bench.summary.mad,
                    bench.summary.min,
                    bench.summary.max,
                    bench.summary.n,
                    bench.summary.cv(),
                    cold,
                    cpu_mean,
                    cpu_eff,
                    tp_val,
                    tp_unit,
                    vs_pct,
                    vs_ci_lo,
                    vs_ci_hi,
                    significant,
                    res_limited,
                    cohens_d,
                    wilcoxon_p,
                    drift_r,
                    bench.timer_ticks_per_sample,
                ));
                #[cfg(feature = "alloc-profiling")]
                if let Some(alloc) = &bench.alloc_stats {
                    out.push_str(&format!(
                        ",{:.1},{:.1},{:.1},{:.0}",
                        alloc.allocs_per_iter,
                        alloc.deallocs_per_iter,
                        alloc.reallocs_per_iter,
                        alloc.bytes_per_iter,
                    ));
                } else {
                    #[cfg(feature = "alloc-profiling")]
                    out.push_str(",,,,");
                }
                out.push('\n');
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

            let cold = if bench.cold_start_ns > 0.0 {
                format!("{:.2}", bench.cold_start_ns)
            } else {
                String::new()
            };

            let subgroup = bench
                .subgroup
                .as_deref()
                .map(csv_escape)
                .unwrap_or_default();

            // group is empty for standalones; comparison columns empty
            out.push_str(&format!(
                ",{},{},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{},{:.4},{},{},{},,,,,,,,,,,{:.0}\n",
                csv_escape(&bench.name),
                subgroup,
                bench.summary.mean,
                bench.summary.std_dev(),
                bench.summary.median,
                bench.summary.mad,
                bench.summary.min,
                bench.summary.max,
                bench.summary.n,
                bench.summary.cv(),
                cold,
                cpu_mean,
                cpu_eff,
                bench.timer_ticks_per_sample,
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
        let (val, unit) = tp.compute(1_000_000.0, None); // 1ms in ns
        assert_eq!(unit, "MiB/s");
        assert!((val - 1000.0).abs() < 0.1);
    }

    #[test]
    fn throughput_bytes_gibs() {
        let tp = Throughput::Bytes(1_073_741_824); // 1 GiB
        // 1 GiB in 1ms = 1000 GiB/s
        let (val, unit) = tp.compute(1_000_000.0, None); // 1ms in ns
        assert_eq!(unit, "GiB/s");
        assert!((val - 1000.0).abs() < 0.1);
    }

    #[test]
    fn throughput_elements() {
        let tp = Throughput::Elements(1000);
        // 1000 elements in 1ms = 1M ops/s
        let (val, unit) = tp.compute(1_000_000.0, None);
        assert_eq!(unit, "Mops/s");
        assert!((val - 1.0).abs() < 0.001);
    }

    #[test]
    fn throughput_format() {
        let tp = Throughput::Bytes(153 * 1024 * 1024); // 153 MiB
        // 153 MiB in 531ms = 288 MiB/s
        let s = tp.format(531_000_000.0, None);
        assert!(s.contains("MiB/s"), "Expected MiB/s, got: {s}");
        assert!(s.contains("288"), "Expected ~288, got: {s}");
    }

    #[test]
    fn benchmark_result_tags() {
        let br = BenchmarkResult {
            name: "test".to_string(),
            summary: make_summary(100.0),
            tags: vec![
                ("library".to_string(), "zenflate".to_string()),
                ("level".to_string(), "L6".to_string()),
            ],
            ..Default::default()
        };
        assert_eq!(br.tag("library"), Some("zenflate"));
        assert_eq!(br.tag("level"), Some("L6"));
        assert_eq!(br.tag("missing"), None);
    }

    fn make_suite_result() -> SuiteResult {
        SuiteResult {
            run_id: RunId("test-123".to_string()),
            git_hash: Some("abc123".to_string()),
            comparisons: vec![ComparisonResult {
                group_name: "compress".to_string(),
                benchmarks: vec![
                    BenchmarkResult {
                        name: "zenflate".to_string(),
                        summary: make_summary(5_000_000.0),
                        tags: vec![("library".to_string(), "zenflate".to_string())],
                        ..Default::default()
                    },
                    BenchmarkResult {
                        name: "libdeflate".to_string(),
                        summary: make_summary(10_000_000.0),
                        tags: vec![("library".to_string(), "libdeflate".to_string())],
                        ..Default::default()
                    },
                ],
                completed_rounds: 100,
                throughput: Some(Throughput::Bytes(1_048_576)),
                iterations_per_sample: 1000,
                ..Default::default()
            }],
            total_time: Duration::from_secs(5),
            timer_resolution_ns: 25,
            ..Default::default()
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

    /// A suite with a real PairedAnalysis so vs_base fields are populated.
    fn make_suite_result_with_analyses() -> SuiteResult {
        use crate::stats::{PairedAnalysis, Summary};

        // Build a PairedAnalysis from real samples so ci_median is populated.
        let base_samples: Vec<f64> = (0..100).map(|_| 5_000_000.0_f64).collect();
        let cand_samples: Vec<f64> = (0..100).map(|_| 10_000_000.0_f64).collect();
        let iters: Vec<usize> = vec![1usize; 100];
        let analysis = PairedAnalysis::compute(&base_samples, &cand_samples, &iters)
            .expect("analysis should succeed for equal-length inputs");

        SuiteResult {
            run_id: RunId("test-456".to_string()),
            comparisons: vec![ComparisonResult {
                group_name: "compress".to_string(),
                benchmarks: vec![
                    BenchmarkResult {
                        name: "zenflate".to_string(),
                        summary: make_summary(5_000_000.0),
                        cold_start_ns: 12_500.0,
                        ..Default::default()
                    },
                    BenchmarkResult {
                        name: "libdeflate".to_string(),
                        summary: make_summary(10_000_000.0),
                        ..Default::default()
                    },
                ],
                analyses: vec![("zenflate".to_string(), "libdeflate".to_string(), analysis)],
                completed_rounds: 50,
                iterations_per_sample: 10,
                ..Default::default()
            }],
            standalones: vec![BenchmarkResult {
                name: "standalone_bench".to_string(),
                summary: Summary::from_slice(&[1_000.0, 1_100.0, 900.0, 1_050.0]),
                cold_start_ns: 5_000.0,
                ..Default::default()
            }],
            total_time: Duration::from_secs(3),
            gate_waits: 2,
            gate_wait_time: Duration::from_millis(250),
            timer_resolution_ns: 25,
            ..Default::default()
        }
    }

    // --- to_llm() ---

    #[test]
    fn llm_output_baseline_has_vs_base_eq_baseline() {
        let result = make_suite_result_with_analyses();
        let llm = result.to_llm();
        // The baseline row must say vs_base=baseline
        let baseline_line = llm
            .lines()
            .find(|l| l.contains("benchmark=zenflate"))
            .expect("should have a zenflate line");
        assert!(
            baseline_line.contains("vs_base=baseline"),
            "baseline row should contain vs_base=baseline, got: {baseline_line}"
        );
    }

    #[test]
    fn llm_output_candidate_has_vs_base_pct() {
        let result = make_suite_result_with_analyses();
        let llm = result.to_llm();
        let cand_line = llm
            .lines()
            .find(|l| l.contains("benchmark=libdeflate"))
            .expect("should have a libdeflate line");
        assert!(
            cand_line.contains("vs_base_pct="),
            "candidate row should contain vs_base_pct=, got: {cand_line}"
        );
    }

    #[test]
    fn llm_output_has_group_field() {
        let result = make_suite_result_with_analyses();
        let llm = result.to_llm();
        assert!(
            llm.contains("group=compress"),
            "llm output should contain group=compress, got:\n{llm}"
        );
    }

    #[test]
    fn llm_output_has_benchmark_field() {
        let result = make_suite_result_with_analyses();
        let llm = result.to_llm();
        assert!(
            llm.contains("benchmark=zenflate"),
            "llm output should contain benchmark=zenflate, got:\n{llm}"
        );
    }

    #[test]
    fn llm_output_has_section_separators() {
        let result = make_suite_result_with_analyses();
        let llm = result.to_llm();
        for line in llm.lines() {
            assert!(
                line.contains("  |  "),
                "every line should have '  |  ' section separators, got: {line}"
            );
        }
    }

    #[test]
    fn llm_output_cold_start_field_present_when_nonzero() {
        let result = make_suite_result_with_analyses();
        let llm = result.to_llm();
        // zenflate has cold_start_ns=12500, should show up as cold=...
        let baseline_line = llm
            .lines()
            .find(|l| l.contains("benchmark=zenflate"))
            .expect("should have a zenflate line");
        assert!(
            baseline_line.contains("cold="),
            "row with nonzero cold_start_ns should have cold= field, got: {baseline_line}"
        );
    }

    #[test]
    fn llm_output_standalone_cold_start_present() {
        let result = make_suite_result_with_analyses();
        let llm = result.to_llm();
        let standalone_line = llm
            .lines()
            .find(|l| l.contains("benchmark=standalone_bench"))
            .expect("should have standalone_bench line");
        assert!(
            standalone_line.contains("cold="),
            "standalone with nonzero cold_start_ns should show cold=, got: {standalone_line}"
        );
    }

    #[test]
    fn llm_output_throughput_absent_when_not_set() {
        let result = make_suite_result_with_analyses();
        let llm = result.to_llm();
        // make_suite_result_with_analyses has no throughput
        assert!(
            !llm.contains("throughput="),
            "no throughput should produce no throughput= field, got:\n{llm}"
        );
    }

    // --- to_markdown() ---

    #[test]
    fn markdown_output_has_min_and_mean_headers() {
        let result = make_suite_result();
        let md = result.to_markdown();
        assert!(
            md.contains("| Min |"),
            "markdown should have '| Min |' column header, got:\n{md}"
        );
        assert!(
            md.contains("| Mean |"),
            "markdown should have '| Mean |' column header, got:\n{md}"
        );
    }

    #[test]
    fn markdown_output_has_vs_base_column_when_comparisons_exist() {
        let result = make_suite_result_with_analyses();
        let md = result.to_markdown();
        assert!(
            md.contains("| vs Base |"),
            "markdown should have '| vs Base |' column when analyses present, got:\n{md}"
        );
    }

    #[test]
    fn markdown_output_methodology_line_has_rounds_cross() {
        let result = make_suite_result_with_analyses();
        let md = result.to_markdown();
        // Methodology line uses × (U+00D7) between rounds count and calls
        assert!(
            md.contains('\u{d7}'),
            "methodology line should contain × (rounds × calls), got:\n{md}"
        );
        assert!(
            md.contains("rounds"),
            "methodology line should mention 'rounds', got:\n{md}"
        );
    }

    // --- to_csv() ---

    #[test]
    fn csv_header_contains_cold_start_ns_column() {
        let result = make_suite_result();
        let csv = result.to_csv();
        let header = csv.lines().next().expect("csv should have a header line");
        assert!(
            header.contains("cold_start_ns"),
            "csv header should contain 'cold_start_ns', got: {header}"
        );
    }

    #[test]
    fn csv_header_contains_vs_base_pct_column() {
        let result = make_suite_result();
        let csv = result.to_csv();
        let header = csv.lines().next().expect("csv should have a header line");
        assert!(
            header.contains("vs_base_pct"),
            "csv header should contain 'vs_base_pct', got: {header}"
        );
    }

    #[test]
    fn csv_header_contains_significant_column() {
        let result = make_suite_result();
        let csv = result.to_csv();
        let header = csv.lines().next().expect("csv should have a header line");
        assert!(
            header.contains("significant"),
            "csv header should contain 'significant', got: {header}"
        );
    }

    #[test]
    fn csv_candidate_row_has_vs_base_pct_value() {
        let result = make_suite_result_with_analyses();
        let csv = result.to_csv();
        let lines: Vec<&str> = csv.lines().collect();
        let cand_row = lines
            .iter()
            .find(|l| l.contains("libdeflate"))
            .expect("should have libdeflate row");
        // The vs_base_pct column should be a non-empty float for the candidate
        // Count commas to find the column position (vs_base_pct is column index 16, 0-based)
        let cols: Vec<&str> = cand_row.split(',').collect();
        // Header: group,benchmark,subgroup,mean_ns,std_dev_ns,median_ns,mad_ns,min_ns,max_ns,
        //         n,cv,cold_start_ns,cpu_mean_ns,cpu_efficiency,throughput_value,throughput_unit,
        //         vs_base_pct,...
        let vs_base_pct_col = 16;
        assert!(
            cols.len() > vs_base_pct_col,
            "candidate row should have enough columns, got {} cols in: {cand_row}",
            cols.len()
        );
        assert!(
            !cols[vs_base_pct_col].is_empty(),
            "vs_base_pct should be non-empty for candidate, got: {cand_row}"
        );
    }

    #[test]
    fn csv_baseline_row_has_empty_vs_base_pct() {
        let result = make_suite_result_with_analyses();
        let csv = result.to_csv();
        let lines: Vec<&str> = csv.lines().collect();
        let base_row = lines
            .iter()
            .find(|l| l.contains("zenflate"))
            .expect("should have zenflate row");
        let cols: Vec<&str> = base_row.split(',').collect();
        let vs_base_pct_col = 16;
        assert!(
            cols.len() > vs_base_pct_col,
            "baseline row should have enough columns, got {} cols in: {base_row}",
            cols.len()
        );
        assert!(
            cols[vs_base_pct_col].is_empty(),
            "vs_base_pct should be empty for baseline row, got: {base_row}"
        );
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
