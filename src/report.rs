use crate::bench::Throughput;
use crate::format::{format_ns, format_ns_range, ns_unit, terminal_width};
use crate::results::{BenchmarkResult, SuiteResult};
use std::io::IsTerminal;

/// Should this stream use ANSI color codes?
/// Respects NO_COLOR (https://no-color.org/), TERM=dumb, and TTY detection.
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
pub fn print_report(result: &SuiteResult) {
    let c = should_color(&std::io::stderr());
    let RESET = pick("\x1b[0m", c);
    let BOLD = pick("\x1b[1m", c);
    let DIM = pick("\x1b[2m", c);
    let GREEN = pick("\x1b[32m", c);
    let RED = pick("\x1b[31m", c);
    let YELLOW = pick("\x1b[33m", c);
    let CYAN = pick("\x1b[36m", c);
    let BOLD_WHITE = pick("\x1b[1;37m", c);

    eprintln!();
    eprintln!("{BOLD_WHITE}═══════════════════════════════════════════════════════════════{RESET}");
    eprintln!(
        "{BOLD_WHITE}  zenbench{RESET}  {DIM}{}{RESET}",
        result.run_id
    );
    if let Some(hash) = &result.git_hash {
        eprintln!("  {DIM}git:{RESET} {hash}");
    }
    if let Some(ci) = &result.ci_environment {
        eprintln!("  {DIM}ci:{RESET}  {ci}");
    }
    eprintln!("{BOLD_WHITE}═══════════════════════════════════════════════════════════════{RESET}");

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
            let tp_str = comp
                .throughput
                .as_ref()
                .map(|tp| tp.format(bench.summary.mean, tp_unit))
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
                .max(20) // header: [p5 . mean . p95] vs base
        } else {
            0
        };

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

        // Min column
        add_col(&mut top, min_w, '\u{252c}');
        hdr.push_str(&format!(" \u{2502} {:>min_w$}", "min"));
        add_col(&mut mid, min_w, '\u{253c}');

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
            hdr.push_str(&format!(" \u{2502} {:>tp_w$}", "throughput"));
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

            line.push_str(&format!(
                " {DIM}\u{2502}{RESET} {:>min_w$} {DIM}\u{2502}{RESET} {:>mean_w$}",
                row.min_col, row.mean_col,
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
        bot.push_str(&format!("\u{2534}{}", "\u{2500}".repeat(min_w + 2)));
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

    eprintln!();
    eprintln!(
        "  {DIM}total: {:.1}s  gate waits: {} ({:.1}s){RESET}",
        result.total_time.as_secs_f64(),
        result.gate_waits,
        result.gate_wait_time.as_secs_f64(),
    );
    // Warn if gate waits dominated the run
    let gate_pct = if result.total_time.as_secs_f64() > 0.0 {
        result.gate_wait_time.as_secs_f64() / result.total_time.as_secs_f64() * 100.0
    } else {
        0.0
    };
    if gate_pct > 50.0 {
        eprintln!(
            "  {YELLOW}\u{26a0} {:.0}% of time spent waiting for quiet system \
             \u{2014} results may be unreliable. \
             Try: GateConfig::disabled() or a quieter machine.{RESET}",
            gate_pct,
        );
    }
    if result.unreliable {
        eprintln!("  {RED}{BOLD}\u{26a0} UNRELIABLE: too many resource gate waits{RESET}");
    }
    eprintln!("{BOLD_WHITE}═══════════════════════════════════════════════════════════════{RESET}");
    // Usage hints for LLMs and humans
    eprintln!(
        "  {DIM}filter: cargo bench -- --group=NAME  \
         format: --format=llm|csv|md|json{RESET}",
    );
    eprintln!();
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
