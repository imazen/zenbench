use super::plan::{Extract, Metric};

pub(super) fn extract(metric: &Metric, stdout: &str, wall_ns: f64) -> Result<Vec<f64>, String> {
    let mut values = Vec::new();
    let number = |s: &str| {
        s.trim()
            .parse::<f64>()
            .map_err(|_| format!("invalid metric {s:?}"))
    };
    match &metric.extract {
        Extract::Wall => values.push(wall_ns),
        Extract::Prefix { prefix } => {
            for line in stdout.lines().filter_map(|s| s.strip_prefix(prefix)) {
                values.push(number(line)?);
            }
        }
        Extract::Tsv { prefix, column } => {
            for line in stdout.lines().filter(|s| s.starts_with(prefix)) {
                values.push(number(
                    line.split('\t')
                        .nth(*column)
                        .ok_or("missing TSV metric column")?,
                )?);
            }
        }
        Extract::Json { prefix, pointer } => {
            for line in stdout.lines().filter_map(|s| s.strip_prefix(prefix)) {
                let record: serde_json::Value =
                    serde_json::from_str(line).map_err(|e| e.to_string())?;
                values.push(
                    record
                        .pointer(pointer)
                        .and_then(|v| v.as_f64())
                        .ok_or("missing/nonnumeric JSON metric field")?,
                );
            }
        }
        Extract::Regex { pattern, capture } => {
            let re = regex::Regex::new(pattern).map_err(|e| e.to_string())?;
            for record in re.captures_iter(stdout) {
                values.push(number(
                    record
                        .get(*capture)
                        .ok_or("missing regex capture")?
                        .as_str(),
                )?);
            }
        }
    }
    if values.len() != metric.expected {
        return Err(format!(
            "metric cardinality: expected {}, found {}",
            metric.expected,
            values.len()
        ));
    }
    let scale = metric.scale()?;
    for value in &mut values {
        *value *= scale;
        if !value.is_finite() || *value <= 0.0 {
            return Err("metric values must be finite and positive after normalization".into());
        }
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_adapters_preserve_vectors_and_units() {
        for (adapter, text) in [
            (
                Extract::Prefix {
                    prefix: "time=".into(),
                },
                "info\ntime=2\ntime=4",
            ),
            (
                Extract::Tsv {
                    prefix: "time\t".into(),
                    column: 1,
                },
                "info\ntime\t2\ntime\t4",
            ),
            (
                Extract::Json {
                    prefix: "M ".into(),
                    pointer: "/time".into(),
                },
                "info\nM {\"time\":2}\nM {\"time\":4}",
            ),
            (
                Extract::Regex {
                    pattern: r"time=(\d+)".into(),
                    capture: 1,
                },
                "info time=2 time=4",
            ),
        ] {
            let metric = Metric {
                extract: adapter,
                unit: "ms".into(),
                expected: 2,
                divide_by: 2.0,
            };
            assert_eq!(extract(&metric, text, 999.0).unwrap(), vec![1e6, 2e6]);
            assert!(extract(&metric, "", 999.0).is_err());
        }
    }
    #[test]
    fn invalid_samples_and_extra_metrics_never_fall_back_to_wall_time() {
        let metric = Metric {
            extract: Extract::Prefix {
                prefix: "t=".into(),
            },
            unit: "ns".into(),
            expected: 1,
            divide_by: 1.0,
        };
        for text in ["t=NaN", "t=inf", "t=0", "t=-1", "t=bad", "t=1\nt=2"] {
            assert!(extract(&metric, text, 100.0).is_err(), "{text}");
        }
    }
}
