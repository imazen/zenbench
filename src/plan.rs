//! Explicit measurement intent. Rigor and CPU-class selection are separate.
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Local exploration, fixed independent confirmation, or one cell of an evidence matrix.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum BenchmarkMode {
    #[default]
    Iterate,
    Confirm,
    Evidence,
}

impl std::fmt::Display for BenchmarkMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Iterate => "iterate",
            Self::Confirm => "confirm",
            Self::Evidence => "evidence",
        })
    }
}

impl std::str::FromStr for BenchmarkMode {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "iterate" => Ok(Self::Iterate),
            "confirm" => Ok(Self::Confirm),
            "evidence" => Ok(Self::Evidence),
            _ => Err(format!(
                "unknown benchmark mode {value:?}; expected iterate, confirm or evidence"
            )),
        }
    }
}

/// A recorded, bounded experiment plan. Confirmation never adapts its stopping
/// rule to the observed effect. CPU placement is configured separately on Suite.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct BenchmarkPlan {
    pub mode: BenchmarkMode,
    /// Independent child processes for Confirm/Evidence; Iterate stays one process.
    pub processes: usize,
    /// Optional crate-wide round count; otherwise each group's explicit count wins.
    pub rounds: Option<usize>,
    /// Practical margin as a percentage (1.0 = one percent).
    pub margin_pct: f64,
    /// Family confidence across the comparisons in this suite.
    pub confidence: f64,
    /// Input/workload identity supplied by the benchmark owner; required for confirmation.
    pub workload_id: Option<String>,
    /// Named matrix cell/scope, required for Evidence. It does not certify other cells.
    pub scope: Option<String>,
    /// Total supervisor deadline for fresh child processes.
    pub timeout: Duration,
}

impl Default for BenchmarkPlan {
    fn default() -> Self {
        Self {
            mode: BenchmarkMode::Iterate,
            processes: 5,
            rounds: None,
            margin_pct: 1.0,
            confidence: 0.95,
            workload_id: None,
            scope: None,
            timeout: Duration::from_secs(300),
        }
    }
}

impl BenchmarkPlan {
    pub(crate) fn resolve(mut self) -> Result<Self, String> {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let value = |flag: &str, env: &str| -> Result<Option<String>, String> {
            let prefix = format!("--{flag}=");
            let values: Vec<_> = args
                .iter()
                .filter_map(|a| a.strip_prefix(&prefix))
                .collect();
            if values.len() > 1 {
                return Err(format!("--{flag} specified more than once"));
            }
            Ok(values
                .first()
                .map(|v| v.to_string())
                .or_else(|| std::env::var(env).ok()))
        };
        if let Some(v) = value("mode", "ZENBENCH_MODE")? {
            self.mode = v.parse()?;
        }
        if let Some(v) = value("processes", "ZENBENCH_PROCESSES")? {
            self.processes = v.parse().map_err(|_| "invalid process count")?;
        }
        if let Some(v) = value("rounds", "ZENBENCH_ROUNDS")? {
            self.rounds = Some(v.parse().map_err(|_| "invalid round count")?);
        }
        if let Some(v) = value("margin", "ZENBENCH_MARGIN")? {
            self.margin_pct = v.parse().map_err(|_| "invalid practical margin")?;
        }
        if let Some(v) = value("confidence", "ZENBENCH_CONFIDENCE")? {
            self.confidence = v.parse().map_err(|_| "invalid confidence")?;
        }
        if let Some(v) = value("workload-id", "ZENBENCH_WORKLOAD_ID")? {
            self.workload_id = Some(v);
        }
        if let Some(v) = value("scope", "ZENBENCH_SCOPE")? {
            self.scope = Some(v);
        }
        if let Some(v) = value("timeout", "ZENBENCH_TIMEOUT")? {
            self.timeout = Duration::from_secs(v.parse().map_err(|_| "invalid timeout seconds")?);
        }
        if self.mode == BenchmarkMode::Iterate {
            let explicit_processes = value("processes", "ZENBENCH_PROCESSES")?.is_some();
            let child = std::env::var("ZENBENCH_SUBPROCESS").as_deref() == Ok("1");
            let legacy = args.iter().any(|a| a.contains("-of-processes="));
            if explicit_processes && !child && !legacy {
                return Err("--processes requires --mode=confirm or --mode=evidence".into());
            }
            self.processes = 1;
        }
        if self.mode != BenchmarkMode::Iterate
            && args
                .iter()
                .any(|a| a.contains("-of-passes=") || a.contains("-of-processes="))
        {
            return Err("confirmation uses --processes; legacy best/mean/median repetitions are exploratory".into());
        }
        if let Ok(format) = std::env::var("ZENBENCH_FORMAT") {
            if !args.iter().any(|a| a.starts_with("--format="))
                && !["llm", "csv", "md", "markdown", "json", "html"].contains(&format.as_str())
            {
                return Err("invalid ZENBENCH_FORMAT".into());
            }
        }
        self.validate()?;
        Ok(self)
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        if !self.margin_pct.is_finite() || !(0.0..100.0).contains(&self.margin_pct) {
            return Err("margin must be finite and between 0 and 100 percent".into());
        }
        if !self.confidence.is_finite() || !(0.5..1.0).contains(&self.confidence) {
            return Err("confidence must be between 0.5 and 1".into());
        }
        if self.rounds == Some(0) || self.processes == 0 || self.timeout.is_zero() {
            return Err("rounds, processes and timeout must be positive".into());
        }
        if self.timeout.subsec_nanos() != 0 {
            return Err("timeout must be a whole number of seconds".into());
        }
        if self.mode != BenchmarkMode::Iterate {
            if self.rounds.is_some_and(|n| n < 2) {
                return Err("confirmation requires at least two rounds per process".into());
            }
            if self.processes < 3 {
                return Err("confirmation needs at least three independent processes".into());
            }
            if self
                .workload_id
                .as_ref()
                .is_none_or(|s| s.trim().is_empty())
            {
                return Err("confirmation requires --workload-id or BenchmarkPlan.workload_id identifying inputs/workload".into());
            }
            if self.mode == BenchmarkMode::Evidence
                && self.scope.as_ref().is_none_or(|s| s.trim().is_empty())
            {
                return Err("evidence requires an explicit --scope naming this matrix cell".into());
            }
        }
        Ok(())
    }
}

