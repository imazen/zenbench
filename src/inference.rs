//! Fixed-plan inference across independent processes. No outlier removal or
//! adaptive stopping. Intervals assume independent approximately normal log ratios
//! (paired arms) or log latencies (separate baseline/candidate populations).
use crate::{BenchmarkMode, SuiteResult, Summary};
use serde::{Deserialize, Serialize};
use statrs::distribution::{ContinuousCDF, StudentsT};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Verdict {
    Improvement,
    Regression,
    Equivalent,
    NonInferior,
    Inconclusive,
    Exploratory,
    Invalid,
}

impl std::fmt::Display for Verdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Improvement => "improvement",
            Self::Regression => "regression",
            Self::Equivalent => "equivalent",
            Self::NonInferior => "non_inferior",
            Self::Inconclusive => "inconclusive",
            Self::Exploratory => "exploratory",
            Self::Invalid => "invalid",
        })
    }
}

/// Process-level interval on a geometric latency ratio, expressed in percent.
/// This differs from the descriptive arithmetic means in benchmark rows.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ProcessComparison {
    pub group: String,
    pub baseline: String,
    pub candidate: String,
    pub estimate_pct: f64,
    pub lower_pct: f64,
    pub upper_pct: f64,
    pub verdict: Verdict,
    pub baseline_processes: usize,
    pub candidate_processes: usize,
    pub family_confidence: f64,
    pub margin_pct: f64,
    pub comparisons_in_family: usize,
    pub method: String,
}

pub(crate) fn issues(result: &SuiteResult) -> Vec<String> {
    let mut issues = Vec::new();
    let Some(execution) = &result.execution else {
        return vec!["missing execution contract".into()];
    };
    let plan = &execution.plan;
    if execution.protocol != "zenbench-process-v1" {
        issues.push("unknown execution protocol".into());
    }
    if plan.mode == BenchmarkMode::Iterate {
        issues.push("exploratory mode".into());
    }
    if let Err(e) = plan.validate() {
        issues.push(e);
    }
    if result.incomplete || result.unreliable {
        issues.push("incomplete or unreliable aggregate".into());
    }
    if !execution.independent_processes
        || result.replicates.len() != plan.processes
        || result.replicates.len() < 3
    {
        issues.push("missing planned independent process repetitions".into());
    }
    if result.comparisons.is_empty() {
        issues.push("empty benchmark selection".into());
    }
    if result.testbed.is_none() {
        issues.push("missing testbed identity".into());
    }
    let mut inventory = std::collections::HashSet::new();
    for group in &result.comparisons {
        if !inventory.insert((&group.group_name, None)) {
            issues.push("duplicate group identity".into());
        }
        for bench in &group.benchmarks {
            if !inventory.insert((&group.group_name, Some(&bench.name))) {
                issues.push("duplicate benchmark identity".into());
            }
        }
    }
    let mut ids = std::collections::HashSet::new();
    let signature = workload_signature(result);
    for run in &result.replicates {
        if workload_signature(run) != signature {
            issues.push("replicate workload or sampling contract differs".into());
        }
        if !ids.insert(&run.run_id.0) {
            issues.push("duplicate replicate identity".into());
        }
        if run.incomplete || run.unreliable || !run.replicates.is_empty() {
            issues.push("incomplete, noisy or nested replicate".into());
        }
        match &run.execution {
            Some(e)
                if e.protocol == execution.protocol
                    && e.plan == *plan
                    && e.binary_sha256 == execution.binary_sha256
                    && e.binary_sha256.len() == 64
                    && e.timer_backend == execution.timer_backend => {}
            _ => issues.push("replicate execution identity differs".into()),
        }
        if run.testbed != result.testbed || run.check_cpu_compatibility(result).is_err() {
            issues.push("replicate machine or placement differs".into());
        }
        if run.cpu_placement.as_ref().is_none_or(|p| {
            !matches!(
                p.status,
                crate::platform::PlacementStatus::EnforcedUniform
                    | crate::platform::PlacementStatus::UniformHardware
            )
        }) {
            issues.push("unverified CPU placement".into());
        }
        for group in &run.comparisons {
            let Some(config) = &group.config else {
                issues.push("missing sampling contract".into());
                continue;
            };
            let n = group.benchmarks.len();
            let expected = if group.baseline_only {
                n.saturating_sub(1)
            } else {
                n * n.saturating_sub(1) / 2
            };
            let names: std::collections::HashSet<_> =
                group.benchmarks.iter().map(|b| &b.name).collect();
            let pairs: std::collections::HashSet<_> = group
                .analyses
                .iter()
                .map(|(a, b, _)| if a < b { (a, b) } else { (b, a) })
                .collect();
            if pairs.len() != expected
                || group.analyses.len() != expected
                || group
                    .analyses
                    .iter()
                    .any(|(a, b, _)| a == b || !names.contains(a) || !names.contains(b))
            {
                issues.push("missing or malformed comparison inventory".into());
            }
            if config.auto_rounds
                || config.max_rounds < 2
                || group.completed_rounds != config.max_rounds
                || group.samples.len() != config.max_rounds
            {
                issues.push("adaptive or unfinished sampling plan".into());
            }
            if group
                .benchmarks
                .iter()
                .any(|b| !b.summary.mean.is_finite() || b.summary.mean <= 0.0)
            {
                issues.push("nonpositive or nonfinite measurements".into());
            }
            if group.samples.iter().any(|s| {
                s.iterations == 0
                    || s.elapsed_ns.len() != group.benchmarks.len()
                    || s.compensated_ns.len() != group.benchmarks.len()
                    || {
                        let mut order = s.execution_order.clone();
                        order.sort_unstable();
                        order != (0..group.benchmarks.len()).collect::<Vec<_>>()
                    }
            }) {
                issues.push("malformed paired rounds".into());
            }
            if group.benchmarks.is_empty()
                || group.samples.iter().any(|s| {
                    s.gate_clean != Some(true)
                        || s.gate_observation_age_ns
                            .is_none_or(|age| age > 1_000_000_000)
                })
            {
                issues.push("missing, stale or flagged resource observations".into());
            }
        }
    }
    issues.sort();
    issues.dedup();
    issues
}

