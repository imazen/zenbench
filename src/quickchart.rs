//! QuickChart URL generation for embedding benchmark charts in READMEs.
//!
//! Produces URLs like `https://quickchart.io/chart?w=700&h=...&bkg=%23080808&f=png&c=...`
//! that render as dark-themed horizontal bar charts. No API key or external
//! dependencies required — the chart config is URL-encoded inline.

use crate::results::{ComparisonResult, SuiteResult};

/// A generated chart URL with its group name.
#[derive(Debug, Clone)]
pub struct QuickChartUrl {
    /// Benchmark group name.
    pub group_name: String,
    /// Full quickchart.io URL that renders the chart as a PNG.
    pub url: String,
}

/// Theme colors for a QuickChart chart.
///
/// Controls every color in the rendered chart. Use the preset constructors
/// (`phosphor`, `ocean`, `ember`, `arctic`, `sunset`, `mono`) or build your own.
#[derive(Debug, Clone)]
pub struct ColorScheme {
    /// Background hex (without `#`). Becomes the `bkg` URL param.
    pub background: String,
    /// Title text color.
    pub title: String,
    /// Data label color (the value text on each bar).
    pub data_label: String,
    /// Y-axis label color (benchmark names).
    pub bar_label: String,
    /// X-axis tick color (numeric scale).
    pub axis_tick: String,
    /// Grid line color.
    pub grid: String,
    /// Zero-line color on the x-axis.
    pub grid_zero: String,
    /// Legend label color (grouped charts).
    pub legend: String,
    /// Color for the fastest bar in single-dataset charts.
    pub fastest: String,
    /// Default bar color (non-fastest, no name match).
    pub default_bar: String,
    /// Palette for grouped (matrix) charts — one color per variant.
    pub grouped_palette: Vec<String>,
}

impl ColorScheme {
    /// **Phosphor** — green-on-black terminal aesthetic. The default.
    pub fn phosphor() -> Self {
        Self {
            background: "080808".into(),
            title: "#33ff66".into(),
            data_label: "#eeeeee".into(),
            bar_label: "#dddddd".into(),
            axis_tick: "#999999".into(),
            grid: "#1a1a1a".into(),
            grid_zero: "#333333".into(),
            legend: "#cccccc".into(),
            fastest: "#00ff41".into(),
            default_bar: "#666666".into(),
            grouped_palette: vec![
                "#00ff41".into(),
                "#007722".into(),
                "#2196f3".into(),
                "#ff9800".into(),
                "#bb9af7".into(),
                "#73daca".into(),
                "#f7768e".into(),
                "#ff9e64".into(),
            ],
        }
    }

    /// **Ocean** — deep navy with cyan and teal accents.
    pub fn ocean() -> Self {
        Self {
            background: "0a1628".into(),
            title: "#56d4e8".into(),
            data_label: "#e0e8f0".into(),
            bar_label: "#c8d4e0".into(),
            axis_tick: "#7090aa".into(),
            grid: "#152238".into(),
            grid_zero: "#2a3f5a".into(),
            legend: "#a0b8cc".into(),
            fastest: "#00e5ff".into(),
            default_bar: "#3a5a7c".into(),
            grouped_palette: vec![
                "#00e5ff".into(),
                "#0288d1".into(),
                "#26c6da".into(),
                "#66bb6a".into(),
                "#ab47bc".into(),
                "#ff7043".into(),
                "#ffa726".into(),
                "#78909c".into(),
            ],
        }
    }

    /// **Ember** — warm charcoal with orange and gold.
    pub fn ember() -> Self {
        Self {
            background: "1a1210".into(),
            title: "#ffb74d".into(),
            data_label: "#f0e0d0".into(),
            bar_label: "#dcc8b0".into(),
            axis_tick: "#998070".into(),
            grid: "#2a1e1a".into(),
            grid_zero: "#4a3530".into(),
            legend: "#ccaa88".into(),
            fastest: "#ff9100".into(),
            default_bar: "#6d4c41".into(),
            grouped_palette: vec![
                "#ff9100".into(),
                "#ff6d00".into(),
                "#ffca28".into(),
                "#ef5350".into(),
                "#ab47bc".into(),
                "#66bb6a".into(),
                "#42a5f5".into(),
                "#8d6e63".into(),
            ],
        }
    }

