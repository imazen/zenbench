//! Publication-quality SVG chart generation via charts-rs.
//!
//! Enabled with the `charts` feature. Produces horizontal bar charts
//! from benchmark results, with grouped bars for matrix-structured
//! benchmarks (variant/param naming convention).

use crate::results::ComparisonResult;
use charts_rs::{HorizontalBarChart, Series};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Generate a horizontal bar chart SVG from a comparison group.
///
/// For matrix-structured groups (benchmarks named `variant/param`),
/// produces grouped bars: each series is a variant (decoder), each
/// category is a parameter (size). Values are shown in human-readable
/// time units (ns/µs/ms/s).
///
/// For flat groups, produces a simple sorted bar chart.
pub fn comparison_to_svg(comp: &ComparisonResult, theme: &str) -> Option<String> {
    if comp.benchmarks.len() < 2 {
        return None;
    }

    if let Some((variants, params, grid)) = detect_matrix(comp) {
        Some(render_matrix_chart(comp, &variants, &params, &grid, theme))
    } else {
        Some(render_flat_chart(comp, theme))
    }
}

/// Save SVG charts for all comparison groups in a suite result.
pub fn save_charts(
    result: &crate::results::SuiteResult,
    dir: &Path,
    theme: &str,
) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    for comp in &result.comparisons {
        if let Some(svg) = comparison_to_svg(comp, theme) {
            let filename = comp
                .group_name
                .replace('/', "_")
                .replace(' ', "_")
                .replace(':', "_");
            std::fs::write(dir.join(format!("{filename}.svg")), &svg)?;
        }
    }
    Ok(())
}

// ---- Matrix detection (shared with html.rs) ----

/// Returns (variants_sorted, params_in_order, grid: HashMap<(variant_idx, param_idx), bench_idx>)
fn detect_matrix(
    comp: &ComparisonResult,
) -> Option<(Vec<String>, Vec<String>, HashMap<(usize, usize), usize>)> {
    let mut param_order: Vec<String> = Vec::new();
    let mut variant_set: HashSet<String> = HashSet::new();
    let mut entries: Vec<(String, String, usize)> = Vec::new();

    for (i, bench) in comp.benchmarks.iter().enumerate() {
        if let Some(slash) = bench.name.rfind('/') {
            let variant = bench.name[..slash].to_string();
            let param = bench.name[slash + 1..].to_string();
            if !param.is_empty() && !variant.is_empty() {
                if !param_order.contains(&param) {
                    param_order.push(param.clone());
                }
                variant_set.insert(variant.clone());
                entries.push((variant, param, i));
            }
        }
    }

    if variant_set.len() < 2 || param_order.len() < 2 || entries.len() != comp.benchmarks.len() {
        return None;
    }

    let mut variants: Vec<String> = variant_set.into_iter().collect();
    variants.sort();

    let variant_idx = |v: &str| variants.iter().position(|x| x == v).unwrap();
    let param_idx = |p: &str| param_order.iter().position(|x| x == p).unwrap();

    let mut grid = HashMap::new();
    for (variant, param, bench_idx) in &entries {
        grid.insert((variant_idx(variant), param_idx(param)), *bench_idx);
    }

    Some((variants, param_order, grid))
}

fn render_matrix_chart(
    comp: &ComparisonResult,
    variants: &[String],
    params: &[String],
    grid: &HashMap<(usize, usize), usize>,
    theme: &str,
) -> String {
    // Each variant becomes a series, each param becomes an x-axis category.
    // Values are mean times in a chosen unit (we pick the unit from the median benchmark).
    let all_means: Vec<f64> = comp.benchmarks.iter().map(|b| b.summary.mean).collect();
    let median_mean = {
        let mut sorted = all_means.clone();
        sorted.sort_by(|a, b| a.total_cmp(b));
        sorted[sorted.len() / 2]
    };

    // Pick a consistent unit for all values
    let (unit, divisor) = pick_unit(median_mean);

    let series_list: Vec<Series> = variants
        .iter()
        .enumerate()
        .map(|(vi, name)| {
            let data: Vec<f32> = params
                .iter()
                .enumerate()
                .map(|(pi, _)| {
                    grid.get(&(vi, pi))
                        .map(|&bi| (comp.benchmarks[bi].summary.mean / divisor) as f32)
                        .unwrap_or(f32::NAN)
                })
                .collect();

            let mut s = Series::new(name.clone(), data);
            s.label_show = true;
            s
        })
        .collect();

    let x_axis_data: Vec<String> = params.to_vec();

    let mut chart = HorizontalBarChart::new_with_theme(series_list, x_axis_data, theme);
    chart.title_text = comp.group_name.clone();
    chart.sub_title_text = format!("mean time ({unit}), lower is better");
    chart.width = 800.0;
    chart.height = (100 + params.len() * variants.len() * 28 + params.len() * 30) as f32;
    chart.margin.right = 30.0;
    chart.margin.left = 10.0;

    chart.svg().unwrap_or_default()
}