pub(crate) fn workload_signature(result: &SuiteResult) -> String {
    let mut groups = result
        .comparisons
        .iter()
        .map(|g| {
            serde_json::json!([
                g.group_name,
                g.benchmarks
                    .iter()
                    .map(|b| (&b.name, &b.tags))
                    .collect::<Vec<_>>(),
                g.throughput,
                g.throughput_unit,
                g.config
            ])
        })
        .collect::<Vec<_>>();
    groups.sort_by_key(|g| g[0].as_str().unwrap_or_default().to_owned());
    serde_json::to_string(&groups).expect("workload identity serializes")
}

pub(crate) fn compatibility_issues(base: &SuiteResult, candidate: &SuiteResult) -> Vec<String> {
    let mut problems = issues(base);
    problems.extend(issues(candidate));
    if base.testbed != candidate.testbed || base.check_cpu_compatibility(candidate).is_err() {
        problems.push("machine or CPU placement differs".into());
    }
    if workload_signature(base) != workload_signature(candidate) {
        problems.push("workload inventory/settings differ".into());
    }
    match (&base.execution, &candidate.execution) {
        (Some(b), Some(c))
            if b.protocol == c.protocol
                && b.plan.workload_id == c.plan.workload_id
                && b.plan.scope == c.plan.scope
                && b.timer_backend == c.timer_backend => {}
        _ => problems.push("execution contract or input identity differs".into()),
    }
    problems.sort();
    problems.dedup();
    problems
}

fn classify(lo: f64, hi: f64, margin: f64) -> Verdict {
    if lo > margin {
        Verdict::Regression
    } else if hi < -margin {
        Verdict::Improvement
    } else if lo >= -margin && hi <= margin {
        Verdict::Equivalent
    } else if hi <= margin {
        Verdict::NonInferior
    } else {
        Verdict::Inconclusive
    }
}