    /// **Arctic** — cool blue-gray with ice blue highlights.
    pub fn arctic() -> Self {
        Self {
            background: "0e1520".into(),
            title: "#81d4fa".into(),
            data_label: "#e8f0f8".into(),
            bar_label: "#c0d8e8".into(),
            axis_tick: "#6888a0".into(),
            grid: "#162030".into(),
            grid_zero: "#283848".into(),
            legend: "#90b0c8".into(),
            fastest: "#40c4ff".into(),
            default_bar: "#455a64".into(),
            grouped_palette: vec![
                "#40c4ff".into(),
                "#0091ea".into(),
                "#80deea".into(),
                "#a5d6a7".into(),
                "#ce93d8".into(),
                "#ffcc80".into(),
                "#ef9a9a".into(),
                "#b0bec5".into(),
            ],
        }
    }

    /// **Sunset** — deep purple with magenta and coral.
    pub fn sunset() -> Self {
        Self {
            background: "1a0e24".into(),
            title: "#f48fb1".into(),
            data_label: "#f0e0f0".into(),
            bar_label: "#d8c0e0".into(),
            axis_tick: "#907898".into(),
            grid: "#241838".into(),
            grid_zero: "#3a2850".into(),
            legend: "#c0a0d0".into(),
            fastest: "#ff4081".into(),
            default_bar: "#5c4070".into(),
            grouped_palette: vec![
                "#ff4081".into(),
                "#e040fb".into(),
                "#ff6e40".into(),
                "#ffab40".into(),
                "#69f0ae".into(),
                "#40c4ff".into(),
                "#b388ff".into(),
                "#ff80ab".into(),
            ],
        }
    }

    /// **Mono** — pure grayscale with white accent.
    pub fn mono() -> Self {
        Self {
            background: "111111".into(),
            title: "#ffffff".into(),
            data_label: "#e0e0e0".into(),
            bar_label: "#cccccc".into(),
            axis_tick: "#888888".into(),
            grid: "#1e1e1e".into(),
            grid_zero: "#333333".into(),
            legend: "#aaaaaa".into(),
            fastest: "#ffffff".into(),
            default_bar: "#555555".into(),
            grouped_palette: vec![
                "#ffffff".into(),
                "#bbbbbb".into(),
                "#888888".into(),
                "#cccccc".into(),
                "#999999".into(),
                "#dddddd".into(),
                "#aaaaaa".into(),
                "#777777".into(),
            ],
        }
    }
}

/// Configuration for QuickChart URL generation.
#[derive(Debug, Clone)]
pub struct QuickChartConfig {
    /// Chart width in pixels. Default: 700.
    pub width: u32,
    /// Per-bar height in pixels, used to compute total height. Default: 32.
    pub bar_height: u32,
    /// Fixed vertical padding in pixels (title, axes, margins). Default: 58.
    pub padding: u32,
    /// Image format: "png" or "svg". Default: "png".
    pub format: String,
    /// Whether to use throughput values (when available) instead of time. Default: true.
    pub prefer_throughput: bool,
    /// Custom bar colors by benchmark name. When a benchmark name matches a key,
    /// that color is used instead of the scheme's default_bar color.
    pub colors: Vec<(String, String)>,
    /// Color scheme controlling all theme colors.
    pub scheme: ColorScheme,
}

impl Default for QuickChartConfig {
    fn default() -> Self {
        Self {
            width: 700,
            bar_height: 32,
            padding: 58,
            format: "png".to_string(),
            prefer_throughput: true,
            colors: Vec::new(),
            scheme: ColorScheme::phosphor(),
        }
    }
}

impl QuickChartConfig {
    /// Create a config with the given color scheme and default layout.
    pub fn with_scheme(scheme: ColorScheme) -> Self {
        Self {
            scheme,
            ..Default::default()
        }
    }

    /// Look up the color for a benchmark name.
    fn color_for(&self, name: &str, is_fastest: bool) -> &str {
        if is_fastest {
            return &self.scheme.fastest;
        }
        for (pattern, color) in &self.colors {
            if name.contains(pattern.as_str()) {
                return color;
            }
        }
        &self.scheme.default_bar
    }
}

impl SuiteResult {
    /// Generate QuickChart URLs for each comparison group.
    ///
    /// Each group produces one horizontal bar chart URL. Benchmarks are sorted
    /// fastest-first. When throughput is set and `prefer_throughput` is true,
    /// values are throughput (higher = better); otherwise values are mean time
    /// in the most readable unit (lower = better).
    pub fn to_quickchart_urls(&self, config: &QuickChartConfig) -> Vec<QuickChartUrl> {
        self.comparisons
            .iter()
            .filter_map(|comp| build_chart_url(comp, config))
            .collect()
    }

