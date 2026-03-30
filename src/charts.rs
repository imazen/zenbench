//! Publication-quality SVG chart generation via charts-rs.
//!
//! Enabled with the `charts` feature. Produces bar charts from benchmark
//! results, with grouped bars for matrix-structured benchmarks
//! (variant/param naming convention).
//!
//! # Orientation
//!
//! - [`ChartOrientation::Horizontal`] — best for few categories with long names
//! - [`ChartOrientation::Vertical`] — best for many categories (sizes, configs)

use crate::results::ComparisonResult;
use charts_rs::{BarChart, HorizontalBarChart, Series};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Chart bar orientation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChartOrientation {
    /// Horizontal bars (categories on Y axis, values on X). Good for few
    /// categories with long names.
    #[default]
    Horizontal,
    /// Vertical bars (categories on X axis, values on Y). Good for many
    /// categories — easier to compare across groups visually.
    Vertical,
}

/// Configuration for chart generation.
#[derive(Debug, Clone)]
pub struct ChartConfig {
    /// charts-rs theme name: "light", "dark", "grafana", "vintage", etc.
    pub theme: String,
    /// Bar orientation.
    pub orientation: ChartOrientation,
    /// Show ±MAD whiskers on bars.
    pub show_whiskers: bool,
    /// Show value labels on bars.
    pub show_labels: bool,
}

impl Default for ChartConfig {
    fn default() -> Self {
        Self {
            theme: "light".to_string(),
            orientation: ChartOrientation::default(),
            show_whiskers: true,
            show_labels: true,
        }
    }
}

/// Generate a bar chart SVG from a comparison group.
pub fn comparison_to_svg(comp: &ComparisonResult, config: &ChartConfig) -> Option<String> {
    if comp.benchmarks.len() < 2 {
        return None;
    }

    if let Some((variants, params, grid)) = detect_matrix(comp) {
        Some(render_matrix_chart(comp, &variants, &params, &grid, config))
    } else {
        Some(render_flat_chart(comp, config))
    }
}

/// Save SVG charts for all comparison groups in a suite result.
pub fn save_charts(
    result: &crate::results::SuiteResult,
    dir: &Path,
    config: &ChartConfig,
) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    for comp in &result.comparisons {
        if let Some(svg) = comparison_to_svg(comp, config) {
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

// ---- Matrix detection ----

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

// ---- Chart rendering ----

fn build_series(
    comp: &ComparisonResult,
    variants: &[String],
    params: &[String],
    grid: &HashMap<(usize, usize), usize>,
    divisor: f64,
    show_labels: bool,
) -> Vec<Series> {
    variants
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
            s.label_show = show_labels;
            s
        })
        .collect()
}

fn median_mean(comp: &ComparisonResult) -> f64 {
    let mut means: Vec<f64> = comp.benchmarks.iter().map(|b| b.summary.mean).collect();
    means.sort_by(|a, b| a.total_cmp(b));
    means[means.len() / 2]
}

fn render_matrix_chart(
    comp: &ComparisonResult,
    variants: &[String],
    params: &[String],
    grid: &HashMap<(usize, usize), usize>,
    config: &ChartConfig,
) -> String {
    let (unit, divisor) = pick_unit(median_mean(comp));
    let series_list = build_series(comp, variants, params, grid, divisor, config.show_labels);
    let x_axis_data: Vec<String> = params.to_vec();
    let sub = format!("mean time ({unit}), lower is better");

    let base_svg = match config.orientation {
        ChartOrientation::Horizontal => {
            let mut c = HorizontalBarChart::new_with_theme(series_list, x_axis_data, &config.theme);
            c.title_text = comp.group_name.clone();
            c.sub_title_text = sub;
            c.title_align = charts_rs::Align::Left;
            c.legend_align = charts_rs::Align::Right;
            c.width = 800.0;
            c.height =
                (120 + params.len() * variants.len() * 28 + params.len() * 30) as f32;
            c.margin.top = 15.0;
            c.margin.right = 30.0;
            c.margin.left = 10.0;
            c.svg().unwrap_or_default()
        }
        ChartOrientation::Vertical => {
            let mut c = BarChart::new_with_theme(series_list, x_axis_data, &config.theme);
            c.title_text = comp.group_name.clone();
            c.sub_title_text = sub;
            c.title_align = charts_rs::Align::Left;
            c.legend_align = charts_rs::Align::Right;
            c.width = (120 + params.len() * variants.len() * 32 + params.len() * 20)
                .max(600) as f32;
            c.height = 450.0;
            c.margin.top = 15.0;
            c.margin.right = 20.0;
            c.margin.left = 10.0;
            c.x_axis_name_rotate = if params.iter().any(|p| p.len() > 6) {
                -30.0
            } else {
                0.0
            };
            c.series_label_font_size = 10.0;
            c.svg().unwrap_or_default()
        }
    };

    if config.show_whiskers {
        inject_whiskers_comment(&base_svg, comp, variants, params, grid, divisor)
    } else {
        base_svg
    }
}