fn interval(
    mean: f64,
    se: f64,
    df: f64,
    confidence: f64,
    family: usize,
    floor: f64,
) -> Option<(f64, f64, f64)> {
    let alpha = (1.0 - confidence) / family.max(1) as f64;
    let critical = StudentsT::new(0.0, 1.0, df)
        .ok()?
        .inverse_cdf(1.0 - alpha / 2.0);
    let width = (critical * se).max(floor);
    let values = (
        100.0 * mean.exp_m1(),
        100.0 * (mean - width).exp_m1(),
        100.0 * (mean + width).exp_m1(),
    );
    (values.0.is_finite() && values.1.is_finite() && values.2.is_finite()).then_some(values)
}

pub(crate) fn paired(
    base: &[f64],
    candidate: &[f64],
    confidence: f64,
    family: usize,
    floor: f64,
) -> Option<(f64, f64, f64)> {
    if base.len() < 3 || base.len() != candidate.len() {
        return None;
    }
    let values: Option<Vec<f64>> = base
        .iter()
        .zip(candidate)
        .map(|(&b, &c)| {
            (b.is_finite() && c.is_finite() && b > 0.0 && c > 0.0).then(|| c.ln() - b.ln())
        })
        .collect();
    let summary = Summary::from_slice(&values?);
    interval(
        summary.mean,
        (summary.variance / summary.n as f64).sqrt(),
        (summary.n - 1) as f64,
        confidence,
        family,
        floor,
    )
}

pub(crate) fn independent(
    base: &[f64],
    candidate: &[f64],
    confidence: f64,
    family: usize,
    floor: f64,
) -> Option<(f64, f64, f64)> {
    if base.len() < 3
        || candidate.len() < 3
        || base
            .iter()
            .chain(candidate)
            .any(|v| !v.is_finite() || *v <= 0.0)
    {
        return None;
    }
    let b = Summary::from_slice(&base.iter().map(|v| v.ln()).collect::<Vec<_>>());
    let c = Summary::from_slice(&candidate.iter().map(|v| v.ln()).collect::<Vec<_>>());
    let bv = b.variance / b.n as f64;
    let cv = c.variance / c.n as f64;
    let denominator = bv * bv / (b.n - 1) as f64 + cv * cv / (c.n - 1) as f64;
    let df = if denominator > 0.0 {
        (bv + cv).powi(2) / denominator
    } else {
        (b.n + c.n - 2) as f64
    };
    interval(
        c.mean - b.mean,
        (bv + cv).sqrt(),
        df,
        confidence,
        family,
        floor,
    )
}

fn means(result: &SuiteResult, group: &str, bench: &str) -> Option<Vec<f64>> {
    result
        .replicates
        .iter()
        .map(|r| {
            let g = r.comparisons.iter().find(|g| g.group_name == group)?;
            let index = g.benchmarks.iter().position(|b| b.name == bench)?;
            let values: Option<Vec<f64>> = g
                .samples
                .iter()
                .map(|s| {
                    (s.iterations > 0)
                        .then(|| {
                            s.compensated_ns
                                .get(index)
                                .map(|&ns| ns as f64 / s.iterations as f64)
                        })
                        .flatten()
                })
                .collect();
            let values = values?;
            (!values.is_empty()).then(|| Summary::from_slice(&values).mean)
        })
        .collect()
}

fn resolution_floor(result: &SuiteResult, group: &str, bench: &str) -> f64 {
    result
        .replicates
        .iter()
        .filter_map(|r| {
            let g = r.comparisons.iter().find(|g| g.group_name == group)?;
            let b = g.benchmarks.iter().find(|b| b.name == bench)?;
            let iterations = g.samples.iter().map(|s| s.iterations).min()?;
            Some(2.0 * r.timer_resolution_ns as f64 / iterations.max(1) as f64 / b.summary.mean)
        })
        .fold(0.0, f64::max)
}