    /// Generate markdown image links for all comparison groups.
    ///
    /// Returns markdown like:
    /// ```text
    /// ![Sort Algorithms](https://quickchart.io/chart?...)
    /// ```
    pub fn to_quickchart_markdown(&self, config: &QuickChartConfig) -> String {
        let urls = self.to_quickchart_urls(config);
        let mut out = String::new();
        for chart in &urls {
            out.push_str(&format!("![{}]({})\n\n", chart.group_name, chart.url));
        }
        out
    }
}

/// Data for one bar in the chart.
struct BarData {
    label: String,
    value: f64,
    is_fastest: bool,
}

fn build_chart_url(comp: &ComparisonResult, config: &QuickChartConfig) -> Option<QuickChartUrl> {
    if comp.benchmarks.is_empty() {
        return None;
    }

    // Detect matrix structure (variant/param naming) for grouped charts
    if let Some(matrix) = crate::html::detect_matrix(comp) {
        return build_grouped_chart_url(comp, &matrix, config);
    }

    let use_throughput = config.prefer_throughput && comp.throughput.is_some();

    // Collect bar data
    let mut bars: Vec<BarData> = comp
        .benchmarks
        .iter()
        .map(|b| {
            let value = if use_throughput {
                let tp = comp.throughput.as_ref().unwrap();
                let (val, _) = tp.compute(b.summary.mean, comp.throughput_unit.as_deref());
                val
            } else {
                b.summary.mean
            };
            BarData {
                label: b.name.clone(),
                value,
                is_fastest: false,
            }
        })
        .collect();

    if bars.is_empty() {
        return None;
    }

    // Mark fastest and sort
    if use_throughput {
        // Higher throughput = faster
        let max_idx = bars
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.value.total_cmp(&b.value))
            .map(|(i, _)| i)
            .unwrap();
        bars[max_idx].is_fastest = true;
        // Sort descending (highest throughput first)
        bars.sort_by(|a, b| b.value.total_cmp(&a.value));
    } else {
        // Lower time = faster
        let min_idx = bars
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| a.value.total_cmp(&b.value))
            .map(|(i, _)| i)
            .unwrap();
        bars[min_idx].is_fastest = true;
        // Sort ascending (fastest/lowest time first)
        bars.sort_by(|a, b| a.value.total_cmp(&b.value));
    }

    // Determine unit and format values
    let (display_values, unit_suffix) = if use_throughput {
        let tp = comp.throughput.as_ref().unwrap();
        let (_, unit) = tp.compute(
            comp.benchmarks[0].summary.mean,
            comp.throughput_unit.as_deref(),
        );
        let values: Vec<f64> = bars.iter().map(|b| b.value).collect();
        (values, unit)
    } else {
        format_time_values(&bars)
    };

    let (title, formatter) =
        build_title_and_formatter(&comp.group_name, &unit_suffix, use_throughput);

    // Build labels, data, colors arrays
    let labels: Vec<String> = bars
        .iter()
        .map(|b| format!("\"{}\"", escape_json(&b.label)))
        .collect();
    let data: Vec<String> = display_values
        .iter()
        .map(|v| format_chart_value(*v))
        .collect();
    let bg_colors: Vec<String> = bars
        .iter()
        .map(|b| format!("\"{}\"", config.color_for(&b.label, b.is_fastest)))
        .collect();

    let chart_json = build_single_dataset_json(
        &labels,
        &data,
        &bg_colors,
        &title,
        &formatter,
        &config.scheme,
    );

    let height = config.padding + (bars.len() as u32) * config.bar_height;
    Some(finish_url(comp, config, &chart_json, height))
}

