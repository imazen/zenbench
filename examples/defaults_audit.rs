//! Local cost and aggregation audit; not a proposed measurement preset.
use std::hint::black_box;
use std::time::Duration;
use zenbench::{Aggregation, Suite, SuiteResult};

fn number(name: &str, fallback: usize) -> usize {
    std::env::var(name).map_or(fallback, |v| v.parse().expect("integer audit setting"))
}

fn register(suite: &mut Suite) {
    let arms = number("AUDIT_ARMS", 2);
    let words = number("AUDIT_WORDS", 1024);
    for index in 0..number("AUDIT_GROUPS", 1) {
        suite.group(format!("audit-{index}"), |g| {
            if let Ok(ms) = std::env::var("AUDIT_WARMUP_MS") {
                g.config()
                    .warmup_time(Duration::from_millis(ms.parse().unwrap()));
            }
            if let Ok(rounds) = std::env::var("AUDIT_ROUNDS") {
                g.config()
                    .auto_rounds(false)
                    .max_rounds(rounds.parse().unwrap());
            }
            for arm in 0..arms {
                let data: Vec<u64> = (0..words as u64).map(|n| n.wrapping_mul(17)).collect();
                g.bench(format!("identical-{arm}"), move |b| {
                    b.iter(|| {
                        black_box(&data)
                            .iter()
                            .fold(0_u64, |a, v| a.wrapping_add(*v).rotate_left(7))
                    });
                });
            }
        });
    }
}

fn fixture() {
    // Synthetic populations with opposing effects, not measured timings.
    let run = |candidate: f64| {
        let mut result = SuiteResult::default();
        let mut group = zenbench::ComparisonResult::default();
        group.group_name = "synthetic".into();
        for (name, mean) in [("base", 100.0), ("candidate", candidate)] {
            let mut bench = zenbench::BenchmarkResult::default();
            bench.name = name.into();
            bench.summary = zenbench::Summary::from_slice(&[mean; 30]);
            group.benchmarks.push(bench);
        }
        let analysis = serde_json::from_value(serde_json::json!({
            "baseline": group.benchmarks[0].summary,
            "candidate": group.benchmarks[1].summary,
            "diff": zenbench::Summary::from_slice(&[candidate - 100.0; 30]),
            "pct_change": candidate - 100.0, "significant": true,
            "cohens_d": 0.0, "n_samples": 30, "n_outliers": 0,
            "ci_lower": candidate - 100.0, "ci_median": candidate - 100.0,
            "ci_upper": candidate - 100.0, "drift_correlation": 0.0,
            "wilcoxon_p": 0.0, "resolution_limited": false
        }))
        .unwrap();
        group
            .analyses
            .push(("base".into(), "candidate".into(), analysis));
        result.comparisons.push(group);
        result
    };
    let result = zenbench::aggregate_results(vec![run(80.0), run(120.0)], Aggregation::Mean);
    let group = &result.comparisons[0];
    println!(
        "{}",
        serde_json::json!({
            "kind": "synthetic_aggregation_counterexample",
            "input_effects_pct": [-20, 20],
            "aggregate_base_mean": group.benchmarks[0].summary.mean,
            "aggregate_candidate_mean": group.benchmarks[1].summary.mean,
            "reported_analysis_pct": group.analyses.first().map(|a| a.2.pct_change),
            "reported_analysis_significant": group.analyses.first().map(|a| a.2.significant),
            "retained_replicates": result.replicates.len(),
            "retained_rounds": group.samples.len()
        })
    );
}

fn main() {
    if std::env::args().any(|a| a == "--fixture") {
        fixture();
        return;
    }
    let result = match zenbench::parse_process_args() {
        Some((n, policy)) => zenbench::run_processes(n, policy),
        None => zenbench::run(register),
    };
    if std::env::var_os("ZENBENCH_SUBPROCESS").is_some() {
        if let Some(dir) = std::env::var_os("AUDIT_CHILD_ARCHIVE") {
            let dir = std::path::PathBuf::from(dir);
            std::fs::create_dir_all(&dir).unwrap();
            result
                .save(dir.join(format!("{}.json", result.run_id)))
                .unwrap();
        }
    }
    zenbench::postprocess_result(&result);
}