pub(crate) fn confirm(result: &SuiteResult) -> Vec<ProcessComparison> {
    if !issues(result).is_empty() {
        return Vec::new();
    }
    let execution = result.execution.as_ref().unwrap();
    let first = &result.replicates[0];
    let family = first
        .comparisons
        .iter()
        .map(|g| g.analyses.len())
        .sum::<usize>()
        .max(1);
    let mut comparisons = Vec::new();
    for group in &first.comparisons {
        for (base, candidate, _) in &group.analyses {
            let Some(b) = means(result, &group.group_name, base) else {
                continue;
            };
            let Some(c) = means(result, &group.group_name, candidate) else {
                continue;
            };
            let floor = resolution_floor(result, &group.group_name, base)
                + resolution_floor(result, &group.group_name, candidate);
            if let Some((estimate_pct, lower_pct, upper_pct)) =
                paired(&b, &c, execution.plan.confidence, family, floor)
            {
                comparisons.push(ProcessComparison {
                    group: group.group_name.clone(),
                    baseline: base.clone(),
                    candidate: candidate.clone(),
                    estimate_pct,
                    lower_pct,
                    upper_pct,
                    verdict: classify(lower_pct, upper_pct, execution.plan.margin_pct),
                    baseline_processes: b.len(),
                    candidate_processes: c.len(),
                    family_confidence: execution.plan.confidence,
                    margin_pct: execution.plan.margin_pct,
                    comparisons_in_family: family,
                    method: "paired_process_log_ratio_t_v1".into(),
                });
            }
        }
    }
    comparisons
}