/// Build a grouped (multi-dataset) chart for matrix-structured benchmarks.
///
/// Labels are the parameter values (e.g., "256x256", "1024x1024").
/// Each variant becomes a dataset with its own color and legend entry.
fn build_grouped_chart_url(
    comp: &ComparisonResult,
    matrix: &crate::html::MatrixChart,
    config: &QuickChartConfig,
) -> Option<QuickChartUrl> {
    let use_throughput = config.prefer_throughput && comp.throughput.is_some();

    // Determine unit from all benchmarks
    let unit_suffix = if use_throughput {
        let tp = comp.throughput.as_ref().unwrap();
        let (_, unit) = tp.compute(
            comp.benchmarks[0].summary.mean,
            comp.throughput_unit.as_deref(),
        );
        unit
    } else {
        // Pick unit from the median benchmark value
        let mut all_means: Vec<f64> = comp.benchmarks.iter().map(|b| b.summary.mean).collect();
        all_means.sort_by(|a, b| a.total_cmp(b));
        let median_ns = all_means[all_means.len() / 2];
        let (_, unit, _) = crate::format::ns_unit(median_ns.abs());
        unit.to_string()
    };

    let (divisor, _, _) = if use_throughput {
        (1.0, "", 0)
    } else {
        let mut all_means: Vec<f64> = comp.benchmarks.iter().map(|b| b.summary.mean).collect();
        all_means.sort_by(|a, b| a.total_cmp(b));
        crate::format::ns_unit(all_means[all_means.len() / 2].abs())
    };

    let (title, formatter) =
        build_title_and_formatter(&comp.group_name, &unit_suffix, use_throughput);

    // Labels = parameter values
    let labels: Vec<String> = matrix
        .params
        .iter()
        .map(|p| format!("\"{}\"", escape_json(p)))
        .collect();

    // One dataset per variant
    let mut datasets: Vec<String> = Vec::new();
    for (vi, variant) in matrix.variants.iter().enumerate() {
        let palette = &config.scheme.grouped_palette;
        let color = &palette[vi % palette.len()];

        let data: Vec<String> = matrix
            .params
            .iter()
            .enumerate()
            .map(|(pi, _)| {
                if let Some(&bi) = matrix.cells.get(&(vi, pi)) {
                    let bench = &comp.benchmarks[bi];
                    let value = if use_throughput {
                        let tp = comp.throughput.as_ref().unwrap();
                        let (val, _) =
                            tp.compute(bench.summary.mean, comp.throughput_unit.as_deref());
                        val
                    } else {
                        bench.summary.mean / divisor
                    };
                    format_chart_value(value)
                } else {
                    "0".to_string()
                }
            })
            .collect();

        datasets.push(format!(
            "{{\"label\":\"{}\",\"data\":[{}],\"backgroundColor\":\"{}\"}}",
            escape_json(variant),
            data.join(","),
            color,
        ));
    }

    let s = &config.scheme;
    let chart_json = format!(
        concat!(
            "{{",
            "\"type\":\"horizontalBar\",",
            "\"data\":{{",
            "\"labels\":[{labels}],",
            "\"datasets\":[{datasets}]",
            "}},",
            "\"options\":{{",
            "\"layout\":{{\"padding\":{{\"top\":0,\"bottom\":0,\"left\":0,\"right\":70}}}},",
            "\"plugins\":{{\"datalabels\":{{",
            "\"anchor\":\"end\",\"align\":\"end\",",
            "\"color\":\"{c_dl}\",",
            "\"font\":{{\"weight\":\"bold\",\"size\":18}},",
            "\"formatter\":\"{formatter}\"",
            "}}}},",
            "\"scales\":{{",
            "\"xAxes\":[{{",
            "\"ticks\":{{\"beginAtZero\":true,\"fontColor\":\"{c_xt}\",\"fontSize\":18,\"padding\":2}},",
            "\"gridLines\":{{\"color\":\"{c_gr}\",\"zeroLineColor\":\"{c_gz}\",\"drawTicks\":false}}",
            "}}],",
            "\"yAxes\":[{{",
            "\"ticks\":{{\"fontColor\":\"{c_bl}\",\"fontSize\":20,\"padding\":6}},",
            "\"gridLines\":{{\"color\":\"{c_gr}\",\"drawTicks\":false}},",
            "\"barPercentage\":0.75,\"categoryPercentage\":0.85",
            "}}]",
            "}},",
            "\"legend\":{{\"display\":true,\"position\":\"bottom\",",
            "\"labels\":{{\"fontColor\":\"{c_lg}\",\"fontSize\":18,\"padding\":8}}}},",
            "\"title\":{{\"display\":true,\"fontColor\":\"{c_tt}\",\"fontSize\":22,",
            "\"padding\":6,\"text\":\"{title}\"}}",
            "}}",
            "}}"
        ),
        labels = labels.join(","),
        datasets = datasets.join(","),
        formatter = escape_json(&formatter),
        title = escape_json(&title),
        c_dl = s.data_label,
        c_xt = s.axis_tick,
        c_gr = s.grid,
        c_gz = s.grid_zero,
        c_bl = s.bar_label,
        c_lg = s.legend,
        c_tt = s.title,
    );

    // Height: each param gets bars for all variants, plus legend space
    let bars_per_param = matrix.variants.len() as u32;
    let legend_extra = 28; // bottom legend
    let height = config.padding
        + legend_extra
        + (matrix.params.len() as u32) * bars_per_param * config.bar_height;

    Some(finish_url(comp, config, &chart_json, height))
}

