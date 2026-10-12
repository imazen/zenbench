#[allow(dead_code)]
#[path = "../src/bin/support/pair/interval.rs"]
mod interval;
mod tests {
    use super::interval::*;
    use serde_json::Value;
    #[test]
    fn independent_scipy_intervals() {
        let oracle: Value =
            serde_json::from_str(include_str!("fixtures/pair_intervals.json")).unwrap();
        for case in oracle["cases"].as_array().unwrap() {
            let v = |key| {
                case[key]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|x| x.as_f64().unwrap())
                    .collect::<Vec<_>>()
            };
            let got = interval(
                &v("base"),
                &v("arm"),
                case["resolution"].as_f64().unwrap(),
                0.95,
                case["family"].as_u64().unwrap() as usize,
            )
            .unwrap();
            for key in ["estimate_pct", "lower_pct", "upper_pct"] {
                let expected = case[key].as_f64().unwrap();
                assert!(
                    (got[key].as_f64().unwrap() - expected).abs() < 1e-8,
                    "{key}: {got} vs {case}"
                );
            }
        }
    }
    #[test]
    fn resolution_is_not_independent_noise() {
        let two = interval(&[100.0; 2], &[100.0; 2], 1.0, 0.95, 2).unwrap();
        let hundred = interval(&[100.0; 100], &[100.0; 100], 1.0, 0.95, 2).unwrap();
        assert_eq!(two["lower_pct"], hundred["lower_pct"]);
        assert_eq!(two["upper_pct"], hundred["upper_pct"]);
        assert_eq!(classify(&two, 1.0), "inconclusive");
        assert!(interval(&[1.0; 5], &[100.0; 5], 1.0, 0.95, 2).is_none());
    }
}
