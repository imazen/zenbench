//! Fixed experiment inference. Internal repetitions never become independent rows.
use serde_json::{Value, json};
use statrs::distribution::{ContinuousCDF, StudentsT};

pub(super) fn mean(values: &[f64]) -> f64 {
    let origin = values.first().copied().unwrap_or(0.0);
    origin
        + values
            .iter()
            .map(|x| (x - origin) / values.len() as f64)
            .sum::<f64>()
}
fn median(values: &[f64]) -> f64 {
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n % 2 == 0 {
        v[n / 2 - 1] / 2.0 + v[n / 2] / 2.0
    } else {
        v[n / 2]
    }
}
pub(super) fn describe(values: &[f64]) -> Value {
    if values.is_empty() {
        return Value::Null;
    }
    let med = median(values);
    json!({"n":values.len(), "mean":mean(values), "median":med,
        "min":values.iter().copied().reduce(f64::min),
        "mad":median(&values.iter().map(|x| (x-med).abs()).collect::<Vec<_>>())})
}

/// Student-t interval on log ratios, widened by bounded measurement error.
/// `resolution` is an absolute bound per invocation mean, not an SE. Repeating
/// a quantized measurement does not divide this bound by sqrt(n).
pub(super) fn interval(
    base: &[f64],
    arm: &[f64],
    resolution: f64,
    confidence: f64,
    family: usize,
) -> Option<Value> {
    if base.len() < 2
        || base.len() != arm.len()
        || family == 0
        || !resolution.is_finite()
        || resolution <= 0.0
        || !confidence.is_finite()
        || !(0.5..1.0).contains(&confidence)
        || base
            .iter()
            .chain(arm)
            .any(|v| !v.is_finite() || *v <= resolution)
    {
        return None;
    }
    let logs: Vec<_> = base.iter().zip(arm).map(|(b, a)| a.ln() - b.ln()).collect();
    let avg = mean(&logs);
    let n = logs.len() as f64;
    let sd = (logs.iter().map(|x| (x - avg).powi(2)).sum::<f64>() / (n - 1.0)).sqrt();
    let critical = StudentsT::new(0.0, 1.0, n - 1.0)
        .ok()?
        .inverse_cdf(1.0 - (1.0 - confidence) / (2.0 * family as f64));
    // Exact extremal log ratios when each invocation lies in [x-r, x+r].
    // Add systematic error to statistical uncertainty, never take their max.
    let lower_error = mean(
        &base
            .iter()
            .zip(arm)
            .map(|(b, a)| -(-resolution / a).ln_1p() + (resolution / b).ln_1p())
            .collect::<Vec<_>>(),
    );
    let upper_error = mean(
        &base
            .iter()
            .zip(arm)
            .map(|(b, a)| (resolution / a).ln_1p() - (-resolution / b).ln_1p())
            .collect::<Vec<_>>(),
    );
    let width = critical * sd / n.sqrt();
    let lower = (avg - width - lower_error).exp_m1() * 100.0;
    let upper = (avg + width + upper_error).exp_m1() * 100.0;
    let estimate = avg.exp_m1() * 100.0;
    if [lower, upper, estimate, width]
        .iter()
        .any(|v| !v.is_finite())
    {
        return None;
    }
    Some(
        json!({"estimate_pct":estimate,"lower_pct":lower,"upper_pct":upper,
        "family_confidence":confidence,"comparisons_in_family":family,
        "blocks":base.len(),"df":base.len()-1,"log_ratio_sd":sd,
        "resolution_lower_log":lower_error,"resolution_upper_log":upper_error,
        "method":"paired-block-log-t-bonferroni-bounded-resolution-v1"}),
    )
}
pub(super) fn classify(interval: &Value, margin: f64) -> &'static str {
    let (Some(lo), Some(hi)) = (
        interval["lower_pct"].as_f64(),
        interval["upper_pct"].as_f64(),
    ) else {
        return "invalid";
    };
    if lo > margin {
        "regression"
    } else if hi < -margin {
        "improvement"
    } else if lo >= -margin && hi <= margin {
        "equivalent"
    } else if hi <= margin {
        "non_inferior"
    } else {
        "inconclusive"
    }
}
