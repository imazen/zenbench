//! Compile and execute the published 0.1.10 construction/unwind contracts.
use zenbench::{GateConfig, ResourceGate, baseline::BaselineComparison};

#[test]
fn published_baseline_result_remains_exhaustively_constructible() {
    let comparison = BaselineComparison {
        benchmarks: Vec::new(),
        regressions: 0,
        improvements: 0,
        unchanged: 0,
        new_benchmarks: Vec::new(),
        missing_benchmarks: Vec::new(),
        warnings: Vec::new(),
    };
    let BaselineComparison {
        benchmarks,
        regressions,
        improvements,
        unchanged,
        new_benchmarks,
        missing_benchmarks,
        warnings,
    } = comparison;
    assert!(
        benchmarks.is_empty()
            && new_benchmarks.is_empty()
            && missing_benchmarks.is_empty()
            && warnings.is_empty()
    );
    assert_eq!((regressions, improvements, unchanged), (0, 0, 0));
}

#[test]
fn published_gate_supports_unwind_boundaries() {
    fn require_bounds<T: std::panic::UnwindSafe + std::panic::RefUnwindSafe>() {}
    require_bounds::<ResourceGate>();
    let gate = ResourceGate::new(GateConfig::default());
    assert!(std::panic::catch_unwind(|| drop(gate)).is_ok());
}