fn build_title_and_formatter(
    group_name: &str,
    unit_suffix: &str,
    use_throughput: bool,
) -> (String, String) {
    let title = if use_throughput {
        format!("{} ({}, higher = better)", group_name, unit_suffix)
    } else {
        format!("{} ({}, lower = better)", group_name, unit_suffix)
    };
    let formatter = format!("(v)=>v+' {}'", escape_json(unit_suffix));
    (title, formatter)
}

fn build_single_dataset_json(
    labels: &[String],
    data: &[String],
    bg_colors: &[String],
    title: &str,
    formatter: &str,
    s: &ColorScheme,
) -> String {
    format!(
        concat!(
            "{{",
            "\"type\":\"horizontalBar\",",
            "\"data\":{{",
            "\"labels\":[{labels}],",
            "\"datasets\":[{{\"data\":[{data}],\"backgroundColor\":[{colors}]}}]",
            "}},",
            "\"options\":{{",
            "\"layout\":{{\"padding\":{{\"top\":0,\"bottom\":0,\"left\":0,\"right\":70}}}},",
            "\"plugins\":{{\"datalabels\":{{",
            "\"anchor\":\"end\",\"align\":\"end\",",
            "\"color\":\"{c_dl}\",",
            "\"font\":{{\"weight\":\"bold\",\"size\":20}},",
            "\"formatter\":\"{formatter}\"",
            "}}}},",
            "\"scales\":{{",
            "\"xAxes\":[{{",
            "\"ticks\":{{\"beginAtZero\":true,\"fontColor\":\"{c_xt}\",\"fontSize\":18,\"padding\":2}},",
            "\"gridLines\":{{\"color\":\"{c_gr}\",\"zeroLineColor\":\"{c_gz}\",\"drawTicks\":false}}",
            "}}],",
            "\"yAxes\":[{{",
            "\"ticks\":{{\"fontColor\":\"{c_bl}\",\"fontSize\":20,\"padding\":6}},",
            "\"gridLines\":{{\"color\":\"{c_gr}\",\"drawTicks\":false}},",
            "\"barPercentage\":0.7,\"categoryPercentage\":0.85",
            "}}]",
            "}},",
            "\"legend\":{{\"display\":false}},",
            "\"title\":{{\"display\":true,\"fontColor\":\"{c_tt}\",\"fontSize\":22,",
            "\"padding\":6,\"text\":\"{title}\"}}",
            "}}",
            "}}"
        ),
        labels = labels.join(","),
        data = data.join(","),
        colors = bg_colors.join(","),
        formatter = escape_json(formatter),
        title = escape_json(title),
        c_dl = s.data_label,
        c_xt = s.axis_tick,
        c_gr = s.grid,
        c_gz = s.grid_zero,
        c_bl = s.bar_label,
        c_tt = s.title,
    )
}

fn finish_url(
    comp: &ComparisonResult,
    config: &QuickChartConfig,
    chart_json: &str,
    height: u32,
) -> QuickChartUrl {
    let encoded = url_encode(chart_json);
    let url = format!(
        "https://quickchart.io/chart?w={}&h={}&bkg=%23{}&f={}&c={}",
        config.width, height, config.scheme.background, config.format, encoded
    );
    QuickChartUrl {
        group_name: comp.group_name.clone(),
        url,
    }
}

/// Convert time values (ns) to a display-friendly unit, returning scaled values and unit string.
fn format_time_values(bars: &[BarData]) -> (Vec<f64>, String) {
    if bars.is_empty() {
        return (Vec::new(), "ns".to_string());
    }

    // Use the median value to pick the unit
    let median_ns = bars[bars.len() / 2].value;
    let (divisor, unit, _dp) = crate::format::ns_unit(median_ns.abs());

    let values: Vec<f64> = bars.iter().map(|b| b.value / divisor).collect();
    (values, unit.to_string())
}

/// Format a value for the chart data array — use integer when possible, otherwise 1-2 decimal places.
fn format_chart_value(v: f64) -> String {
    if v >= 100.0 && (v - v.round()).abs() < 0.05 {
        format!("{}", v.round() as i64)
    } else if v >= 10.0 {
        format!("{:.1}", v)
    } else {
        format!("{:.2}", v)
    }
}

/// Minimal JSON string escaping (for values embedded in the chart config).
fn escape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
}

/// URL-encode a string (percent-encoding all non-unreserved characters).
fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push(HEX_UPPER[(b >> 4) as usize] as char);
                out.push(HEX_UPPER[(b & 0x0f) as usize] as char);
            }
        }
    }
    out
}