fn render_flat_chart(comp: &ComparisonResult, config: &ChartConfig) -> String {
    let mut benches: Vec<(usize, f64)> = comp
        .benchmarks
        .iter()
        .enumerate()
        .map(|(i, b)| (i, b.summary.mean))
        .collect();
    benches.sort_by(|a, b| a.1.total_cmp(&b.1));

    let mm = benches[benches.len() / 2].1;
    let (unit, divisor) = pick_unit(mm);

    let x_axis_data: Vec<String> = benches
        .iter()
        .map(|&(i, _)| comp.benchmarks[i].name.clone())
        .collect();

    let data: Vec<f32> = benches
        .iter()
        .map(|&(i, _)| (comp.benchmarks[i].summary.mean / divisor) as f32)
        .collect();

    let mut s = Series::new("mean".to_string(), data);
    s.label_show = config.show_labels;

    let sub = format!("mean time ({unit}), lower is better");

    match config.orientation {
        ChartOrientation::Horizontal => {
            let mut c = HorizontalBarChart::new_with_theme(vec![s], x_axis_data, &config.theme);
            c.title_text = comp.group_name.clone();
            c.sub_title_text = sub;
            c.title_align = charts_rs::Align::Left;
            c.width = 700.0;
            c.height = (100 + benches.len() * 32) as f32;
            c.margin.top = 15.0;
            c.margin.right = 20.0;
            c.margin.left = 10.0;
            c.svg().unwrap_or_default()
        }
        ChartOrientation::Vertical => {
            let mut c = BarChart::new_with_theme(vec![s], x_axis_data, &config.theme);
            c.title_text = comp.group_name.clone();
            c.sub_title_text = sub;
            c.title_align = charts_rs::Align::Left;
            c.width = (80 + benches.len() * 60).max(400) as f32;
            c.height = 400.0;
            c.margin.top = 15.0;
            c.margin.right = 20.0;
            c.margin.left = 10.0;
            c.svg().unwrap_or_default()
        }
    }
}

/// Inject ±MAD annotation into the SVG as a text comment after the closing </svg>.
///
/// charts-rs doesn't support native error bars, so we append a small table
/// showing mean ± MAD for each benchmark as an HTML comment inside the SVG.
/// This preserves the data for anyone inspecting the SVG source.
fn inject_whiskers_comment(
    svg: &str,
    comp: &ComparisonResult,
    variants: &[String],
    params: &[String],
    grid: &HashMap<(usize, usize), usize>,
    divisor: f64,
) -> String {
    // Find the unit from the divisor
    let unit_label = match divisor as u64 {
        1 => "ns",
        1_000 => "µs",
        1_000_000 => "ms",
        1_000_000_000 => "s",
        _ => "?",
    };

    let mut comment = String::from("\n<!-- zenbench ±MAD data\n");
    for (pi, param) in params.iter().enumerate() {
        comment.push_str(&format!("  {param}:\n"));
        for (vi, variant) in variants.iter().enumerate() {
            if let Some(&bi) = grid.get(&(vi, pi)) {
                let b = &comp.benchmarks[bi];
                let mean = b.summary.mean / divisor;
                let mad = b.summary.mad / divisor;
                comment.push_str(&format!(
                    "    {variant}: {mean:.2} ±{mad:.2} {unit_label}\n"
                ));
            }
        }
    }
    comment.push_str("-->\n");

    // Insert comment before closing </svg>
    if let Some(pos) = svg.rfind("</svg>") {
        let mut result = svg[..pos].to_string();
        result.push_str(&comment);
        result.push_str(&svg[pos..]);
        result
    } else {
        format!("{svg}{comment}")
    }
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
    fn matrix_horizontal() {
        let comp = make_comp(
            "decode baseline 4:2:0",
            vec![
                make_bench("mozjpeg/256x256", 275_000.0),
                make_bench("zenjpeg/256x256", 246_000.0),
                make_bench("mozjpeg/512x512", 1_060_000.0),
                make_bench("zenjpeg/512x512", 1_030_000.0),
            ],
        );
        let cfg = ChartConfig::default();
        let svg = comparison_to_svg(&comp, &cfg).unwrap();
        assert!(svg.contains("<svg"));
        assert!(svg.contains("256x256"));
        assert!(svg.contains("mozjpeg"));
        assert!(svg.contains("±"), "should contain MAD data");
    }

    #[test]
    fn matrix_vertical() {
        let comp = make_comp(
            "decode baseline 4:2:0",
            vec![
                make_bench("mozjpeg/2048", 15_500_000.0),
                make_bench("zenjpeg/2048", 15_300_000.0),
                make_bench("mozjpeg/4096", 62_900_000.0),
                make_bench("zenjpeg/4096", 62_200_000.0),
            ],
        );
        let cfg = ChartConfig {
            orientation: ChartOrientation::Vertical,
            ..Default::default()
        };
        let svg = comparison_to_svg(&comp, &cfg).unwrap();
        assert!(svg.contains("<svg"));
    }

    #[test]
    fn flat_chart() {
        let comp = make_comp(
            "sort",
            vec![
                make_bench("std", 100_000.0),
                make_bench("unstable", 80_000.0),
            ],
        );
        let svg = comparison_to_svg(&comp, &ChartConfig::default()).unwrap();
        assert!(svg.contains("<svg"));
    }

    #[test]
    fn dark_theme() {
        let comp = make_comp(
            "test",
            vec![
                make_bench("a/1", 100.0),
                make_bench("b/1", 200.0),
                make_bench("a/2", 300.0),
                make_bench("b/2", 400.0),
            ],
        );
        let cfg = ChartConfig {
            theme: "dark".to_string(),
            ..Default::default()
        };
        let svg = comparison_to_svg(&comp, &cfg).unwrap();
        assert!(svg.contains("<svg"));
    }
}
