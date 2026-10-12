use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

type Env = BTreeMap<String, Option<String>>;
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Plan {
    pub schema: u32,
    pub baseline: Arm,
    pub candidates: Vec<Arm>,
    pub cases: Vec<Case>,
    pub metric: Metric,
    pub check: Check,
    /// Describe the child's timing/warmup boundary; no invented warmup process.
    pub warmup: String,
    #[serde(default = "blocks")]
    pub blocks: usize,
    #[serde(default = "yes")]
    pub aa: bool,
    #[serde(default)]
    pub clean_env: bool,
    #[serde(default)]
    pub env: Env,
    #[serde(default)]
    pub record_env: Vec<String>,
    #[serde(default = "deadline")]
    pub deadline_seconds: u64,
    #[serde(default)]
    pub settle_ms: u64,
    #[serde(default)]
    pub seed: Option<u64>,
}
fn blocks() -> usize {
    4
}
fn yes() -> bool {
    true
}
fn deadline() -> u64 {
    300
}
fn timeout() -> u64 {
    30
}
fn one() -> f64 {
    1.0
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Arm {
    pub id: String,
    pub program: PathBuf,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub cwd: Option<PathBuf>,
    #[serde(default)]
    pub env: Env,
    #[serde(default = "timeout")]
    pub timeout_seconds: u64,
    pub revision: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Case {
    pub id: String,
    #[serde(default)]
    pub fields: BTreeMap<String, String>,
    #[serde(default)]
    pub inputs: BTreeMap<String, PathBuf>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Metric {
    pub extract: Extract,
    /// Units of the extracted value; normalization produces ns per work unit.
    pub unit: String,
    pub expected: usize,
    #[serde(default = "one")]
    pub divide_by: f64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Extract {
    Prefix { prefix: String },
    Tsv { prefix: String, column: usize },
    Json { prefix: String, pointer: String },
    Regex { pattern: String, capture: usize },
    Wall,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Check {
    Files {
        paths: Vec<PathBuf>,
    },
    Json {
        prefix: String,
        pointers: Vec<String>,
    },
    Command {
        program: PathBuf,
        args: Vec<String>,
        timeout_seconds: u64,
    },
    Unchecked {
        reason: String,
    },
}

impl Plan {
    pub(super) fn validate(&self) -> Result<(), String> {
        if self.schema != 1 {
            return Err("unknown pair plan schema; expected 1".into());
        }
        if self.blocks == 0
            || self.cases.is_empty()
            || self.candidates.is_empty()
            || self.warmup.trim().is_empty()
            || self.deadline_seconds == 0
        {
            return Err(
                "blocks, cases, candidates, deadline and warmup description are required".into(),
            );
        }
        if self
            .blocks
            .checked_mul(self.cases.len())
            .and_then(|n| n.checked_mul(self.candidates.len() + 1 + usize::from(self.aa)))
            .is_none_or(|n| n > 100_000)
        {
            return Err("plan exceeds 100000 invocations".into());
        }
        let mut ids = std::collections::BTreeSet::new();
        for arm in self.arms() {
            if arm.id.is_empty()
                || arm.id == "__aa"
                || !ids.insert(&arm.id)
                || arm.timeout_seconds == 0
                || arm.revision.is_empty()
            {
                return Err("arm IDs must be unique/nonempty (not __aa); revision and positive timeout are required".into());
            }
            validate_env(&arm.env)?;
        }
        validate_env(&self.env)?;
        let mut ids = std::collections::BTreeSet::new();
        for case in &self.cases {
            if case.id.is_empty() || !ids.insert(&case.id) {
                return Err("case IDs must be nonempty and unique".into());
            }
            if case
                .fields
                .keys()
                .any(|k| k == "output" || case.inputs.contains_key(k))
                || case.inputs.contains_key("output")
            {
                return Err("case fields, inputs and reserved output must not collide".into());
            }
        }
        self.metric.scale()?;
        if self.metric.expected == 0 || self.metric.expected > 1_000_000 {
            return Err("expected metric cardinality must be 1..1000000".into());
        }
        if matches!(self.metric.extract, Extract::Wall)
            && (self.metric.expected != 1 || self.metric.unit != "ns")
        {
            return Err("wall metric requires expected=1 and unit=ns".into());
        }
        if let Extract::Regex { pattern, capture } = &self.metric.extract {
            let re = regex::Regex::new(pattern).map_err(|e| e.to_string())?;
            if *capture >= re.captures_len() {
                return Err("regex capture index is absent".into());
            }
        }
        match &self.check {
            Check::Files { paths }
                if paths.is_empty()
                    || paths.iter().any(|p| {
                        p.components()
                            .any(|c| !matches!(c, std::path::Component::Normal(_)))
                    }) =>
            {
                return Err(
                    "check files must be nonempty relative paths without parent components".into(),
                );
            }
            Check::Json { pointers, .. } if pointers.is_empty() => {
                return Err("correctness JSON pointers cannot be empty".into());
            }
            Check::Command {
                timeout_seconds: 0, ..
            } => return Err("checker timeout must be positive".into()),
            Check::Unchecked { reason } if reason.trim().is_empty() => {
                return Err("unchecked correctness requires a reason".into());
            }
            _ => {}
        }
        if let Check::Command { args, .. } = &self.check {
            let fields = BTreeMap::from([
                ("baseline".into(), "baseline-output".into()),
                ("candidate".into(), "candidate-output".into()),
            ]);
            for arg in args {
                expand(arg, &fields)?;
            }
        }
        Ok(())
    }
    pub(super) fn arms(&self) -> impl Iterator<Item = &Arm> {
        std::iter::once(&self.baseline).chain(&self.candidates)
    }
}
fn validate_env(env: &Env) -> Result<(), String> {
    for (key, value) in env {
        if key.is_empty()
            || key.contains(['=', '\0'])
            || key
                .to_ascii_uppercase()
                .starts_with("ZENBENCH_COORDINATION_")
            || value.as_ref().is_some_and(|s| s.contains('\0'))
        {
            return Err(format!("invalid or reserved environment key {key:?}"));
        }
    }
    Ok(())
}
impl Metric {
    pub(super) fn scale(&self) -> Result<f64, String> {
        let units = match self.unit.as_str() {
            "ns" => 1.0,
            "us" => 1e3,
            "ms" => 1e6,
            "s" => 1e9,
            _ => return Err("metric unit must be ns/us/ms/s".into()),
        };
        if !self.divide_by.is_finite() || self.divide_by <= 0.0 {
            return Err("divide_by must be finite and positive".into());
        }
        Ok(units / self.divide_by)
    }
}
pub(super) fn expand(value: &str, fields: &BTreeMap<String, String>) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = value;
    while let Some((prefix, tail)) = rest.split_once("${") {
        out.push_str(prefix);
        let (key, tail) = tail.split_once('}').ok_or("unterminated placeholder")?;
        out.push_str(
            fields
                .get(key)
                .ok_or_else(|| format!("unknown placeholder {key:?}"))?,
        );
        rest = tail;
    }
    out.push_str(rest);
    if out.contains('\0') {
        return Err("NUL in command field".into());
    }
    Ok(out)
}
pub(super) fn resolve_path(base: &Path, path: &Path) -> Result<PathBuf, String> {
    base.join(path)
        .canonicalize()
        .map_err(|e| format!("{}: {e}", base.join(path).display()))
}
