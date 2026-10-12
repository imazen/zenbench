use std::time::Duration;
use zenbench::{
    Aggregation, BenchmarkResult, ComparisonResult, RoundSample, SuiteResult, Summary,
    aggregate_results,
};

fn population(candidate: f64) -> SuiteResult {
    let mut group = ComparisonResult::default();
    group.group_name = "opposed".into();
    group.completed_rounds = 30;
    for (name, mean) in [("base", 100.0), ("candidate", candidate)] {
        let mut bench = BenchmarkResult::default();
        bench.name = name.into();
        bench.summary = Summary::from_slice(&[mean; 30]);
        group.benchmarks.push(bench);
    }
    let mut sample = RoundSample::default();
    sample.iterations = 1;
    sample.execution_order = vec![0, 1];
    sample.elapsed_ns = vec![100, candidate as u64];
    sample.compensated_ns = sample.elapsed_ns.clone();
    group.samples = vec![sample; 30];
    let analysis = serde_json::from_value(serde_json::json!({
        "baseline": group.benchmarks[0].summary,
        "candidate": group.benchmarks[1].summary,
        "diff": Summary::from_slice(&[candidate - 100.0; 30]),
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
    let mut run = SuiteResult::default();
    run.comparisons.push(group);
    run.total_time = Duration::from_secs(2);
    run.gate_wait_time = Duration::from_millis(200);
    run.gate_waits = 3;
    run
}

#[test]
fn opposed_populations_do_not_inherit_a_significant_first_run() {
    for policy in [Aggregation::Mean, Aggregation::Best, Aggregation::Median] {
        let inputs = vec![population(80.0), population(120.0)];
        let original = serde_json::to_value(&inputs).unwrap();
        let result = aggregate_results(inputs, policy);
        assert!(result.comparisons[0].analyses.is_empty());
        assert_eq!(serde_json::to_value(&result.replicates).unwrap(), original);
        assert_eq!(result.total_time, Duration::from_secs(4));
        assert_eq!(result.gate_wait_time, Duration::from_millis(400));
        assert_eq!(result.gate_waits, 6);
        assert_eq!(result.comparisons[0].completed_rounds, 60);
        assert!(result.comparisons[0].samples.is_empty());
        if matches!(policy, Aggregation::Mean) {
            assert_eq!(result.comparisons[0].benchmarks[0].summary.mean, 100.0);
            assert_eq!(result.comparisons[0].benchmarks[1].summary.mean, 100.0);
        }
    }
}

#[test]
#[should_panic(expected = "different benchmark inventories")]
fn partial_replicates_cannot_silently_disappear() {
    let complete = population(80.0);
    let mut partial = complete.clone();
    partial.comparisons[0].benchmarks.pop();
    aggregate_results(vec![complete, partial], Aggregation::Mean);
}