const HEX_UPPER: &[u8; 16] = b"0123456789ABCDEF";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::Throughput;
    use crate::results::{BenchmarkResult, ComparisonResult};
    use crate::stats::Summary;

    fn make_benchmark(name: &str, mean_ns: f64) -> BenchmarkResult {
        BenchmarkResult {
            name: name.to_string(),
            summary: {
                let mut s = Summary::new();
                // Feed samples to get the desired mean
                s.push(mean_ns);
                s
            },
            ..Default::default()
        }
    }

    fn make_comparison(
        name: &str,
        benchmarks: Vec<BenchmarkResult>,
        throughput: Option<Throughput>,
    ) -> ComparisonResult {
        ComparisonResult {
            group_name: name.to_string(),
            benchmarks,
            analyses: Vec::new(),
            completed_rounds: 10,
            throughput,
            cache_firewall: false,
            cache_firewall_bytes: 0,
            baseline_only: false,
            throughput_unit: None,
            sort_by_speed: true,
            expect_sub_ns: false,
            cold_start: false,
            iterations_per_sample: 1000,
        }
    }

    #[test]
    fn url_starts_with_quickchart() {
        let comp = make_comparison(
            "sorting",
            vec![
                make_benchmark("quicksort", 100.0),
                make_benchmark("bubblesort", 500.0),
            ],
            None,
        );
        let config = QuickChartConfig::default();
        let result = build_chart_url(&comp, &config).unwrap();
        assert!(result.url.starts_with("https://quickchart.io/chart?"));
        assert!(result.url.contains("w=700"));
        assert!(result.url.contains("bkg=%23080808"));
        assert!(result.url.contains("f=png"));
        assert_eq!(result.group_name, "sorting");
    }

    #[test]
    fn fastest_gets_green_color() {
        let comp = make_comparison(
            "test",
            vec![make_benchmark("fast", 50.0), make_benchmark("slow", 200.0)],
            None,
        );
        let config = QuickChartConfig::default();
        let result = build_chart_url(&comp, &config).unwrap();
        // The URL-decoded JSON should contain #00ff41 for the fastest
        let decoded = result.url.replace("%23", "#").replace("%22", "\"");
        assert!(decoded.contains("#00ff41"), "fastest should get green");
        assert!(decoded.contains("#666666"), "slower should get gray");
    }

    #[test]
    fn throughput_mode_uses_higher_is_better() {
        let comp = make_comparison(
            "decode",
            vec![
                make_benchmark("codec_a", 100.0), // faster → higher throughput
                make_benchmark("codec_b", 200.0),
            ],
            Some(Throughput::Bytes(1_000_000)),
        );
        let config = QuickChartConfig::default();
        let result = build_chart_url(&comp, &config).unwrap();
        assert!(
            result.url.contains("higher"),
            "should say higher = better for throughput"
        );
    }

    #[test]
    fn time_mode_uses_lower_is_better() {
        let comp = make_comparison(
            "sort",
            vec![make_benchmark("a", 100.0), make_benchmark("b", 200.0)],
            None,
        );
        let config = QuickChartConfig::default();
        let result = build_chart_url(&comp, &config).unwrap();
        assert!(
            result.url.contains("lower"),
            "should say lower = better for time"
        );
    }

    #[test]
    fn empty_benchmarks_returns_none() {
        let comp = make_comparison("empty", vec![], None);
        let config = QuickChartConfig::default();
        assert!(build_chart_url(&comp, &config).is_none());
    }

    #[test]
    fn height_scales_with_bar_count() {
        let config = QuickChartConfig::default();

        let comp2 = make_comparison(
            "two",
            vec![make_benchmark("a", 100.0), make_benchmark("b", 200.0)],
            None,
        );
        let comp5 = make_comparison(
            "five",
            vec![
                make_benchmark("a", 100.0),
                make_benchmark("b", 200.0),
                make_benchmark("c", 300.0),
                make_benchmark("d", 400.0),
                make_benchmark("e", 500.0),
            ],
            None,
        );

        let url2 = build_chart_url(&comp2, &config).unwrap();
        let url5 = build_chart_url(&comp5, &config).unwrap();

        // Extract height from h= parameter
        let h2: u32 = url2
            .url
            .split("h=")
            .nth(1)
            .unwrap()
            .split('&')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        let h5: u32 = url5
            .url
            .split("h=")
            .nth(1)
            .unwrap()
            .split('&')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert!(h5 > h2, "5 bars should be taller than 2 bars");
        assert_eq!(h5 - h2, 3 * config.bar_height);
    }

    #[test]
    fn custom_colors_applied() {
        let comp = make_comparison(
            "codecs",
            vec![
                make_benchmark("zenjpeg", 50.0),
                make_benchmark("mozjpeg", 80.0),
                make_benchmark("libjpeg", 120.0),
            ],
            None,
        );
        let config = QuickChartConfig {
            colors: vec![("mozjpeg".to_string(), "#ff9800".to_string())],
            ..Default::default()
        };
        let result = build_chart_url(&comp, &config).unwrap();
        let decoded = result.url.replace("%23", "#").replace("%22", "\"");
        assert!(decoded.contains("#ff9800"), "mozjpeg should get amber");
    }

    #[test]
    fn url_encode_roundtrip() {
        assert_eq!(url_encode("hello"), "hello");
        assert_eq!(url_encode("a b"), "a%20b");
        assert_eq!(url_encode("{\"x\":1}"), "%7B%22x%22%3A1%7D");
    }

    #[test]
    fn escape_json_handles_special_chars() {
        assert_eq!(escape_json(r#"say "hi""#), r#"say \"hi\""#);
        assert_eq!(escape_json("a\\b"), "a\\\\b");
        assert_eq!(escape_json("line\nnewline"), "line\\nnewline");
    }

    #[test]
    fn markdown_output() {
        let suite = SuiteResult {
            run_id: crate::results::RunId("test-123".to_string()),
            timestamp: "2026-01-01T00:00:00Z".to_string(),
            git_hash: None,
            ci_environment: None,
            comparisons: vec![make_comparison(
                "Sort",
                vec![
                    make_benchmark("quick", 100.0),
                    make_benchmark("bubble", 500.0),
                ],
                None,
            )],
            total_time: std::time::Duration::from_secs(5),
            gate_waits: 0,
            gate_wait_time: std::time::Duration::ZERO,
            unreliable: false,
            timer_resolution_ns: 41,
            loop_overhead_ns: 0.5,
            testbed: None,
            calibration: None,
            ..Default::default()
        };
        let md = suite.to_quickchart_markdown(&QuickChartConfig::default());
        assert!(md.starts_with("![Sort](https://quickchart.io/chart?"));
        assert!(md.contains("w=700"));
    }

    // --- Grouped (matrix) chart tests ---

    #[test]
    fn grouped_chart_detected_for_matrix_names() {
        // Benchmarks named "variant/param" should produce a grouped chart
        let comp = make_comparison(
            "SrcOver blend",
            vec![
                make_benchmark("BRAG8/256x256", 29.0),
                make_benchmark("BRAG8/1024x1024", 20.0),
                make_benchmark("sw-composite/256x256", 13.0),
                make_benchmark("sw-composite/1024x1024", 11.0),
            ],
            None,
        );
        let config = QuickChartConfig::default();
        let result = build_chart_url(&comp, &config).unwrap();

        // Should have legend enabled (grouped chart) and datasets with labels
        let decoded = url_decode_rough(&result.url);
        assert!(
            decoded.contains("\"legend\":{\"display\":true"),
            "grouped chart should enable legend"
        );
        assert!(
            decoded.contains("\"label\":\"BRAG8\""),
            "should have variant as dataset label"
        );
        assert!(
            decoded.contains("\"label\":\"sw-composite\""),
            "should have second variant as dataset label"
        );
    }

    #[test]
    fn grouped_chart_has_param_labels() {
        let comp = make_comparison(
            "blend",
            vec![
                make_benchmark("fast/small", 10.0),
                make_benchmark("fast/large", 20.0),
                make_benchmark("slow/small", 30.0),
                make_benchmark("slow/large", 40.0),
            ],
            None,
        );
        let config = QuickChartConfig::default();
        let result = build_chart_url(&comp, &config).unwrap();
        let decoded = url_decode_rough(&result.url);
        // Labels should be the params, not the full "variant/param" names
        assert!(decoded.contains("\"small\""), "params should be labels");
        assert!(decoded.contains("\"large\""), "params should be labels");
    }

    #[test]
    fn grouped_chart_uses_palette_colors() {
        let comp = make_comparison(
            "test",
            vec![
                make_benchmark("a/x", 10.0),
                make_benchmark("a/y", 20.0),
                make_benchmark("b/x", 30.0),
                make_benchmark("b/y", 40.0),
            ],
            None,
        );
        let config = QuickChartConfig::default();
        let result = build_chart_url(&comp, &config).unwrap();
        let decoded = url_decode_rough(&result.url);
        // First variant gets first palette color
        assert!(
            decoded.contains(&ColorScheme::phosphor().grouped_palette[0]),
            "first variant should get first palette color"
        );
    }

    #[test]
    fn flat_names_produce_single_dataset() {
        // Names without "/" should NOT trigger grouped chart
        let comp = make_comparison(
            "flat",
            vec![make_benchmark("alpha", 10.0), make_benchmark("beta", 20.0)],
            None,
        );
        let config = QuickChartConfig::default();
        let result = build_chart_url(&comp, &config).unwrap();
        let decoded = url_decode_rough(&result.url);
        assert!(
            decoded.contains("\"legend\":{\"display\":false"),
            "flat chart should hide legend"
        );
    }

    /// Rough URL decode for test assertions (only handles %XX).
    fn url_decode_rough(url: &str) -> String {
        let mut out = String::new();
        let bytes = url.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' && i + 2 < bytes.len() {
                let hi = hex_val(bytes[i + 1]);
                let lo = hex_val(bytes[i + 2]);
                if let (Some(h), Some(l)) = (hi, lo) {
                    out.push((h << 4 | l) as char);
                    i += 3;
                    continue;
                }
            }
            out.push(bytes[i] as char);
            i += 1;
        }
        out
    }

    fn hex_val(b: u8) -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'A'..=b'F' => Some(b - b'A' + 10),
            b'a'..=b'f' => Some(b - b'a' + 10),
            _ => None,
        }
    }

    #[test]
    #[ignore] // run with: cargo test print_demo_urls -- --ignored --nocapture
    fn print_demo_urls() {
        // 1. Flat with custom colors
        let flat = make_comparison(
            "JPEG Decode 4K",
            vec![
                make_benchmark("zenjpeg", 12_400_000.0),
                make_benchmark("mozjpeg", 18_600_000.0),
                make_benchmark("libjpeg-turbo", 15_200_000.0),
                make_benchmark("image crate", 31_000_000.0),
            ],
            None,
        );
        let config_colored = QuickChartConfig {
            colors: vec![
                ("mozjpeg".into(), "#ff9800".into()),
                ("libjpeg".into(), "#2196f3".into()),
            ],
            ..Default::default()
        };

        // 2. Throughput chart
        let throughput = make_comparison(
            "PNG Encode",
            vec![
                make_benchmark("zenpng", 8_000_000.0),
                make_benchmark("libpng", 22_000_000.0),
                make_benchmark("image crate", 35_000_000.0),
            ],
            Some(Throughput::Bytes(12_000_000)), // 12 MB image
        );

        // 3. Grouped (matrix)
        let grouped = make_comparison(
            "SrcOver Blend",
            vec![
                make_benchmark("BRAG8/256x256", 1_600_000.0),
                make_benchmark("BRAG8/1024x1024", 20_000_000.0),
                make_benchmark("sw-composite/256x256", 6_000_000.0),
                make_benchmark("sw-composite/1024x1024", 29_000_000.0),
                make_benchmark("naive/256x256", 13_000_000.0),
                make_benchmark("naive/1024x1024", 89_000_000.0),
            ],
            None,
        );

        // 4. Minimal 2-bar
        let minimal = make_comparison(
            "Hash Function",
            vec![make_benchmark("xxhash", 42.0), make_benchmark("fnv", 78.0)],
            None,
        );

        let default_config = QuickChartConfig::default();

        eprintln!("\n=== FLAT (phosphor, custom colors) ===");
        eprintln!("{}\n", build_chart_url(&flat, &config_colored).unwrap().url);
        eprintln!("=== THROUGHPUT (phosphor) ===");
        eprintln!(
            "{}\n",
            build_chart_url(&throughput, &default_config).unwrap().url
        );
        eprintln!("=== GROUPED (phosphor) ===");
        eprintln!(
            "{}\n",
            build_chart_url(&grouped, &default_config).unwrap().url
        );
        eprintln!("=== MINIMAL (phosphor) ===");
        eprintln!(
            "{}\n",
            build_chart_url(&minimal, &default_config).unwrap().url
        );

        // All schemes on the flat chart
        let schemes: &[(&str, ColorScheme)] = &[
            ("ocean", ColorScheme::ocean()),
            ("ember", ColorScheme::ember()),
            ("arctic", ColorScheme::arctic()),
            ("sunset", ColorScheme::sunset()),
            ("mono", ColorScheme::mono()),
        ];
        for (name, scheme) in schemes {
            let cfg = QuickChartConfig::with_scheme(scheme.clone());
            eprintln!("=== FLAT ({name}) ===");
            eprintln!("{}\n", build_chart_url(&flat, &cfg).unwrap().url);
        }

        // All schemes on the grouped chart
        for (name, scheme) in schemes {
            let cfg = QuickChartConfig::with_scheme(scheme.clone());
            eprintln!("=== GROUPED ({name}) ===");
            eprintln!("{}\n", build_chart_url(&grouped, &cfg).unwrap().url);
        }
    }
}
