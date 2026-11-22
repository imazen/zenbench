//! Descriptive linear fits over complete cases in a single retained packet.
use super::{Check, Packet, case_complete};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

#[derive(Clone, Default, Ord, PartialOrd, Eq, PartialEq, Serialize)]
struct Factors {
    fields: BTreeMap<String, String>,
    inputs: BTreeMap<String, PathBuf>,
}

struct CasePoint<'a> {
    id: &'a str,
    x: f64,
    means: Vec<f64>,
}

pub(super) fn analyze(
    packet: &Packet,
    axis: &str,
    unit: &str,
    vary_inputs: &[String],
) -> Result<Value, String> {
    if axis.trim().is_empty() || unit.trim().is_empty() {
        return Err("axis and axis unit are required".into());
    }
    if matches!(packet.plan.check, Check::Unchecked { .. }) {
        return Err("case fits require a recorded correctness check".into());
    }
    let varying: BTreeSet<_> = vary_inputs.iter().cloned().collect();
    if varying.len() != vary_inputs.len() {
        return Err("duplicate --vary-input name".into());
    }
    let arms: Vec<_> = packet.plan.arms().map(|arm| &arm.id).collect();
    let mut strata = BTreeMap::<Factors, Vec<CasePoint<'_>>>::new();
    let mut excluded = Vec::new();
    let mut inventory = BTreeMap::<Factors, Vec<f64>>::new();
    for (index, case) in packet.plan.cases.iter().enumerate() {
        let mut fields = case.fields.clone();
        let x = fields
            .remove(axis)
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|value| value.is_finite())
            .ok_or_else(|| format!("case {:?} lacks finite numeric field {axis:?}", case.id))?;
        let mut inputs = case.inputs.clone();
        for name in &varying {
            if inputs.remove(name).is_none() {
                return Err(format!("case {:?} lacks varying input {name:?}", case.id));
            }
        }
        let factors = Factors { fields, inputs };
        let xs = inventory.entry(factors.clone()).or_default();
        if xs.contains(&x) {
            return Err(format!(
                "duplicate {axis:?} coordinate {x} in case {:?} within one stratum",
                case.id
            ));
        }
        xs.push(x);
        if !case_complete(packet, index) {
            excluded.push(json!({"case":case.id,"axis_value":x,"factors":factors,
                "reason":"case lacks complete blocks or passed correctness"}));
            continue;
        }
        let mut means = vec![0.0; arms.len()];
        let mut counts = vec![0; arms.len()];
        for invocation in packet.invocations.iter().filter(|row| row.case == index) {
            if !matches!(packet.plan.check, Check::Command { .. })
                && invocation.correctness.is_none()
            {
                return Err(format!(
                    "case {:?} has no retained correctness evidence",
                    case.id
                ));
            }
            if invocation.arm < arms.len() {
                let arm = invocation.arm;
                counts[arm] += 1;
                let average = mean(&invocation.values_ns);
                means[arm] += (average - means[arm]) / f64::from(counts[arm]);
            }
        }
        strata.entry(factors).or_default().push(CasePoint {
            id: &case.id,
            x,
            means,
        });
    }
    let mut fits = Vec::new();
    for (factors, mut points) in strata {
        points.sort_by(|a, b| a.x.total_cmp(&b.x));
        if points.len() < 2 {
            excluded.push(
                json!({"case":points[0].id,"axis_value":points[0].x,"factors":factors,
                "reason":"fewer than two complete coordinates in stratum"}),
            );
            continue;
        }
        for (arm, name) in arms.iter().enumerate() {
            let data: Vec<_> = points
                .iter()
                .map(|point| (point.x, point.means[arm]))
                .collect();
            let (intercept, slope, residuals) = line(&data).ok_or_else(|| {
                format!("fit for arm {name:?} has unrepresentable coefficients or residuals")
            })?;
            let cases: Vec<_> = points.iter().zip(residuals).map(|(p, residual)| {
                json!({"case":p.id,"axis_value":p.x,"mean_ns":p.means[arm],"residual_ns":residual})
            }).collect();
            fits.push(
                json!({"arm":name,"factors":factors,"intercept_ns":intercept,
                "slope_ns_per_axis_unit":slope,"cases":cases}),
            );
        }
    }
    if fits.is_empty() {
        return Err(format!(
            "no stratum has two complete coordinates; excluded cases: {excluded:?}"
        ));
    }
    Ok(
        json!({"kind":"zenbench_pair_fit","schema":1,"fingerprint":packet.fingerprint,
        "axis":axis,"axis_unit":unit,"vary_inputs":varying,"fits":fits,"excluded_cases":excluded,
        "inference":"descriptive least squares on equally weighted case means from one packet"}),
    )
}

fn mean(values: &[f64]) -> f64 {
    values.iter().enumerate().fold(0.0, |mean, (i, value)| {
        mean + (value - mean) / (i + 1) as f64
    })
}

fn line(points: &[(f64, f64)]) -> Option<(f64, f64, Vec<f64>)> {
    let x_scale = points.iter().map(|(x, _)| x.abs()).fold(0.0, f64::max);
    let y_scale = points.iter().map(|(_, y)| y.abs()).fold(0.0, f64::max);
    let xs: Vec<_> = points.iter().map(|(x, _)| x / x_scale).collect();
    let ys: Vec<_> = points.iter().map(|(_, y)| y / y_scale).collect();
    let (x_mean, y_mean) = (mean(&xs), mean(&ys));
    let (xx, xy) = xs.iter().zip(&ys).fold((0.0, 0.0), |(xx, xy), (x, y)| {
        let dx = x - x_mean;
        (xx + dx * dx, xy + dx * (y - y_mean))
    });
    let scaled_slope = xy / xx;
    let intercept = (-scaled_slope).mul_add(x_mean, y_mean) * y_scale;
    let slope = (scaled_slope * y_scale) / x_scale;
    let residuals: Vec<_> = xs
        .iter()
        .zip(ys)
        .map(|(x, y)| (-scaled_slope).mul_add(x - x_mean, y - y_mean) * y_scale)
        .collect();
    (intercept.is_finite() && slope.is_finite() && residuals.iter().all(|r| r.is_finite()))
        .then_some((intercept, slope, residuals))
}

#[cfg(test)]
mod tests {
    use super::line;

    #[test]
    fn fits_intercept_slope_and_residuals() {
        let (intercept, slope, residuals) =
            line(&[(0.0, 7.0), (1.0, 9.0), (3.0, 13.0), (4.0, 15.0)]).unwrap();
        assert!((intercept - 7.0).abs() < 1e-12);
        assert!((slope - 2.0).abs() < 1e-12);
        assert!(residuals.iter().all(|r| r.abs() < 1e-12));
        assert!(line(&[(1.0, 7.0), (1.0, 8.0)]).is_none());
    }
}
