//! HTML report generation with inline SVG charts.
//!
//! Produces a self-contained HTML file — no external dependencies,
//! no JavaScript required. SVG bar charts render natively in all browsers.

use crate::format::format_ns;
use crate::results::{ComparisonResult, SuiteResult};

/// Generate a complete HTML report as a string.
pub fn to_html(result: &SuiteResult) -> String {
    let mut html = String::with_capacity(16_000);

    html.push_str(HTML_HEAD);
    html.push_str(&format!(
        "<h1>zenbench <small>{}</small></h1>\n",
        result.run_id
    ));
    if let Some(hash) = &result.git_hash {
        html.push_str(&format!("<p class=\"meta\">git: <code>{hash}</code></p>\n"));
    }
    html.push_str(&format!(
        "<p class=\"meta\">total: {:.1}s</p>\n",
        result.total_time.as_secs_f64()
    ));

    for comp in &result.comparisons {
        html.push_str(&render_group(comp));
    }

    html.push_str("</div></body></html>");
    html
}

fn render_group(comp: &ComparisonResult) -> String {
    let mut html = String::new();
    let tp_unit = comp.throughput_unit.as_deref();

    html.push_str(&format!(
        "<details open><summary><h2>{}</h2>\
         <span class=\"meta\">{} rounds × {} calls</span></summary>\n",
        comp.group_name, comp.completed_rounds, comp.iterations_per_sample,
    ));

    // Table
    let has_throughput = comp.throughput.is_some();
    html.push_str("<table><tr><th>benchmark</th><th>mean</th><th>±mad</th>");
    if has_throughput {
        html.push_str("<th>throughput</th>");
    }
    html.push_str("</tr>\n");

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
        .map(|(_, cand, a)| (cand.as_str(), a))
        .collect();

    let fastest_mean = comp
        .benchmarks
        .iter()
        .map(|b| b.summary.mean)
        .fold(f64::INFINITY, f64::min);

    for bench in &comp.benchmarks {
        let is_fastest =
            (bench.summary.mean - fastest_mean).abs() < f64::EPSILON && comp.benchmarks.len() > 1;
        let cls = if is_fastest { " class=\"fastest\"" } else { "" };

        let vs = if let Some(a) = baseline_analyses.get(bench.name.as_str()) {
            let base = a.baseline.mean;
            if base.abs() > f64::EPSILON {
                let pct = a.pct_change;
                let color = if pct < -1.0 {
                    "green"
                } else if pct > 1.0 {
                    "red"
                } else {
                    "inherit"
                };
                format!(" <span style=\"color:{color}\">{pct:+.1}%</span>")
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        let tp = if has_throughput {
            comp.throughput
                .as_ref()
                .map(|tp| {
                    let (val, unit) = tp.compute(bench.summary.mean, tp_unit);
                    format!("<td>{val:.2} {unit}</td>")
                })
                .unwrap_or_else(|| "<td></td>".to_string())
        } else {
            String::new()
        };

        html.push_str(&format!(
            "<tr{cls}><td>{}{vs}</td><td>{}</td><td>±{}</td>{tp}</tr>\n",
            bench.name,
            format_ns(bench.summary.mean),
            format_ns(bench.summary.mad),
        ));
    }
    html.push_str("</table>\n");

    // SVG bar chart
    html.push_str(&render_svg_bar_chart(comp));

    html.push_str("</details>\n");
    html
}

fn render_svg_bar_chart(comp: &ComparisonResult) -> String {
    if comp.benchmarks.len() < 2 {
        return String::new();
    }

    let tp_unit = comp.throughput_unit.as_deref();
    let has_throughput = comp.throughput.is_some();

    let mut sorted: Vec<&crate::results::BenchmarkResult> = comp.benchmarks.iter().collect();
    sorted.sort_by(|a, b| a.summary.mean.total_cmp(&b.summary.mean));

    let max_mean = sorted.last().map(|b| b.summary.mean).unwrap_or(1.0);
    let min_mean = sorted.first().map(|b| b.summary.mean).unwrap_or(1.0);

    let bar_h = 24;
    let gap = 4;
    let label_w = 200;
    let chart_w = 400;
    let value_w = 120;
    let total_w = label_w + chart_w + value_w + 20;
    let total_h = sorted.len() * (bar_h + gap) + 10;

    let mut svg = format!(
        "<svg width=\"{total_w}\" height=\"{total_h}\" xmlns=\"http://www.w3.org/2000/svg\">\n\
         <style>\n\
           text {{ font-family: -apple-system, sans-serif; font-size: 13px; fill: #c0caf5; }}\n\
           .bar {{ fill: #7aa2f7; }}\n\
           .bar-fastest {{ fill: #9ece6a; }}\n\
           .value {{ font-size: 12px; fill: #565f89; }}\n\
         </style>\n\
         <rect width=\"100%\" height=\"100%\" fill=\"#1a1b26\" rx=\"6\"/>\n"
    );

    let fastest_mean = sorted
        .first()
        .map(|b| b.summary.mean)
        .unwrap_or(f64::INFINITY);

    for (i, bench) in sorted.iter().enumerate() {
        let y = i * (bar_h + gap) + 5;
        let is_fastest = (bench.summary.mean - fastest_mean).abs() < f64::EPSILON;

        let frac = if has_throughput && min_mean > 0.0 {
            min_mean / bench.summary.mean
        } else {
            bench.summary.mean / max_mean
        };
        let bar_w = (frac * chart_w as f64).max(2.0) as usize;
        let cls = if is_fastest { "bar-fastest" } else { "bar" };

        let label = if bench.name.len() > 25 {
            format!("{}…", &bench.name[..24])
        } else {
            bench.name.clone()
        };

        let value = if has_throughput {
            comp.throughput
                .as_ref()
                .map(|tp| tp.format(bench.summary.mean, tp_unit))
                .unwrap_or_default()
        } else {
            format_ns(bench.summary.mean)
        };

        svg.push_str(&format!(
            "  <text x=\"5\" y=\"{ty}\" dominant-baseline=\"middle\">{label}</text>\n\
             <rect class=\"{cls}\" x=\"{label_w}\" y=\"{y}\" width=\"{bar_w}\" height=\"{bar_h}\" rx=\"3\"/>\n\
             <text class=\"value\" x=\"{vx}\" y=\"{ty}\" dominant-baseline=\"middle\">{value}</text>\n",
            ty = y + bar_h / 2,
            vx = label_w + bar_w + 8,
        ));
    }

    svg.push_str("</svg>\n");
    svg
}

const HTML_HEAD: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>zenbench results</title>
<style>
  :root { --bg: #1a1b26; --fg: #c0caf5; --accent: #7aa2f7; --dim: #565f89; }
  body { font-family: -apple-system, sans-serif; background: var(--bg); color: var(--fg); margin: 0; padding: 0; }
  .container { max-width: 960px; margin: 0 auto; padding: 2rem; }
  h1 { color: #fff; } h1 small { color: var(--dim); font-weight: normal; font-size: 0.5em; }
  h2 { color: var(--accent); font-size: 1.2rem; margin: 0; display: inline; }
  .meta { color: var(--dim); font-size: 0.85rem; }
  summary { cursor: pointer; padding: 0.5rem 0; }
  details { margin: 1rem 0; border: 1px solid #2f3549; border-radius: 8px; padding: 1rem; }
  table { border-collapse: collapse; width: 100%; margin: 0.5rem 0; }
  th, td { padding: 0.3rem 0.6rem; text-align: left; border-bottom: 1px solid #2f3549; font-size: 0.9rem; }
  th { color: var(--accent); }
  .fastest td { color: #9ece6a; }
  svg { margin: 0.5rem 0; display: block; }
  code { background: #24283b; padding: 0.1em 0.3em; border-radius: 3px; font-size: 0.85em; }
</style>
</head>
<body><div class="container">
"#;
