use crate::bench::Throughput;
use crate::format::{format_ns, format_ns_range, ns_unit, terminal_width};
use crate::results::{BenchmarkResult, ComparisonResult, SuiteResult};
use std::io::IsTerminal;

/// Report display style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportStyle {
    /// Box-drawing tables with borders. Detailed, wider.
    Table,
    /// Tree-style with ├─/╰─ nesting. Compact, divan-like.
    Tree,
}

/// Should this stream use ANSI color codes?
/// Respects NO_COLOR (https://no-color.org/), TERM=dumb, and TTY detection.
/// Whether stderr is an interactive terminal (supports \r cursor control).
pub fn stderr_is_tty() -> bool {
    std::io::stderr().is_terminal()
}

/// Print a transient status message on stderr. On a TTY, uses \r to
/// overwrite the previous line. When piped, suppresses entirely.
pub fn status(msg: &str) {
    if stderr_is_tty() {
        eprint!("\r\x1b[K{msg}");
    }
}

/// Clear any transient status line. No-op when piped.
pub fn clear_status() {
    if stderr_is_tty() {
        eprint!("\r\x1b[K");
    }
}

pub(crate) fn should_color(stream: &dyn IsTerminal) -> bool {
    if std::env::var("NO_COLOR").is_ok() {
        return false;
    }
    if std::env::var("TERM").as_deref() == Ok("dumb") {
        return false;
    }
    stream.is_terminal()
}

