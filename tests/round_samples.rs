use std::time::Duration;
use zenbench::{Aggregation, GateConfig, SuiteResult, aggregate_results, black_box, run_gated};

#[test]
fn engine_retains_paired_batches_and_round_trips_evidence() {
    let result = run_gated(GateConfig::disabled(), |suite| {
        suite.compare("retained", |group| {
            let config = group.config();
            config
                .max_rounds(10)
                .min_rounds(10)
                .auto_rounds(false)
                .warmup_time(Duration::ZERO)
                .linear_sampling(true);
            config.min_iterations = 10;
            config.max_iterations = 10;
            group.bench("a", |b| {
                b.iter(|| black_box(17u64).wrapping_mul(black_box(13)))
            });
            group.bench("b", |b| {
                b.iter(|| black_box(31u64).wrapping_mul(black_box(19)))
            });
        });
    });
    let cmp = &result.comparisons[0];
    assert_eq!(cmp.completed_rounds, 10);
    assert_eq!(cmp.samples.len(), cmp.completed_rounds);
    assert_ne!(cmp.samples[0].iterations, cmp.samples[9].iterations);
    assert!(
        cmp.samples
            .windows(2)
            .any(|pair| pair[0].execution_order != pair[1].execution_order)
    );
    for sample in &cmp.samples {
        assert!(sample.iterations > 0);
        let mut order = sample.execution_order.clone();
        order.sort_unstable();
        assert_eq!(order, vec![0, 1]);
        assert_eq!(sample.elapsed_ns.len(), 2);
        assert_eq!(sample.compensated_ns.len(), 2);
        for (&raw, &compensated) in sample.elapsed_ns.iter().zip(&sample.compensated_ns) {
            let overhead = (result.loop_overhead_ns * sample.iterations as f64) as u64;
            assert_eq!(compensated, raw.saturating_sub(overhead).max(1));
        }
    }
    for (index, bench) in cmp.benchmarks.iter().enumerate() {
        let mean = cmp
            .samples
            .iter()
            .map(|s| s.compensated_ns[index] as f64 / s.iterations as f64)
            .sum::<f64>()
            / cmp.samples.len() as f64;
        assert_eq!(bench.summary.n, cmp.samples.len());
        assert!((mean - bench.summary.mean).abs() <= 1e-10 * mean.max(1.0));
    }
    let bytes = serde_json::to_vec(&result).unwrap();
    let restored: SuiteResult = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(restored.comparisons[0].samples, cmp.samples);

    let mut legacy = serde_json::to_value(&result).unwrap();
    legacy["comparisons"][0]
        .as_object_mut()
        .unwrap()
        .remove("samples");
    let restored: SuiteResult = serde_json::from_value(legacy).unwrap();
    assert!(restored.comparisons[0].samples.is_empty());

    let single = aggregate_results(vec![result.clone()], Aggregation::Mean);
    assert_eq!(single.comparisons[0].samples, cmp.samples);
    for policy in [Aggregation::Best, Aggregation::Mean, Aggregation::Median] {
        let aggregate = aggregate_results(vec![result.clone(), result.clone()], policy);
        assert!(aggregate.comparisons[0].samples.is_empty());
    }
}

#[test]
fn single_iteration_rounds_are_not_warmup_or_batch_averages() {
    let result = run_gated(GateConfig::disabled(), |suite| {
        suite.compare("individual", |group| {
            let config = group.config();
            config
                .max_rounds(3)
                .min_rounds(3)
                .auto_rounds(false)
                .warmup_time(Duration::ZERO);
            config.min_iterations = 1;
            config.max_iterations = 1;
            group.bench("a", |b| b.iter(|| black_box(47u64)));
        });
    });
    assert_eq!(result.comparisons[0].samples.len(), 3);
    for sample in &result.comparisons[0].samples {
        assert_eq!(sample.iterations, 1);
        assert_eq!(sample.execution_order, vec![0]);
        assert_eq!(sample.elapsed_ns.len(), 1);
    }
}