pub(crate) fn compare_benchmark(
    base: &SuiteResult,
    candidate: &SuiteResult,
    group: &str,
    bench: &str,
    margin: f64,
    family: usize,
) -> Option<ProcessComparison> {
    let b = means(base, group, bench)?;
    let c = means(candidate, group, bench)?;
    let confidence = candidate.execution.as_ref()?.plan.confidence;
    let floor = resolution_floor(base, group, bench) + resolution_floor(candidate, group, bench);
    let (estimate_pct, lower_pct, upper_pct) = independent(&b, &c, confidence, family, floor)?;
    Some(ProcessComparison {
        group: group.into(),
        baseline: bench.into(),
        candidate: bench.into(),
        estimate_pct,
        lower_pct,
        upper_pct,
        verdict: classify(lower_pct, upper_pct, margin),
        baseline_processes: b.len(),
        candidate_processes: c.len(),
        family_confidence: confidence,
        margin_pct: margin,
        comparisons_in_family: family,
        method: "independent_process_log_latency_welch_v1".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_population(values: &[u64]) -> SuiteResult {
        use crate::{
            BenchmarkPlan, BenchmarkResult, ComparisonResult, ExecutionRecord, RoundSample, RunId,
        };
        let plan = BenchmarkPlan {
            mode: BenchmarkMode::Confirm,
            processes: values.len(),
            workload_id: Some("synthetic-control".into()),
            ..Default::default()
        };
        let placement = crate::platform::CpuPlacement {
            policy: crate::platform::CpuPolicy::Auto,
            topology: crate::platform::CpuTopology {
                kind: crate::platform::TopologyKind::Homogeneous,
                classes: vec![],
                source: "synthetic".into(),
                diagnostic: None,
            },
            status: crate::platform::PlacementStatus::UniformHardware,
            core_class: Some("synthetic".into()),
            effective_cpus: vec![0],
            requested_cpus: None,
            thread_affinity_requested: None,
            thread_affinity_effective: Some(vec![0]),
            enforcement: "none".into(),
        };
        let runs = values
            .iter()
            .enumerate()
            .map(|(i, &value)| {
                let mut group = ComparisonResult {
                    group_name: "synthetic".into(),
                    config: Some(crate::GroupConfig {
                        max_rounds: 2,
                        min_rounds: 2,
                        auto_rounds: false,
                        ..Default::default()
                    }),
                    completed_rounds: 2,
                    cpu_placement: Some(placement.clone()),
                    ..Default::default()
                };
                group.benchmarks.push(BenchmarkResult {
                    name: "work".into(),
                    summary: Summary::from_slice(&[value as f64; 2]),
                    ..Default::default()
                });
                group.samples = vec![
                    RoundSample {
                        iterations: 1,
                        execution_order: vec![0],
                        elapsed_ns: vec![value],
                        compensated_ns: vec![value],
                        gate_clean: Some(true),
                        gate_observation_age_ns: Some(10),
                        gate_observation: Some("synthetic clean observation".into()),
                    };
                    2
                ];
                SuiteResult {
                    run_id: RunId(format!("synthetic-{i}")),
                    comparisons: vec![group],
                    testbed: Some(crate::Testbed {
                        cpu_model: "synthetic".into(),
                        arch: "synthetic".into(),
                        os: "synthetic".into(),
                        logical_cores: 1,
                        physical_cores: 1,
                    }),
                    cpu_placement: Some(placement.clone()),
                    execution: Some(ExecutionRecord {
                        protocol: "zenbench-process-v1".into(),
                        plan: plan.clone(),
                        binary_sha256: "0".repeat(64),
                        process_id: i as u32 + 1,
                        independent_processes: false,
                        timer_backend: "instant".into(),
                    }),
                    ..Default::default()
                }
            })
            .collect();
        let mut result = crate::aggregate_results(runs, crate::Aggregation::Mean);
        result.execution.as_mut().unwrap().independent_processes = true;
        result
    }

    #[test]
    fn promotion_requires_every_case_and_complete_fixed_independent_evidence() {
        let base = synthetic_population(&[100_000, 100_010, 99_990, 100_020, 99_980]);
        assert!(issues(&base).is_empty(), "{:?}", issues(&base));
        assert!(crate::baseline::compare_against_baseline(&base, &base, 1.0).promotable);
        let noisy = synthetic_population(&[60_000, 80_000, 100_000, 120_000, 140_000]);
        let diff = crate::baseline::compare_against_baseline(&base, &noisy, 1.0);
        assert!(!diff.promotable);
        assert_eq!(diff.confirmed[0].verdict, Verdict::Inconclusive);
        let mut missing = base.clone();
        missing.comparisons.clear();
        assert!(!crate::baseline::compare_against_baseline(&base, &missing, 1.0).promotable);
        for change in 0..6 {
            let mut invalid = base.clone();
            match change {
                0 => invalid.unreliable = true,
                1 => invalid.replicates.pop().map(|_| ()).unwrap(),
                2 => invalid.replicates[1].run_id = invalid.replicates[0].run_id.clone(),
                3 => invalid.replicates[0].comparisons[0].samples[0].gate_clean = None,
                4 => {
                    invalid.replicates[0].comparisons[0]
                        .config
                        .as_mut()
                        .unwrap()
                        .auto_rounds = true
                }
                5 => invalid.incomplete = true,
                _ => unreachable!(),
            }
            let diff = crate::baseline::compare_against_baseline(&base, &invalid, 1.0);
            assert!(!diff.promotable, "mutation {change}");
            assert!(diff.confirmed.is_empty(), "mutation {change}");
        }
    }

    #[test]
    fn fixed_plan_coverage_and_excluded_noise_models_match_scipy() {
        // Same deterministic inputs as examples/coverage_oracle.py; the expected
        // counts come from SciPy's t quantiles, independently of statrs.
        struct Noise(u64);
        impl Noise {
            fn uniform(&mut self) -> f64 {
                self.0 = self
                    .0
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                ((self.0 >> 11) as f64 + 0.5) / ((1u64 << 53) as f64)
            }
            fn normal(&mut self) -> f64 {
                (-2.0 * self.uniform().ln()).sqrt() * (std::f64::consts::TAU * self.uniform()).cos()
            }
        }
        let oracle: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/process_coverage.json")).unwrap();
        let mut rng = Noise(oracle["seed"].as_u64().unwrap());
        let excludes = |values: &[f64], truth: f64| {
            let base = vec![1.0; values.len()];
            let candidate: Vec<_> = values.iter().map(|v| v.exp()).collect();
            let (_, lo, hi) = paired(&base, &candidate, 0.95, 1, 0.0).unwrap();
            let truth_pct = 100.0 * truth.exp_m1();
            truth_pct < lo || truth_pct > hi
        };
        let mut counts = [0usize; 5];
        for _ in 0..oracle["trials"].as_u64().unwrap() {
            let values: Vec<_> = (0..20).map(|_| 0.03 * rng.normal()).collect();
            counts[0] += usize::from(excludes(&values[..5], 0.0));
            let shifted: Vec<_> = values[..5].iter().map(|v| v + 1.1_f64.ln()).collect();
            counts[1] += usize::from(excludes(&shifted, 1.1_f64.ln()));
            counts[2] += usize::from(excludes(&shifted, 0.0));
            let mut correlated = vec![values[0]];
            for v in &values[1..5] {
                correlated
                    .push(0.9 * correlated.last().unwrap() + (1.0 - 0.9_f64.powi(2)).sqrt() * v);
            }
            counts[3] += usize::from(excludes(&correlated, 0.0));
            counts[4] += usize::from((3..=20).any(|n| excludes(&values[..n], 0.0)));
        }
        for (name, count) in [
            "null",
            "known_effect_misses",
            "known_effect_detected",
            "correlated",
            "optional_stopping",
        ]
        .into_iter()
        .zip(counts)
        {
            assert_eq!(
                count as u64,
                oracle["counts"][name].as_u64().unwrap(),
                "{name}"
            );
        }
    }

    #[test]
    fn paired_and_welch_intervals_match_independent_scipy_oracle() {
        #[derive(Deserialize)]
        struct Case {
            base: Vec<f64>,
            candidate: Vec<f64>,
            family: usize,
            paired: [f64; 3],
            independent: [f64; 3],
        }
        #[derive(Deserialize)]
        struct Oracle {
            cases: Vec<Case>,
        }
        let oracle: Oracle =
            serde_json::from_str(include_str!("../tests/fixtures/process_intervals.json")).unwrap();
        for case in oracle.cases {
            for (actual, expected) in [
                (
                    paired(&case.base, &case.candidate, 0.95, case.family, 0.0).unwrap(),
                    case.paired,
                ),
                (
                    independent(&case.base, &case.candidate, 0.95, case.family, 0.0).unwrap(),
                    case.independent,
                ),
            ] {
                for (a, e) in [actual.0, actual.1, actual.2].into_iter().zip(expected) {
                    assert!(
                        (a - e).abs() < 1e-8 * e.abs().max(1.0),
                        "actual={a}, SciPy={e}"
                    );
                }
            }
        }
    }

    #[test]
    fn insufficient_nonfinite_or_nonpositive_data_cannot_confirm() {
        assert!(paired(&[100.0; 2], &[110.0; 2], 0.95, 1, 0.0).is_none());
        assert!(paired(&[0.0; 5], &[110.0; 5], 0.95, 1, 0.0).is_none());
        assert!(independent(&[f64::NAN; 5], &[110.0; 5], 0.95, 1, 0.0).is_none());
        assert!(independent(&[100.0; 5], &[-1.0; 5], 0.95, 1, 0.0).is_none());
    }

    #[test]
    fn inconclusive_is_not_equivalence_or_permission_to_promote() {
        assert_eq!(classify(-5.0, 5.0, 1.0), Verdict::Inconclusive);
        assert_eq!(classify(-0.5, 0.5, 1.0), Verdict::Equivalent);
        assert_eq!(classify(-5.0, 0.5, 1.0), Verdict::NonInferior);
        assert_eq!(classify(2.0, 5.0, 1.0), Verdict::Regression);
        assert_eq!(classify(-5.0, -2.0, 1.0), Verdict::Improvement);
    }
}