/// ANSI code if color enabled, empty string if not.
const fn pick(code: &'static str, color: bool) -> &'static str {
    if color { code } else { "" }
}

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

struct StandaloneRow {
    name: String,
    mean_range: String,
    n: String,
    cpu: String,
}

/// Print a human-readable report to stderr (with ANSI colors).
#[allow(non_snake_case)]
/// Print the report header. Call once before streaming groups.
pub fn print_header(run_id: &crate::results::RunId, git_hash: Option<&str>, ci: Option<&str>) {
    let c = should_color(&std::io::stderr());
    let RESET = pick("\x1b[0m", c);
    let DIM = pick("\x1b[2m", c);
    let BOLD_WHITE = pick("\x1b[1;37m", c);
    eprintln!();
    eprintln!("{BOLD_WHITE}═══════════════════════════════════════════════════════════════{RESET}");
    eprintln!("{BOLD_WHITE}  zenbench{RESET}  {DIM}{run_id}{RESET}");
    if let Some(h) = git_hash {
        eprintln!("  {DIM}git:{RESET} {h}");
    }
    if let Some(c) = ci {
        eprintln!("  {DIM}ci:{RESET}  {c}");
    }
    eprintln!("{BOLD_WHITE}═══════════════════════════════════════════════════════════════{RESET}");
}

/// Print report footer. Call once after all groups.
#[allow(non_snake_case)]
pub fn print_footer(
    total_time: std::time::Duration,
    gate_waits: usize,
    gate_wait_time: std::time::Duration,
    unreliable: bool,
) {
    let c = should_color(&std::io::stderr());
    let RESET = pick("\x1b[0m", c);
    let BOLD = pick("\x1b[1m", c);
    let DIM = pick("\x1b[2m", c);
    let RED = pick("\x1b[31m", c);
    let YELLOW = pick("\x1b[33m", c);
    let BOLD_WHITE = pick("\x1b[1;37m", c);
    eprintln!();
    eprintln!(
        "  {DIM}total: {:.1}s  gate waits: {} ({:.1}s){RESET}",
        total_time.as_secs_f64(), gate_waits, gate_wait_time.as_secs_f64(),
    );
    let gate_pct = if total_time.as_secs_f64() > 0.0 {
        gate_wait_time.as_secs_f64() / total_time.as_secs_f64() * 100.0
    } else {
        0.0
    };
    if gate_pct > 50.0 {
        eprintln!(
            "  {YELLOW}\u{26a0} {gate_pct:.0}% of time spent waiting for quiet system \
             \u{2014} results may be unreliable. \
             Try: GateConfig::disabled() or a quieter machine.{RESET}",
        );
    }
    if unreliable {
        eprintln!("  {RED}{BOLD}\u{26a0} UNRELIABLE: too many resource gate waits{RESET}");
    }
    eprintln!("{BOLD_WHITE}═══════════════════════════════════════════════════════════════{RESET}");
    eprintln!(
        "  {DIM}filter: cargo bench -- --group=NAME  format: --format=llm|csv|md|json{RESET}",
    );
}

/// Print a single group's report. Dispatches to table or tree style.
pub fn print_group(comp: &ComparisonResult, _timer_res: u64) {
    print_group_styled(comp, _timer_res, detect_style());
}

/// Print a single group in the specified style.
pub fn print_group_styled(comp: &ComparisonResult, timer_res: u64, style: ReportStyle) {
    match style {
        ReportStyle::Table => {
            let wrapper = SuiteResult {
                comparisons: vec![comp.clone()],
                timer_resolution_ns: timer_res,
                ..Default::default()
            };
            print_report_body(&wrapper);
        }
        ReportStyle::Tree => {
            print_group_tree(comp, timer_res);
        }
    }
}

/// Detect style from --style=tree|table arg or ZENBENCH_STYLE env var.
pub fn detect_style() -> ReportStyle {
    if std::env::var("ZENBENCH_STYLE").is_ok_and(|s| s == "table")
        || std::env::args().any(|a| a == "--style=table")
    {
        return ReportStyle::Table;
    }
    ReportStyle::Tree // default: tree
}

/// Print all comparison groups and standalones (no header/footer).
#[allow(non_snake_case)]
fn print_report_body(result: &SuiteResult) {
    let c = should_color(&std::io::stderr());
    let RESET = pick("\x1b[0m", c);
    let BOLD = pick("\x1b[1m", c);
    let DIM = pick("\x1b[2m", c);
    let GREEN = pick("\x1b[32m", c);
    let RED = pick("\x1b[31m", c);
    let YELLOW = pick("\x1b[33m", c);
    let CYAN = pick("\x1b[36m", c);

    for comp in &result.comparisons {
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
        let mut meta = format!("{} rounds \u{d7} {calls_str}", comp.completed_rounds);
        if comp.cold_start {
            meta.push_str(", cold start, clear-L2");
        } else if comp.cache_firewall {
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
        if comp.completed_rounds < 10 {
            meta.push_str(&format!(
                " \x1b[33m⚠ only {} rounds\x1b[0m",
                comp.completed_rounds,
            ));
        }
        let header_text = format!("{} ", comp.group_name);
        let separator_len = 63usize.saturating_sub(header_text.len() + 2);
        eprintln!(
            "  {BOLD}{header_text}{RESET}{DIM}{}{RESET}",
            "\u{2500}".repeat(separator_len),
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

        // Build analysis lookup: candidate name -> analysis (only baseline pairs)
        let baseline_analyses: std::collections::HashMap<&str, &crate::stats::PairedAnalysis> =
            comp.analyses
                .iter()
                .filter(|(base, _, _)| base == baseline_name)
                .map(|(_, cand, analysis)| (cand.as_str(), analysis))
                .collect();

        // Compute column widths — cap name to terminal_width/3 to prevent wrapping
        let term_w = terminal_width().unwrap_or(80);
        let max_name = term_w / 3;
        let name_w = comp
            .benchmarks
            .iter()
            .map(|b| b.name.len())
            .max()
            .unwrap_or(4)
            .max(9) // "benchmark" header
            .min(max_name);

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
                    "drift r={:.2} \u{2014} {direction}",
                    analysis.drift_correlation,
                ));
                comparison_markers
                    .entry((base.as_str(), cand.as_str()))
                    .or_default()
                    .push_str(&format!("[{n}]"));
            }
        }

        // Group-level round count warning is shown on the methodology line above.
        // No orphan footnote needed — the ⚠ marker on the meta line is clearer.

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
        let mut raw_rows: Vec<RawRow> = Vec::with_capacity(display_indices.len());
        for &i in &display_indices {
            let bench = &comp.benchmarks[i];
            let is_fastest = (bench.summary.mean - fastest_mean).abs() < f64::EPSILON
                && comp.benchmarks.len() > 1;
            // Throughput: compact "4.91G" format — value + SI prefix
            let tp_str = comp
                .throughput
                .as_ref()
                .map(|tp| {
                    let (val, unit) = tp.compute(bench.summary.mean, tp_unit);
                    // Extract the prefix (G/M/K or empty) from unit like "Gchecks/s"
                    let prefix = if unit.starts_with('G') {
                        "G"
                    } else if unit.starts_with('M') {
                        "M"
                    } else if unit.starts_with('K') {
                        "K"
                    } else {
                        ""
                    };
                    if val >= 100.0 {
                        format!("{val:.0}{prefix}")
                    } else if val >= 10.0 {
                        format!("{val:.1}{prefix}")
                    } else {
                        format!("{val:.2}{prefix}")
                    }
                })
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
            // MAD (median absolute deviation, scaled) instead of stddev.
            // Robust to outliers -- one 10x spike from a context switch
            // doesn't destroy it like it does stddev.
            let sigma_str = format!("{:.*}", mean_dp, bench.summary.mad / mean_divisor);

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
                    let mid_pct = analysis.ci_median / base_mean * 100.0;
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
                        "CI [{ci_lo_pct} .. {ci_hi_pct}] crosses zero \u{2014} \
                         cannot confirm a difference",
                    ));
                    markers.push_str(&format!("[{n}]"));
                }
                if analysis.significant && analysis.cohens_d.abs() < 0.2 {
                    let n = add_footnote(format!(
                        "real but tiny (effect {:.2}) \u{2014} unlikely to matter",
                        analysis.cohens_d,
                    ));
                    markers.push_str(&format!("[{n}]"));
                }
                // Drift marker
                if let Some(dm) = comparison_markers.get(&(baseline_name, bench.name.as_str())) {
                    markers.push_str(dm);
                }
            }
            let cv = bench.summary.cv();
            if cv > 0.20 && bench.summary.n > 10 {
                let n = add_footnote(format!(
                    "CV={:.0}% \u{2014} noisy, try a quieter system",
                    cv * 100.0,
                ));
                markers.push_str(&format!("[{n}]"));
            }
            if bench.summary.mean < 1.0
                && bench.summary.n > 0
                && (cv < 0.01 || bench.summary.variance < f64::EPSILON)
            {
                let n = add_footnote(
                    "sub-ns with near-zero variance \u{2014} likely optimized away".to_string(),
                );
                markers.push_str(&format!("[{n}]"));
            }
            // Timer resolution check — only flags when the SAMPLE time
            // (mean × iterations) is near timer resolution, not the per-iter time.
            // A 0.2ns per-iter with 10M iters = 2s per sample, which is fine.
            let timer_res = result.timer_resolution_ns as f64;
            let sample_time = bench.summary.mean * comp.iterations_per_sample as f64;
            if timer_res > 0.0 && sample_time > 0.0 && sample_time < timer_res * 100.0 {
                let n = add_footnote(format!(
                    "sample time ({}) near timer resolution ({:.0}ns) \u{2014} increase iterations",
                    crate::format::format_ns(sample_time),
                    timer_res,
                ));
                markers.push_str(&format!("[{n}]"));
            }
            // Cold start below timer resolution
            if bench.cold_start_ns > 0.0 && bench.cold_start_ns < timer_res * 5.0 {
                let n = add_footnote(format!(
                    "cold start ({:.0}ns) near timer resolution \u{2014} unreliable",
                    bench.cold_start_ns,
                ));
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
        // Show sigma column when there's room (no throughput), or always if no comparisons
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
            let sigma_col = format!("\u{b1}{:>sigma_val_w$}{mean_unit}", raw.sigma_str);
            let vs_base = if !raw.vs_vals[0].is_empty() {
                format!(
                    "[{:>vs_val_w$} {:>vs_val_w$} {:>vs_val_w$}]",
                    raw.vs_vals[0], raw.vs_vals[1], raw.vs_vals[2],
                )
            } else {
                String::new()
            };
            // Truncate name if it exceeds the column width
            let display_name = if bench.name.len() > name_w {
                format!("{}…", &bench.name[..name_w - 1])
            } else {
                bench.name.clone()
            };
            rows.push(Row {
                name: display_name,
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
        // Throughput header: strip the SI prefix to get base unit (e.g., "checks/s")
        let tp_unit_str = comp
            .throughput
            .as_ref()
            .map(|tp| {
                let ref_mean = comp
                    .benchmarks
                    .iter()
                    .find(|b| b.name == baseline_name)
                    .unwrap_or(&comp.benchmarks[0])
                    .summary
                    .mean;
                let (_, unit) = tp.compute(ref_mean, tp_unit);
                // Strip SI prefix: "Gchecks/s" → "checks/s", "MiB/s" → "iB/s"
                let base = unit.trim_start_matches(['G', 'M', 'K', 'T']);
                if base.is_empty() { unit } else { base.to_string() }
            })
            .unwrap_or_default();
        let tp_w = if has_throughput {
            let val_w = rows
                .iter()
                .map(|r| r.throughput.len())
                .max()
                .unwrap_or(4)
                .max(4);
            val_w.max(tp_unit_str.len())
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
                .max(20) // header: [p5 . mean . p95] vs base
        } else {
            0
        };

        // Adaptive column dropping: estimate total width and drop min column
        // if the table would exceed terminal width.
        let col_overhead = 3; // " │ " between each column
        let estimated_width = 2 // indent
            + name_w + col_overhead
            + min_w + col_overhead
            + mean_w + col_overhead
            + if show_sigma { sigma_w + col_overhead } else { 0 }
            + if has_comparisons { vs_w + col_overhead } else { 0 }
            + if has_throughput { tp_w + col_overhead } else { 0 }
            + if has_cpu { cpu_w + col_overhead } else { 0 }
            + 2; // closing " │"
        let show_min = estimated_width <= term_w;

        // Helper to build a table line with consistent column structure
        let add_col = |line: &mut String, width: usize, corner: char| {
            line.push_str(&format!("{corner}{}", "\u{2500}".repeat(width + 2)));
        };

        // Build table borders and header
        let mut top = String::from("  ");
        let mut hdr = String::from("  ");
        let mut mid = String::from("  ");

        // Name column
        add_col(&mut top, name_w, '\u{250c}');
        hdr.push_str(&format!("\u{2502} {:<name_w$}", "benchmark"));
        add_col(&mut mid, name_w, '\u{251c}');

        // Min column (dropped if table too wide for terminal)
        if show_min {
            add_col(&mut top, min_w, '\u{252c}');
            hdr.push_str(&format!(" \u{2502} {:>min_w$}", "min"));
            add_col(&mut mid, min_w, '\u{253c}');
        }

        // Mean column
        add_col(&mut top, mean_w, '\u{252c}');
        hdr.push_str(&format!(" \u{2502} {:>mean_w$}", "mean"));
        add_col(&mut mid, mean_w, '\u{253c}');

        // sigma column
        if show_sigma {
            add_col(&mut top, sigma_w, '\u{252c}');
            hdr.push_str(&format!(" \u{2502} {:>sigma_w$}", "mad"));
            add_col(&mut mid, sigma_w, '\u{253c}');
        }

        // vs base column
        if has_comparisons {
            add_col(&mut top, vs_w, '\u{252c}');
            hdr.push_str(&format!(" \u{2502} {:>vs_w$}", "95% CI vs base"));
            add_col(&mut mid, vs_w, '\u{253c}');
        }

        if has_throughput {
            add_col(&mut top, tp_w, '\u{252c}');
            hdr.push_str(&format!(" \u{2502} {:>tp_w$}", tp_unit_str));
            add_col(&mut mid, tp_w, '\u{253c}');
        }
        if has_cpu {
            add_col(&mut top, cpu_w, '\u{252c}');
            hdr.push_str(&format!(" \u{2502} {:>cpu_w$}", "cpu"));
            add_col(&mut mid, cpu_w, '\u{253c}');
        }
        top.push('\u{2510}');
        hdr.push_str(" \u{2502}");
        mid.push('\u{2524}');

        eprintln!("{DIM}{top}{RESET}");
        eprintln!("{DIM}{hdr}{RESET}");
        eprintln!("{DIM}{mid}{RESET}");

        let has_subgroups = rows.iter().any(|r| r.subgroup.is_some());
        // Inner width = top line chars minus "  ┌" prefix and "┐" suffix
        // top is like "  ┌───┬───┐" -- inner is everything between ┌ and ┐
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
                            "  {DIM}\u{251c}\u{2500} {RESET}{BOLD}{label}{RESET}{DIM} {}\u{2524}{RESET}",
                            "\u{2500}".repeat(pad),
                        );
                    }
                }
            }
            let name_color = if row.is_fastest { GREEN } else { "" };
            let name_reset = if row.is_fastest { RESET } else { "" };

            let mut line = format!(
                "  {DIM}\u{2502}{RESET} {name_color}{:<name_w$}{name_reset}",
                row.name,
            );

            if show_min {
                line.push_str(&format!(
                    " {DIM}\u{2502}{RESET} {:>min_w$}",
                    row.min_col,
                ));
            }
            line.push_str(&format!(
                " {DIM}\u{2502}{RESET} {:>mean_w$}",
                row.mean_col,
            ));
            if show_sigma {
                line.push_str(&format!(
                    " {DIM}\u{2502}{RESET} {DIM}{:>sigma_w$}{RESET}",
                    row.sigma_col,
                ));
            }

            if has_comparisons {
                let vc = row.vs_base_color;
                let vr = if vc.is_empty() { "" } else { RESET };
                line.push_str(&format!(
                    " {DIM}\u{2502}{RESET} {vc}{:>vs_w$}{vr}",
                    row.vs_base
                ));
            }

            if has_throughput {
                line.push_str(&format!(
                    " {DIM}\u{2502}{RESET} {CYAN}{:>tp_w$}{RESET}",
                    row.throughput,
                ));
            }
            if has_cpu {
                line.push_str(&format!(
                    " {DIM}\u{2502}{RESET} {DIM}{:>cpu_w$}{RESET}",
                    row.cpu
                ));
            }
            line.push_str(&format!(" {DIM}\u{2502}{RESET}"));

            if !row.markers.is_empty() {
                line.push_str(&format!(" {YELLOW}{}{RESET}", row.markers));
            }

            eprintln!("{line}");
        }

        // Bottom border
        let mut bot = String::from("  \u{2514}");
        bot.push_str(&"\u{2500}".repeat(name_w + 2));
        if show_min {
            bot.push_str(&format!("\u{2534}{}", "\u{2500}".repeat(min_w + 2)));
        }
        bot.push_str(&format!("\u{2534}{}", "\u{2500}".repeat(mean_w + 2)));
        if show_sigma {
            bot.push_str(&format!("\u{2534}{}", "\u{2500}".repeat(sigma_w + 2)));
        }
        if has_comparisons {
            bot.push_str(&format!("\u{2534}{}", "\u{2500}".repeat(vs_w + 2)));
        }
        if has_throughput {
            bot.push_str(&format!("\u{2534}{}", "\u{2500}".repeat(tp_w + 2)));
        }
        if has_cpu {
            bot.push_str(&format!("\u{2534}{}", "\u{2500}".repeat(cpu_w + 2)));
        }
        bot.push('\u{2518}');
        eprintln!("{DIM}{bot}{RESET}");

        // Terminal bar chart -- always sorted fastest-first
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
                            .map(|tp| tp.format(b.summary.mean, tp_unit))
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

            // Scale: for throughput bars, longest = highest throughput (best).
            // For time bars, longest = highest time (worst — fastest highlighted green).
            // When throughput is set, invert: bar ∝ 1/mean (higher throughput = longer bar).
            let max_mean = comp
                .benchmarks
                .iter()
                .map(|b| b.summary.mean)
                .fold(0.0_f64, f64::max);
            let min_mean = comp
                .benchmarks
                .iter()
                .map(|b| b.summary.mean)
                .fold(f64::INFINITY, f64::min);

            if max_mean > 0.0 {
                eprintln!();
                for (idx, &bench_i) in bar_indices.iter().enumerate() {
                    let bench = &comp.benchmarks[bench_i];
                    let is_fastest = (bench.summary.mean - fastest_mean).abs() < f64::EPSILON;

                    // Truncate name to fit name_w (same as table)
                    let display_name = if bench.name.len() > name_w {
                        format!("{}…", &bench.name[..name_w - 1])
                    } else {
                        bench.name.clone()
                    };

                    // Throughput mode: longest bar = highest throughput = lowest mean time.
                    // Time mode: longest bar = highest time = slowest benchmark.
                    let frac = if has_throughput && min_mean > 0.0 {
                        min_mean / bench.summary.mean // invert: fastest gets longest bar
                    } else {
                        bench.summary.mean / max_mean
                    };
                    let bar_len = (frac * bar_max as f64).round().max(1.0) as usize;
                    let bar: String = "\u{2588}".repeat(bar_len);

                    let name_color = if is_fastest { GREEN } else { "" };
                    let name_reset = if is_fastest { RESET } else { "" };
                    let bar_color = if is_fastest { GREEN } else { CYAN };

                    eprintln!(
                        "  {name_color}{:<name_w$}{name_reset}  {bar_color}{bar}{RESET} {DIM}{}{RESET}",
                        display_name, bar_labels[idx],
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
    if !result.standalones.is_empty() {
        eprintln!();
        eprintln!("  {BOLD}standalone{RESET}");

        let name_w = result
            .standalones
            .iter()
            .map(|b| b.name.len())
            .max()
            .unwrap_or(9)
            .max(9);

        let has_cpu = result.standalones.iter().any(|b| b.cpu_summary.is_some());

        // Pre-format
        let rows: Vec<StandaloneRow> = result
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

        let mut top = format!("  \u{250c}{}", "\u{2500}".repeat(name_w + 2));
        let mut mid = format!("  \u{251c}{}", "\u{2500}".repeat(name_w + 2));
        let mut hdr = format!("  \u{2502} {:<name_w$}", "benchmark");
        top.push_str(&format!("\u{252c}{}", "\u{2500}".repeat(mean_w + 2)));
        hdr.push_str(&format!(" \u{2502} {:^mean_w$}", "lo  mean  hi"));
        mid.push_str(&format!("\u{253c}{}", "\u{2500}".repeat(mean_w + 2)));
        top.push_str(&format!("\u{252c}{}", "\u{2500}".repeat(n_w + 2)));
        hdr.push_str(&format!(" \u{2502} {:>n_w$}", "n"));
        mid.push_str(&format!("\u{253c}{}", "\u{2500}".repeat(n_w + 2)));
        if has_cpu {
            top.push_str(&format!("\u{252c}{}", "\u{2500}".repeat(cpu_w + 2)));
            hdr.push_str(&format!(" \u{2502} {:>cpu_w$}", "cpu"));
            mid.push_str(&format!("\u{253c}{}", "\u{2500}".repeat(cpu_w + 2)));
        }
        top.push('\u{2510}');
        hdr.push_str(" \u{2502}");
        mid.push('\u{2524}');

        eprintln!("{DIM}{top}{RESET}");
        eprintln!("{DIM}{hdr}{RESET}");
        eprintln!("{DIM}{mid}{RESET}");

        for row in &rows {
            let mut line = format!(
                "  {DIM}\u{2502}{RESET} {:<name_w$} {DIM}\u{2502}{RESET} {:>mean_w$} {DIM}\u{2502}{RESET} {:>n_w$}",
                row.name, row.mean_range, row.n,
            );
            if has_cpu {
                line.push_str(&format!(
                    " {DIM}\u{2502}{RESET} {DIM}{:>cpu_w$}{RESET}",
                    row.cpu,
                ));
            }
            line.push_str(&format!(" {DIM}\u{2502}{RESET}"));
            eprintln!("{line}");
        }

        let mut bot = format!("  \u{2514}{}", "\u{2500}".repeat(name_w + 2));
        bot.push_str(&format!("\u{2534}{}", "\u{2500}".repeat(mean_w + 2)));
        bot.push_str(&format!("\u{2534}{}", "\u{2500}".repeat(n_w + 2)));
        if has_cpu {
            bot.push_str(&format!("\u{2534}{}", "\u{2500}".repeat(cpu_w + 2)));
        }
        bot.push('\u{2518}');
        eprintln!("{DIM}{bot}{RESET}");
    }

    // Footer is now printed separately via print_footer()
}

/// Print a complete report (header + all groups + standalones + footer).
/// Used by SuiteResult::print_report() for batch mode.
pub fn print_report(result: &SuiteResult) {
    print_header(&result.run_id, result.git_hash.as_deref(), result.ci_environment.as_deref());
    print_report_body(result);
    print_footer(result.total_time, result.gate_waits, result.gate_wait_time, result.unreliable);
}

/// Print a group in tree style (compact, divan-like).
#[allow(non_snake_case)]
fn print_group_tree(comp: &ComparisonResult, _timer_res: u64) {
    let c = should_color(&std::io::stderr());
    let RESET = pick("\x1b[0m", c);
    let BOLD = pick("\x1b[1m", c);
    let DIM = pick("\x1b[2m", c);
    let GREEN = pick("\x1b[32m", c);
    let _RED = pick("\x1b[31m", c);
    let YELLOW = pick("\x1b[33m", c);
    let CYAN = pick("\x1b[36m", c);

    if comp.benchmarks.is_empty() {
        return;
    }

    let has_throughput = comp.throughput.is_some();
    let tp_unit = comp.throughput_unit.as_deref();

    // Find baseline
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

    let baseline_analyses: std::collections::HashMap<&str, &crate::stats::PairedAnalysis> = comp
        .analyses
        .iter()
        .filter(|(base, _, _)| base == baseline_name)
        .map(|(_, cand, analysis)| (cand.as_str(), analysis))
        .collect();

    let fastest_mean = comp
        .benchmarks
        .iter()
        .map(|b| b.summary.mean)
        .fold(f64::INFINITY, f64::min);

    // Compute column widths
    let term_w = terminal_width().unwrap_or(80);
    let max_name = term_w / 3;
    let name_w = comp
        .benchmarks
        .iter()
        .map(|b| b.name.len())
        .max()
        .unwrap_or(4)
        .max(4)
        .min(max_name);

    // Build display data per benchmark
    struct TreeRow {
        name: String,
        subgroup: Option<String>,
        mean_mad_str: String, // "258 ±7" — mean and noise in one column
        ci_str: String,
        tp_str: String,
        is_fastest: bool,
        markers: String,
    }

    let (mean_divisor, mean_unit, mean_dp) = ns_unit(
        comp.benchmarks
            .iter()
            .find(|b| b.name == baseline_name)
            .unwrap_or(&comp.benchmarks[0])
            .summary
            .mean
            .abs(),
    );

    let mut footnotes: Vec<String> = Vec::new();
    let mut add_footnote = |msg: String| -> usize {
        footnotes.push(msg);
        footnotes.len()
    };

    // Metadata line
    let iters = comp.iterations_per_sample;
    let iters_str = if iters >= 1_000_000 {
        format!("{}M", iters / 1_000_000)
    } else if iters >= 1000 {
        format!("{}K", iters / 1000)
    } else {
        format!("{iters}")
    };
    let mut meta = format!("{} rounds", comp.completed_rounds);
    if iters > 0 {
        meta.push_str(&format!(" × {iters_str} calls"));
    }
    if comp.completed_rounds < 10 {
        meta.push_str(&format!(" {YELLOW}⚠ only {} rounds{RESET}", comp.completed_rounds));
    }

    let mut rows: Vec<TreeRow> = Vec::new();

    for bench in &comp.benchmarks {
        let is_baseline = bench.name == baseline_name;
        let is_fastest =
            (bench.summary.mean - fastest_mean).abs() < f64::EPSILON && comp.benchmarks.len() > 1;

        let mean_val = format!("{:.*}", mean_dp, bench.summary.mean / mean_divisor);
        let mad_val = format!("{:.*}", mean_dp, bench.summary.mad / mean_divisor);
        let mean_mad_str = format!("{mean_val} ±{mad_val}");

        // CI string: compact [lo–hi] or [lo%–hi%]
        let ci_str = if is_baseline {
            if let Some(ci) = &bench.mean_ci {
                let lo = format!("{:.*}", mean_dp, ci.lower / mean_divisor);
                let hi = format!("{:.*}", mean_dp, ci.upper / mean_divisor);
                format!("[{lo}–{hi}]{mean_unit}")
            } else {
                String::new()
            }
        } else if let Some(analysis) = baseline_analyses.get(bench.name.as_str()) {
            let base_mean = analysis.baseline.mean;
            if base_mean.abs() > f64::EPSILON {
                let lo_pct = analysis.ci_lower / base_mean * 100.0;
                let hi_pct = analysis.ci_upper / base_mean * 100.0;
                format!("[{lo_pct:+.1}%–{hi_pct:+.1}%]")
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        // Throughput
        let tp_str = if has_throughput {
            comp.throughput
                .as_ref()
                .map(|tp| {
                    let (val, unit) = tp.compute(bench.summary.mean, tp_unit);
                    let prefix = if unit.starts_with('G') {
                        "G"
                    } else if unit.starts_with('M') {
                        "M"
                    } else if unit.starts_with('K') {
                        "K"
                    } else {
                        ""
                    };
                    if val >= 100.0 {
                        format!("{val:.0}{prefix}")
                    } else if val >= 10.0 {
                        format!("{val:.1}{prefix}")
                    } else {
                        format!("{val:.2}{prefix}")
                    }
                })
                .unwrap_or_default()
        } else {
            String::new()
        };

        // Footnote markers
        let mut markers = String::new();
        if let Some(analysis) = baseline_analyses.get(bench.name.as_str()) {
            if !analysis.significant {
                let n = add_footnote("CI crosses zero".to_string());
                markers.push_str(&format!(" [{n}]"));
            }
            if analysis.drift_correlation.abs() > 0.5 {
                let dir = if analysis.drift_correlation > 0.0 {
                    "slower"
                } else {
                    "faster"
                };
                let n = add_footnote(format!(
                    "drift r={:.2} — later rounds {dir}",
                    analysis.drift_correlation
                ));
                markers.push_str(&format!(" [{n}]"));
            }
        }
        let cv = bench.summary.cv();
        if cv > 0.20 {
            let n = add_footnote(format!("CV={:.0}%", cv * 100.0));
            markers.push_str(&format!(" [{n}]"));
        }

        rows.push(TreeRow {
            name: if bench.name.len() > name_w {
                format!("{}…", &bench.name[..name_w - 1])
            } else {
                bench.name.clone()
            },
            subgroup: bench.subgroup.clone(),
            mean_mad_str,
            ci_str,
            tp_str,
            is_fastest,
            markers,
        });
    }

    // Compute column widths for alignment
    let mean_val_w = rows.iter().map(|r| r.mean_mad_str.len()).max().unwrap_or(1);
    let ci_w = rows.iter().map(|r| r.ci_str.len()).max().unwrap_or(0);
    let tp_w = rows.iter().map(|r| r.tp_str.len()).max().unwrap_or(0);

    // Throughput unit for header
    let tp_header = if has_throughput {
        comp.throughput
            .as_ref()
            .map(|tp| {
                let ref_mean = comp
                    .benchmarks
                    .iter()
                    .find(|b| b.name == baseline_name)
                    .unwrap_or(&comp.benchmarks[0])
                    .summary
                    .mean;
                let (_, unit) = tp.compute(ref_mean, tp_unit);
                let base = unit.trim_start_matches(['G', 'M', 'K', 'T']);
                if base.is_empty() { unit } else { base.to_string() }
            })
            .unwrap_or_default()
    } else {
        String::new()
    };

    // Compute total left-column width: tree prefix + name
    // Max prefix is 6 chars ("│  ╰─ ") for nested items, 3 chars ("╰─ ") for flat
    let has_subgroups = rows.iter().any(|r| r.subgroup.is_some());
    let prefix_w = if has_subgroups { 6 } else { 3 }; // "├─ " or "│  ╰─ "
    let left_col_w = prefix_w + name_w;

    // Print group header
    eprintln!();
    eprintln!(
        "  {BOLD}{}{RESET}  {DIM}{meta}{RESET}",
        comp.group_name,
    );
    let mean_header = format!("mean ±mad {mean_unit}");
    let tp_hdr = if has_throughput {
        format!("  {:>tp_w$}", tp_header)
    } else {
        String::new()
    };
    eprintln!(
        "  {:left_col_w$}  {:>mean_w$}  {:<ci_w$}{tp_hdr}",
        "", mean_header, "95% CI vs base",
        mean_w = mean_val_w + mean_unit.len(),
    );

    // Group benchmarks by subgroup
    let mut current_subgroup: Option<&str> = None;
    let n_rows = rows.len();

    for (idx, row) in rows.iter().enumerate() {
        let is_last_in_group = idx + 1 == n_rows
            || rows
                .get(idx + 1)
                .map(|next| next.subgroup != row.subgroup)
                .unwrap_or(true);

        // Subgroup header
        let new_subgroup = row.subgroup.as_deref();
        if new_subgroup != current_subgroup {
            current_subgroup = new_subgroup;
            if let Some(sg) = new_subgroup {
                let is_last_subgroup = {
                    // Check if any later row has a different subgroup
                    !rows[idx + 1..].iter().any(|r| r.subgroup.as_deref() != new_subgroup)
                };
                let branch = if is_last_subgroup { "╰─" } else { "├─" };
                eprintln!("  {DIM}{branch} {sg}{RESET}");
            }
        }

        // Tree prefix
        let (prefix, branch) = if row.subgroup.is_some() {
            let parent_last = !rows[idx..]
                .iter()
                .skip(1)
                .any(|r| r.subgroup == row.subgroup);
            let is_last_subgroup_in_group = !rows[idx + 1..].iter().any(|r| r.subgroup != row.subgroup);
            let vert = if is_last_subgroup_in_group { "   " } else { "│  " };
            if parent_last || is_last_in_group {
                (vert, "╰─")
            } else {
                (vert, "├─")
            }
        } else {
            let is_last = idx + 1 == n_rows;
            ("", if is_last { "╰─" } else { "├─" })
        };

        let name_color = if row.is_fastest { GREEN } else { "" };
        let name_reset = if row.is_fastest { RESET } else { "" };

        let tp_col = if has_throughput {
            format!("  {CYAN}{:>tp_w$}{RESET}", row.tp_str)
        } else {
            String::new()
        };

        // Compute the actual prefix width for this row
        let prefix_str = format!("{prefix}{branch} ");
        let prefix_chars = prefix_str.chars().count();
        // Pad name so total left column = left_col_w
        let this_name_w = left_col_w.saturating_sub(prefix_chars);

        eprintln!(
            "  {DIM}{prefix}{branch}{RESET} {name_color}{:<this_name_w$}{name_reset}  {:>mean_w$}{DIM}{mean_unit}{RESET}  {DIM}{:<ci_w$}{RESET}{tp_col}{YELLOW}{}{RESET}",
            row.name,
            row.mean_mad_str,
            row.ci_str,
            row.markers,
            mean_w = mean_val_w,
        );
    }

    // Compact bar chart (throughput or time, sorted fastest-first)
    if rows.len() >= 2 {
        let term_w = terminal_width().unwrap_or(80);
        let mut bar_indices: Vec<usize> = (0..comp.benchmarks.len()).collect();
        bar_indices.sort_by(|&a, &b| {
            comp.benchmarks[a]
                .summary
                .mean
                .total_cmp(&comp.benchmarks[b].summary.mean)
        });

        let bar_labels: Vec<String> = if has_throughput {
            bar_indices
                .iter()
                .map(|&i| {
                    comp.throughput
                        .as_ref()
                        .map(|tp| tp.format(comp.benchmarks[i].summary.mean, tp_unit))
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
        let bar_max = if term_w > overhead + 4 {
            term_w - overhead
        } else {
            20
        };

        let max_mean = comp
            .benchmarks
            .iter()
            .map(|b| b.summary.mean)
            .fold(0.0_f64, f64::max);
        let min_mean = comp
            .benchmarks
            .iter()
            .map(|b| b.summary.mean)
            .fold(f64::INFINITY, f64::min);

        if max_mean > 0.0 {
            eprintln!();
            for (idx, &bench_i) in bar_indices.iter().enumerate() {
                let bench = &comp.benchmarks[bench_i];
                let is_fastest =
                    (bench.summary.mean - fastest_mean).abs() < f64::EPSILON;

                let display_name = if bench.name.len() > name_w {
                    format!("{}…", &bench.name[..name_w - 1])
                } else {
                    bench.name.clone()
                };

                let frac = if has_throughput && min_mean > 0.0 {
                    min_mean / bench.summary.mean
                } else {
                    bench.summary.mean / max_mean
                };
                let bar_len = (frac * bar_max as f64).round().max(1.0) as usize;
                let bar: String = "\u{2588}".repeat(bar_len);

                let name_color = if is_fastest { GREEN } else { "" };
                let name_reset = if is_fastest { RESET } else { "" };
                let bar_color = if is_fastest { GREEN } else { CYAN };

                eprintln!(
                    "  {name_color}{:<name_w$}{name_reset}  {bar_color}{bar}{RESET} {DIM}{}{RESET}",
                    display_name, bar_labels[idx],
                );
            }
        }
    }

    // Footnotes
    if !footnotes.is_empty() {
        for (i, note) in footnotes.iter().enumerate() {
            eprintln!("  {YELLOW}[{}]{RESET} {DIM}{note}{RESET}", i + 1);
        }
    }
}

/// Generate a text-based bar chart for a group of benchmarks.
///
/// Returns a fenced code block that renders as monospace in markdown.
pub(crate) fn format_bar_chart(
    benchmarks: &[BenchmarkResult],
    throughput: Option<&Throughput>,
    throughput_unit: Option<&str>,
) -> String {
    const BAR_WIDTH: usize = 30;
    let blocks = [
        '\u{2588}', '\u{2589}', '\u{258a}', '\u{258b}', '\u{258c}', '\u{258d}', '\u{258e}',
        '\u{258f}',
    ];

    if benchmarks.is_empty() {
        return String::new();
    }

    // Determine the metric to chart: throughput (higher=better) or time (lower=better)
    let (values, labels): (Vec<f64>, Vec<String>) = if let Some(tp) = throughput {
        // Chart throughput (higher is better)
        benchmarks
            .iter()
            .map(|b| {
                let (val, unit) = tp.compute(b.summary.mean, throughput_unit);
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
            bar.push('\u{2588}');
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