/// Recorded execution identity. Binary digest identifies the executed artifact;
/// runtime Git metadata is deliberately not treated as build provenance.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ExecutionRecord {
    pub protocol: String,
    pub plan: BenchmarkPlan,
    pub binary_sha256: String,
    pub process_id: u32,
    pub independent_processes: bool,
    pub timer_backend: String,
}

pub(crate) fn validate_harness_args(args: &[String]) -> Result<(), String> {
    let valued = [
        "mode",
        "processes",
        "rounds",
        "margin",
        "confidence",
        "workload-id",
        "scope",
        "timeout",
        "group",
        "format",
        "save-baseline",
        "baseline",
        "max-regression",
        "best-of-passes",
        "mean-of-passes",
        "median-of-passes",
        "best-of-processes",
        "mean-of-processes",
        "median-of-processes",
        "cpu-policy",
        "cpu-set",
    ];
    let mut seen = std::collections::HashSet::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if ["--bench", "--no-busy-gate", "--update-on-pass", "--dry-run"].contains(&arg.as_str()) {
            continue;
        }
        if ["--cpu-policy", "--cpu-set"].contains(&arg.as_str()) {
            if iter.next().is_none() {
                return Err(format!("{arg} requires a value"));
            }
            continue;
        }
        let Some((name, value)) = arg.strip_prefix("--").and_then(|s| s.split_once('=')) else {
            return Err(format!(
                "unknown benchmark argument {arg:?}; use --name=value for options"
            ));
        };
        if !valued.contains(&name) || value.is_empty() {
            return Err(format!("unknown or empty benchmark option {arg:?}"));
        }
        if !seen.insert(name) {
            return Err(format!("duplicate benchmark option --{name}"));
        }
        if name.contains("-of-") && value.parse::<usize>().ok().is_none_or(|n| n == 0) {
            return Err(format!("invalid repetition count {arg:?}"));
        }
        if name == "format" && !["llm", "csv", "md", "markdown", "json", "html"].contains(&value) {
            return Err(format!("unknown output format {value:?}"));
        }
        if name == "max-regression"
            && value
                .parse::<f64>()
                .ok()
                .is_none_or(|v| !v.is_finite() || !(0.0..100.0).contains(&v))
        {
            return Err("max-regression must be finite and between 0 and 100 percent".into());
        }
    }
    if seen.contains("save-baseline") && seen.contains("baseline") {
        return Err("use --baseline with --update-on-pass; saving before comparison would replace the control".into());
    }
    if args.iter().any(|a| a == "--update-on-pass") && !seen.contains("baseline") {
        return Err("--update-on-pass requires --baseline".into());
    }
    if seen.contains("max-regression") && !seen.contains("baseline") {
        return Err("--max-regression requires --baseline".into());
    }
    if seen.contains("processes") && seen.iter().any(|s| s.ends_with("-of-processes")) {
        return Err("--processes conflicts with legacy process repetition flags".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_flags_and_unsafe_promotion_combinations_are_rejected() {
        let args = |items: &[&str]| items.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(
            validate_harness_args(&args(&[
                "--mode=confirm",
                "--workload-id=v1",
                "--rounds=30",
                "--bench"
            ]))
            .is_ok()
        );
        for items in [
            vec!["--mod=confirm"],
            vec!["--format=jsno"],
            vec!["--best-of-processes=oops"],
            vec!["--update-on-pass"],
            vec!["--baseline=a", "--save-baseline=a"],
            vec!["--mode=confirm", "--mode=iterate"],
            vec!["--processes=3", "--mean-of-processes=5"],
        ] {
            assert!(validate_harness_args(&args(&items)).is_err(), "{items:?}");
        }
    }

    #[test]
    fn confirmation_requires_workload_scope_and_independent_repetitions() {
        let mut plan = BenchmarkPlan {
            mode: BenchmarkMode::Confirm,
            ..Default::default()
        };
        assert!(plan.validate().is_err());
        plan.workload_id = Some("input-v1".into());
        assert!(plan.validate().is_ok());
        plan.processes = 1;
        assert!(plan.validate().is_err());
        plan.processes = 5;
        plan.mode = BenchmarkMode::Evidence;
        assert!(plan.validate().is_err());
        plan.scope = Some("p-cores/input-v1".into());
        assert!(plan.validate().is_ok());
        plan.margin_pct = f64::NAN;
        assert!(plan.validate().is_err());
    }
}