fn render_flat_chart(comp: &ComparisonResult, theme: &str) -> String {
    // Single series, sorted by mean time
    let mut benches: Vec<(usize, f64)> = comp
        .benchmarks
        .iter()
        .enumerate()
        .map(|(i, b)| (i, b.summary.mean))
        .collect();
    benches.sort_by(|a, b| a.1.total_cmp(&b.1));

    let median_mean = benches[benches.len() / 2].1;
    let (unit, divisor) = pick_unit(median_mean);

    let x_axis_data: Vec<String> = benches
        .iter()
        .map(|&(i, _)| comp.benchmarks[i].name.clone())
        .collect();

    let data: Vec<f32> = benches
        .iter()
        .map(|&(i, _)| (comp.benchmarks[i].summary.mean / divisor) as f32)
        .collect();

    let mut s = Series::new("mean".to_string(), data);
    s.label_show = true;

    let mut chart = HorizontalBarChart::new_with_theme(vec![s], x_axis_data, theme);
    chart.title_text = comp.group_name.clone();
    chart.sub_title_text = format!("mean time ({unit}), lower is better");
    chart.width = 700.0;
    chart.height = (100 + benches.len() * 32) as f32;
    chart.margin.right = 20.0;
    chart.margin.left = 10.0;

    chart.svg().unwrap_or_default()
}

/// Pick a human-readable time unit and divisor for a given nanosecond value.
fn pick_unit(ns: f64) -> (&'static str, f64) {
    if ns >= 1e9 {
        ("s", 1e9)
    } else if ns >= 1e6 {
        ("ms", 1e6)
    } else if ns >= 1e3 {
        ("µs", 1e3)
    } else {
        ("ns", 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::results::{BenchmarkResult, ComparisonResult};
    use crate::stats::Summary;

    fn make_bench(name: &str, mean_ns: f64) -> BenchmarkResult {
        let mut b = BenchmarkResult::default();
        b.name = name.to_string();
        b.summary = Summary::from_slice(&[mean_ns * 0.95, mean_ns, mean_ns * 1.05]);
        b
    }

    fn make_comp(name: &str, benches: Vec<BenchmarkResult>) -> ComparisonResult {
        ComparisonResult {
            group_name: name.to_string(),
            benchmarks: benches,
            analyses: Vec::new(),
            completed_rounds: 30,
            throughput: None,
            cache_firewall: false,
            cache_firewall_bytes: 0,
            baseline_only: false,
            throughput_unit: None,
            sort_by_speed: false,
            expect_sub_ns: false,
            cold_start: false,
            iterations_per_sample: 100,
        }
    }

    #[test]
    fn matrix_chart_produces_svg() {
        let comp = make_comp(
            "decode baseline 4:2:0",
            vec![
                make_bench("mozjpeg/256x256", 275_000.0),
                make_bench("zenjpeg/256x256", 246_000.0),
                make_bench("mozjpeg/512x512", 1_060_000.0),
                make_bench("zenjpeg/512x512", 1_030_000.0),
            ],
        );
        let svg = comparison_to_svg(&comp, "light").expect("should produce SVG");
        assert!(svg.contains("<svg"));
        assert!(svg.contains("256x256"));
        assert!(svg.contains("mozjpeg"));
    }

    #[test]
    fn flat_chart_produces_svg() {
        let comp = make_comp(
            "sort algorithms",
            vec![
                make_bench("std_sort", 100_000.0),
                make_bench("unstable", 80_000.0),
                make_bench("parallel", 50_000.0),
            ],
        );
        let svg = comparison_to_svg(&comp, "light").expect("should produce SVG");
        assert!(svg.contains("<svg"));
        assert!(svg.contains("std_sort"));
    }

    #[test]
    fn dark_theme_produces_svg() {
        let comp = make_comp(
            "test",
            vec![
                make_bench("a/1", 100.0),
                make_bench("b/1", 200.0),
                make_bench("a/2", 300.0),
                make_bench("b/2", 400.0),
            ],
        );
        let svg = comparison_to_svg(&comp, "dark").expect("should produce SVG");
        assert!(svg.contains("<svg"));
    }
}
