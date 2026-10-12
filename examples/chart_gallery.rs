//! Generate a chart style gallery from explicitly synthetic data; no benchmarks run.
//! cargo run --features charts --example chart_gallery -- NEW_DIRECTORY
use std::path::PathBuf;
use zenbench::charts::{ChartConfig, ChartOrientation};
use zenbench::{BenchmarkResult, ComparisonResult, RunId, SuiteResult, Summary};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("provide a new output directory")?,
    );
    std::fs::create_dir(&output)?;
    let mut result = SuiteResult::default();
    result.run_id = RunId("illustrative-data-not-a-benchmark".into());
    let mut group = ComparisonResult::default();
    group.group_name = "Illustrative data — latency".into();
    for (name, mean) in [
        ("baseline/256 items", 1200.0),
        ("candidate/256 items", 900.0),
        ("baseline/1024 items", 4800.0),
        ("candidate/1024 items", 3200.0),
    ] {
        let mut benchmark = BenchmarkResult::default();
        benchmark.name = name.into();
        benchmark.summary = Summary::from_slice(&[mean * 0.95, mean, mean * 1.05]);
        group.benchmarks.push(benchmark);
    }
    result.comparisons.push(group);
    result.save(output.join("illustrative.json.gz"))?;
    for (theme, orientation, directory) in [
        ("light", ChartOrientation::Horizontal, "light"),
        ("dark", ChartOrientation::Horizontal, "dark"),
        ("light", ChartOrientation::Vertical, "vertical"),
    ] {
        result.save_publication_charts(
            output.join(directory),
            &ChartConfig {
                theme: theme.into(),
                orientation,
                ..Default::default()
            },
        )?;
    }
    result.save_charts(output.join("adaptive"))?;
    std::fs::write(output.join("report.html"), result.to_html())?;
    Ok(())
}
