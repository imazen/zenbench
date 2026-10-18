//! CLI-only fresh-process comparisons. A child invocation is one observation.
mod analysis;
mod interval;
use analysis::{exit_code, summarize};
mod metric;
mod plan;
use plan::{Arm, Case, Check, Plan, expand, resolve_path};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
const LIMIT: u64 = 64 * 1024 * 1024;

#[derive(Serialize, Deserialize, Clone)]
struct Invocation {
    case: usize,
    block: usize,
    arm: usize,
    order: usize,
    output: PathBuf,
    status: String,
    error: Option<String>,
    values_ns: Vec<f64>,
    wall_seconds: f64,
    exit_code: Option<i32>,
    process_id: Option<u32>,
    continuation_safe: bool,
    observations: Vec<Value>,
    artifacts: BTreeMap<PathBuf, String>,
    correctness: Option<Value>,
    #[serde(default)]
    priority: Value,
}
#[derive(Serialize, Deserialize)]
struct Packet {
    kind: String,
    #[serde(default = "iterate")]
    mode: String,
    schema: u32,
    fingerprint: String,
    plan: Plan,
    identities: Value,
    #[serde(default)]
    context: Value,
    seed: u64,
    schedule: Vec<Vec<Vec<usize>>>,
    invocations: Vec<Invocation>,
    summaries: Vec<Value>,
    #[serde(default)]
    decision: Value,
    complete: bool,
    placement: Value,
    elapsed_seconds: f64,
    resume_from: Option<PathBuf>,
    commands: Value,
}

pub(crate) struct Options<'a> {
    pub plan: &'a Path,
    pub output: Option<&'a Path>,
    pub case: Option<&'a str>,
    pub resume: Option<&'a Path>,
    pub dry_run: bool,
    pub wait: u64,
    pub mode: &'a str,
}

fn iterate() -> String {
    "iterate".into()
}

pub(crate) fn run(options: Options<'_>) -> Result<i32, String> {
    if !matches!(options.mode, "iterate" | "confirm") {
        return Err("pair mode must be iterate or confirm".into());
    }
    if options.mode == "confirm" && options.resume.is_some() {
        return Err(
            "confirmation cannot resume or pool attempts; run a fresh fixed experiment".into(),
        );
    }
    let plan_file = options.plan.canonicalize().map_err(|e| e.to_string())?;
    let base = plan_file.parent().ok_or("plan has no parent directory")?;
    let mut plan: Plan =
        serde_json::from_slice(&read_limited(&plan_file)?).map_err(|e| e.to_string())?;
    plan.validate()?;
    if options.mode == "confirm" && plan.confirmation.is_none() {
        return Err("--mode=confirm requires a predeclared confirmation object in the plan".into());
    }
    if let Some(filter) = options.case {
        plan.cases.retain(|c| c.id == filter);
    }
    if plan.cases.is_empty() {
        return Err("case selection is empty".into());
    }
    let inherited: BTreeMap<OsString, OsString> = if plan.clean_env {
        BTreeMap::new()
    } else {
        std::env::vars_os()
            .map(|(key, value)| (env_key(&key), value))
            .collect()
    };
    let mut arms: Vec<_> = plan.arms().cloned().collect();
    if plan.aa {
        let mut control = plan.baseline.clone();
        control.id = "__aa".into();
        arms.push(control);
    }
    // Resolve all inputs/binaries before any workload runs. PATH search uses the
    // actual arm environment and cwd, not the supervisor's implicit shell.
    let mut identities = BTreeMap::new();
    for arm in &mut arms {
        let cwd = resolve_path(base, arm.cwd.as_deref().unwrap_or(Path::new(".")))?;
        arm.cwd = Some(cwd.clone());
        let env = environment(&inherited, &plan, arm, &BTreeMap::new(), false)?;
        arm.program = executable(&cwd, &arm.program, &env)?;
        identities.insert(arm.program.clone(), digest_file(&arm.program)?);
    }
    for case in &mut plan.cases {
        for path in case.inputs.values_mut() {
            *path = resolve_path(base, path)?;
            identities.insert(path.clone(), digest_file(path)?);
        }
    }
    if let Check::Command { program, .. } = &mut plan.check {
        // Checker executables must be paths, not ambiguous PATH names.
        *program = resolve_path(base, program)?;
        identities.insert(program.clone(), digest_file(program)?);
    }
    // Freeze the resolved arms in the retained plan; A/A remains a derived arm.
    plan.baseline = arms[0].clone();
    plan.candidates = arms[1..1 + plan.candidates.len()].to_vec();
    let recorded_env: BTreeMap<_, _> = plan
        .record_env
        .iter()
        .map(|k| {
            (
                k.clone(),
                inherited
                    .get(&env_key(std::ffi::OsStr::new(k)))
                    .map(|v| v.to_string_lossy().into_owned()),
            )
        })
        .collect();
    let identity = json!({"files":identities,"inherited_env":recorded_env,"inherited_env_complete":plan.clean_env,"provenance_limit":"Only explicitly selected inherited environment values are retained. Shared libraries and undeclared runtime assets are not identified."});
    let fingerprint_record = if options.mode == "iterate" {
        json!({"plan":plan,"identity":identity})
    } else {
        json!({"plan":plan,"identity":identity,"mode":options.mode})
    };
    let fingerprint = digest(&serde_json::to_vec(&fingerprint_record).map_err(|e| e.to_string())?);
    let placement = serde_json::to_value(zenbench::platform::check_cpu_placement()?)
        .map_err(|e| e.to_string())?;
    let mut prior = if let Some(path) = options.resume {
        Some(load(path)?)
    } else {
        None
    };
    if let Some(previous) = &prior {
        if previous.fingerprint != fingerprint {
            return Err("resume fingerprint mismatch: plan, binaries, inputs or recorded environment changed".into());
        }
        verify_artifacts(previous)?;
    }
    if prior.as_ref().is_some_and(|p| p.placement != placement) {
        return Err("resume CPU placement differs from the retained experiment".into());
    }
    let seed = prior
        .as_ref()
        .map(|p| p.seed)
        .or(plan.seed)
        .unwrap_or_else(|| now() as u64 ^ u64::from(std::process::id()));
    let schedule = schedule(plan.cases.len(), plan.blocks, arms.len(), seed);
    let output = options.output.map(Path::to_path_buf).unwrap_or_else(|| {
        PathBuf::from("target/zenbench/pairs").join(format!("{:x}-{}", now(), std::process::id()))
    });
    let output = std::path::absolute(output).map_err(|e| e.to_string())?;
    // Validate every expansion before admission. Dry-run carries exact argv/env
    // overrides; inherited secrets are never dumped.
    let mut launches = Vec::new();
    let mut expanded_bytes = 0usize;
    for (case_index, case) in plan.cases.iter().enumerate() {
        for (block, order) in schedule[case_index].iter().enumerate() {
            for (position, &arm) in order.iter().enumerate() {
                let dir = output.join(format!("c{case_index}-b{block}-o{position}"));
                let fields = fields(case, &dir);
                let args: Vec<_> = arms[arm]
                    .args
                    .iter()
                    .map(|s| expand(s, &fields))
                    .collect::<Result<_, _>>()?;
                environment(&inherited, &plan, &arms[arm], &fields, true)?;
                let overrides: BTreeMap<_, _> = plan
                    .env
                    .iter()
                    .chain(&arms[arm].env)
                    .map(|(k, v)| {
                        Ok((
                            k.clone(),
                            v.as_ref().map(|v| expand(v, &fields)).transpose()?,
                        ))
                    })
                    .collect::<Result<_, String>>()?;
                let launch = json!({"case":case.id,"block":block,"order":position,"arm":arms[arm].id,"program":arms[arm].program,"args":args,"cwd":arms[arm].cwd,"output":dir,"env_overrides":overrides,"timeout_seconds":arms[arm].timeout_seconds});
                expanded_bytes += serde_json::to_vec(&launch)
                    .map_err(|e| e.to_string())?
                    .len();
                if expanded_bytes > 8 * 1024 * 1024 {
                    return Err("expanded launch plan exceeds 8 MiB; select fewer cases".into());
                }
                launches.push(launch);
            }
        }
    }
    if options.dry_run {
        println!("{}", serde_json::to_string_pretty(&json!({"fingerprint":fingerprint,"mode":options.mode,"plan":plan,"identities":identity,"placement":placement,"seed":seed,"schedule":schedule,"launches":launches,"invocation_count":launches.len(),"output":output,"runtime_estimate":null,"inference":"exploratory; no acceptance decision"})).unwrap());
        return Ok(0);
    }
    let start = Instant::now();
    let mut admission = crate::coordination::Reservation::acquire(
        "external pair experiment",
        Duration::from_secs(options.wait),
    )
    .map_err(|e| e.to_string())?;
    fs::create_dir(&output)
        .or_else(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                fs::create_dir_all(output.parent().unwrap())?;
                fs::create_dir(&output)
            } else {
                Err(e)
            }
        })
        .map_err(|e| format!("output must be a new directory: {e}"))?;
    fs::write(output.join(".gitignore"), "*\n").map_err(|e| e.to_string())?;
    eprintln!("[zenbench pair] attempt retained at {}", output.display());
    let mut packet = Packet {
        kind: "zenbench_pair".into(),
        mode: options.mode.into(),
        schema: 1,
        fingerprint,
        plan,
        identities: identity,
        context: json!({"testbed":zenbench::platform::detect_testbed(),"started_unix_ns":now().to_string(),
            "runner_sha256":digest_file(&std::env::current_exe().map_err(|e|e.to_string())?)?,
            "runner_version":env!("CARGO_PKG_VERSION"),
            "supervisor_priority":priority(std::process::id()),"priority_policy":"inherited; programs must not change priority during timing"}),
        seed,
        schedule,
        invocations: Vec::new(),
        summaries: Vec::new(),
        decision: Value::Null,
        complete: false,
        placement,
        elapsed_seconds: 0.0,
        resume_from: options.resume.map(Path::to_path_buf),
        commands: json!({"decode":"zenbench data read packet.json.gz --format json","compare":"zenbench data pair-diff BEFORE.json.gz AFTER.json.gz","export":"zenbench data read packet.json.gz --format csv"}),
    };
    // Planned experiment and complete order are durable before the first launch.
    write_json(&output.join("experiment.json"), &packet)?;
    let mut journal = File::create(output.join("events.jsonl")).map_err(|e| e.to_string())?;
    let monitor = zenbench::platform::SystemMonitor::new();
    let deadline = Duration::from_secs(packet.plan.deadline_seconds);
    'experiment: for ci in 0..packet.plan.cases.len() {
        if let Some(previous) = prior.as_mut() {
            if case_complete(previous, ci) {
                packet.invocations.extend(
                    previous
                        .invocations
                        .iter()
                        .filter(|i| i.case == ci)
                        .cloned(),
                );
                continue;
            }
        }
        for block in 0..packet.plan.blocks {
            for (order, &arm_index) in packet.schedule[ci][block].iter().enumerate() {
                if start.elapsed() >= deadline {
                    break 'experiment;
                }
                let arm = &arms[arm_index];
                let dir = output.join(format!("c{ci}-b{block}-o{order}"));
                fs::create_dir(&dir).map_err(|e| e.to_string())?;
                let mut event = Invocation {
                    case: ci,
                    block,
                    arm: arm_index,
                    order,
                    output: dir.clone(),
                    status: "running".into(),
                    error: None,
                    values_ns: Vec::new(),
                    wall_seconds: 0.0,
                    exit_code: None,
                    process_id: None,
                    continuation_safe: true,
                    observations: Vec::new(),
                    artifacts: BTreeMap::new(),
                    correctness: None,
                    priority: Value::Null,
                };
                append(&mut journal, &event)?;
                let attempt = invoke(
                    &packet.plan,
                    arm,
                    &packet.plan.cases[ci],
                    &inherited,
                    &dir,
                    &monitor,
                    &mut admission,
                    start,
                    deadline,
                    &packet.placement,
                    packet.mode == "confirm",
                    &mut event,
                );
                match attempt {
                    Ok(()) => event.status = "completed".into(),
                    Err(error) => {
                        event.status = "failed".into();
                        event.error = Some(error);
                    }
                }
                for name in ["stdout.txt", "stderr.txt"] {
                    let path = dir.join(name);
                    if path.exists() {
                        event.artifacts.insert(path.clone(), digest_file(&path)?);
                    }
                }
                append(&mut journal, &event)?;
                let can_continue = event.continuation_safe;
                packet.invocations.push(event);
                if !can_continue
                    || packet
                        .invocations
                        .iter()
                        .map(|i| i.observations.len())
                        .sum::<usize>()
                        > 100_000
                {
                    break 'experiment;
                }
            }
            check_block(&mut packet, ci, block, &mut admission, start, deadline)?;
            write_json(
                &output.join(format!("case-{ci}-block-{block}.json")),
                &packet
                    .invocations
                    .iter()
                    .filter(|i| i.case == ci && i.block == block)
                    .collect::<Vec<_>>(),
            )?;
            if packet.invocations.iter().any(|i| !i.continuation_safe) {
                break 'experiment;
            }
        }
    }
    // Refuse changed executable/input identities even if they changed mid-run.
    for (path, expected) in &identities {
        if digest_file(path)? != *expected {
            return Err(format!(
                "identity changed during experiment: {}; journal retained",
                path.display()
            ));
        }
    }
    packet.summaries = summarize(&packet);
    packet.complete = (0..packet.plan.cases.len()).all(|i| case_complete(&packet, i));
    packet.decision = decision(&packet);
    packet.elapsed_seconds = start.elapsed().as_secs_f64();
    save(&output.join("packet.json.gz"), &packet)?;
    fs::write(output.join("selected.csv"), csv(&packet)).map_err(|e| e.to_string())?;
    println!("{}", report(&packet));
    eprintln!(
        "[zenbench pair] evidence: {}\n[zenbench pair] decode: zenbench data read {} --format json\n[zenbench pair] selected export: {}",
        output.join("packet.json.gz").display(),
        output.join("packet.json.gz").display(),
        output.join("selected.csv").display()
    );
    Ok(exit_code(&packet))
}

fn env_key(key: &std::ffi::OsStr) -> OsString {
    #[cfg(windows)]
    if let Some(key) = key.to_str() {
        return OsString::from(key.to_ascii_uppercase());
    }
    key.to_owned()
}
fn environment(
    inherited: &BTreeMap<OsString, OsString>,
    plan: &Plan,
    arm: &Arm,
    fields: &BTreeMap<String, String>,
    substitute: bool,
) -> Result<BTreeMap<OsString, OsString>, String> {
    let mut env = inherited.clone();
    for (key, value) in plan.env.iter().chain(&arm.env) {
        match value {
            Some(value) => {
                env.insert(
                    env_key(std::ffi::OsStr::new(key)),
                    if substitute {
                        expand(value, fields)?
                    } else {
                        value.clone()
                    }
                    .into(),
                );
            }
            None => {
                env.remove(&env_key(std::ffi::OsStr::new(key)));
            }
        }
    }
    Ok(env)
}
fn executable(
    cwd: &Path,
    name: &Path,
    env: &BTreeMap<OsString, OsString>,
) -> Result<PathBuf, String> {
    if name.is_absolute() || name.components().count() > 1 {
        return resolve_path(cwd, name);
    }
    let path = env
        .get(&env_key(std::ffi::OsStr::new("PATH")))
        .ok_or("bare executable requires PATH or an explicit path")?;
    for directory in std::env::split_paths(path) {
        let candidate = cwd.join(directory).join(name);
        if candidate.is_file() {
            return candidate.canonicalize().map_err(|e| e.to_string());
        }
        #[cfg(windows)]
        if candidate.extension().is_none() && candidate.with_extension("exe").is_file() {
            return candidate
                .with_extension("exe")
                .canonicalize()
                .map_err(|e| e.to_string());
        }
    }
    Err(format!(
        "executable {} not found in arm PATH",
        name.display()
    ))
}
fn fields(case: &Case, output: &Path) -> BTreeMap<String, String> {
    let mut fields = case.fields.clone();
    fields.extend(
        case.inputs
            .iter()
            .map(|(k, p)| (k.clone(), p.to_string_lossy().into_owned())),
    );
    fields.insert("output".into(), output.to_string_lossy().into_owned());
    fields
}
#[allow(clippy::too_many_arguments)]
fn invoke(
    plan: &Plan,
    arm: &Arm,
    case: &Case,
    inherited: &BTreeMap<OsString, OsString>,
    dir: &Path,
    monitor: &zenbench::platform::SystemMonitor,
    admission: &mut crate::coordination::Reservation,
    start: Instant,
    deadline: Duration,
    placement: &Value,
    confirm: bool,
    event: &mut Invocation,
) -> Result<(), String> {
    let fields = fields(case, dir);
    let mut command = Command::new(&arm.program);
    command
        .current_dir(arm.cwd.as_ref().unwrap())
        .args(
            arm.args
                .iter()
                .map(|s| expand(s, &fields))
                .collect::<Result<Vec<_>, _>>()?,
        )
        .env_clear()
        .envs(environment(inherited, plan, arm, &fields, true)?)
        .stdin(Stdio::null())
        .stdout(File::create(dir.join("stdout.txt")).map_err(|e| e.to_string())?)
        .stderr(File::create(dir.join("stderr.txt")).map_err(|e| e.to_string())?);
    let before = serde_json::to_value(zenbench::platform::check_cpu_placement()?)
        .map_err(|e| e.to_string())?;
    if &before != placement {
        return Err("CPU placement changed before invocation".into());
    }
    let settle = Duration::from_millis(plan.settle_ms);
    let remaining = deadline.saturating_sub(start.elapsed());
    if remaining <= settle {
        return Err("total deadline exhausted; invocation not started".into());
    }
    std::thread::sleep(settle);
    event.observations.push(observation(monitor));
    if start.elapsed() >= deadline {
        return Err("total deadline exhausted during admission observation".into());
    }
    let began = Instant::now();
    event.continuation_safe = false;
    let mut child_priority = Value::Null;
    let done = std::sync::atomic::AtomicBool::new(false);
    let child_pid = std::sync::atomic::AtomicU32::new(0);
    let result = std::thread::scope(|scope| {
        let observer = scope.spawn(|| {
            use std::sync::atomic::Ordering;
            let mut observations = Vec::new();
            while confirm && !done.load(Ordering::Acquire) {
                let pid = child_pid.load(Ordering::Acquire);
                if pid == 0 {
                    std::thread::sleep(Duration::from_millis(10));
                    continue;
                }
                let mut sample = observe(monitor, &[pid], "during");
                let sampled_at = Instant::now();
                // The worker may have exited during this OS counter window.
                // Retain it, but do not mislabel unassignable CPU as foreign.
                // A fixed fresh boundary window follows after the thread joins.
                if observations.is_empty() {
                    // sysinfo needs a prior process sample to attribute CPU.
                    // Never interpret a newly discovered worker's zero delta
                    // as proof that the first interval had no worker activity.
                    sample["phase"] = json!("start_transition");
                    sample["used_for_admission"] = json!(false);
                }
                observations.push((sampled_at, sample));
                if observations.len() >= 10000 {
                    observations.push((Instant::now(), json!({"phase":"observation_limit","clean":false,"used_for_admission":true})));
                    break;
                }
            }
            observations
        });
        let mut lifecycle = super::supervisor::Lifecycle::default();
        let result = super::supervisor::run_with_observer(
            command,
            Duration::from_secs(arm.timeout_seconds).min(deadline.saturating_sub(start.elapsed())),
            admission,
            None,
            &[dir.join("stdout.txt"), dir.join("stderr.txt")],
            super::supervisor::ChildObservation {
                process_id: &mut event.process_id,
                poll: &mut |pid| {
                    if child_priority.is_null() {
                        child_priority = priority(pid);
                    }
                    child_pid.store(pid, std::sync::atomic::Ordering::Release);
                    Ok(())
                },
                lifecycle: Some(&mut lifecycle),
            },
        );
        event.wall_seconds = began.elapsed().as_secs_f64();
        done.store(true, std::sync::atomic::Ordering::Release);
        event.observations.extend(finish_resource_observations(
            observer.join().expect("resource observer panicked"),
            lifecycle,
            began,
        ));
        result.map(|outcome| outcome.status)
    });
    event.priority = child_priority;
    event.observations.push(observation(monitor));
    let status = result.map_err(|e| e.to_string())?;
    event.continuation_safe = true;
    event.exit_code = status.code();
    if !status.success() {
        return Err(format!("child exited with {status}"));
    }
    if &serde_json::to_value(zenbench::platform::check_cpu_placement()?)
        .map_err(|e| e.to_string())?
        != placement
    {
        return Err("CPU placement changed after invocation".into());
    }
    let metric_text = if matches!(plan.metric.extract, plan::Extract::Wall) {
        String::new()
    } else {
        String::from_utf8(read_limited(&dir.join(plan.metric.stream.file()))?)
            .map_err(|e| e.to_string())?
    };
    event.values_ns = metric::extract(&plan.metric, &metric_text, event.wall_seconds * 1e9)?;
    match &plan.check {
        Check::Files { paths } => {
            let mut hashes = BTreeMap::new();
            for path in paths {
                let key = path.to_string_lossy().into_owned();
                let path = dir.join(path).canonicalize().map_err(|e| e.to_string())?;
                if !path.starts_with(dir.canonicalize().map_err(|e| e.to_string())?) {
                    return Err("correctness artifact escapes its fresh output directory".into());
                }
                let hash = digest_file(&path)?;
                hashes.insert(key, hash.clone());
                event.artifacts.insert(path, hash);
            }
            event.correctness = Some(json!(hashes));
        }
        Check::Json {
            prefix,
            pointers,
            stream,
        } => {
            let check_text = String::from_utf8(read_limited(&dir.join(stream.file()))?)
                .map_err(|e| e.to_string())?;
            let rows: Vec<_> = check_text
                .lines()
                .filter_map(|s| s.strip_prefix(prefix))
                .collect();
            if rows.len() != 1 {
                return Err("correctness contract requires exactly one matching JSON row".into());
            }
            let row: Value = serde_json::from_str(rows[0]).map_err(|e| e.to_string())?;
            let values: Vec<_> = pointers
                .iter()
                .map(|p| {
                    row.pointer(p)
                        .cloned()
                        .ok_or_else(|| format!("missing correctness field {p}"))
                })
                .collect::<Result<_, _>>()?;
            event.correctness = Some(json!(values));
        }
        _ => {}
    }
    Ok(())
}
fn priority(pid: u32) -> Value {
    #[cfg(unix)]
    {
        match Command::new("ps")
            .args(["-o", "ni=", "-p", &pid.to_string()])
            .output()
        {
            Ok(out) if out.status.success() => {
                let text = String::from_utf8_lossy(&out.stdout);
                match text.trim().parse::<i32>() {
                    Ok(nice) => json!({"nice":nice,"source":"ps ni","qos_verified":false}),
                    Err(_) => {
                        json!({"unavailable":"process exited or priority could not be parsed"})
                    }
                }
            }
            _ => json!({"unavailable":"OS priority query failed"}),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        json!({"unavailable":"native priority query not implemented","policy":"inherited"})
    }
}
fn observation(monitor: &zenbench::platform::SystemMonitor) -> Value {
    observe(monitor, &[std::process::id()], "boundary")
}

/// Classify sampled windows with the supervisor's owned leader lifecycle.
fn finish_resource_observations(
    samples: Vec<(Instant, Value)>,
    lifecycle: super::supervisor::Lifecycle,
    began: Instant,
) -> Vec<Value> {
    let relative = |t: Option<Instant>| {
        t.map_or(Value::Null, |t| {
            json!(t.saturating_duration_since(began).as_nanos().to_string())
        })
    };
    let mut observations: Vec<Value> = samples
        .into_iter()
        .map(|(sampled_at, mut sample)| {
            sample["sample_completed_after_start_ns"] = relative(Some(sampled_at));
            sample["leader_last_alive_after_start_ns"] = relative(lifecycle.last_alive);
            sample["leader_exit_observed_after_start_ns"] = relative(lifecycle.exit_observed);
            // A reaped leader cannot be subtracted from CPU counters. Only a window
            // completing at or after the owned exit observation can have refreshed
            // after reaping; windows completing earlier, including those after the
            // last live probe, remain subject to admission.
            if sample["phase"] == "during"
                && lifecycle
                    .exit_observed
                    .is_some_and(|exit| sampled_at >= exit)
            {
                sample["phase"] = json!("exit_transition");
                sample["used_for_admission"] = json!(false);
            }
            // Transitions waive only the whole-host CPU magnitude. Heavy foreign
            // processes, RAM, temperature, freshness and finite CPU must still
            // pass at every window; false or missing evidence rejects.
            if sample["ownership_independent_clean"] != true {
                sample["clean"] = json!(false);
                sample["used_for_admission"] = json!(true);
            }
            if matches!(
                sample["phase"].as_str(),
                Some("start_transition" | "exit_transition")
            ) && sample["used_for_admission"] == false
            {
                sample["admission_waiver"] = json!("cpu_load_magnitude");
            }
            sample
        })
        .collect();
    // Without an owned live probe before the exit observation, these windows are
    // not tied to the worker's lifetime: refuse them rather than trust clean values.
    let observed = matches!(
        (lifecycle.last_alive, lifecycle.exit_observed),
        (Some(alive), Some(exit)) if alive < exit
    );
    if !observations.is_empty() && !observed {
        observations.push(json!({"phase":"lifecycle_unobserved","clean":false,"used_for_admission":true,
            "policy":"whole-host-strict-v1","leader_last_alive_after_start_ns":relative(lifecycle.last_alive),
            "leader_exit_observed_after_start_ns":relative(lifecycle.exit_observed)}));
    }
    observations
}
/// Strict whole-host admission for one snapshot. Only the CPU-load magnitude
/// depends on assigning worker CPU, so only it may be waived at transitions.
#[derive(Debug, PartialEq)]
struct Admission {
    cpu_load_within_limit: bool,
    ownership_independent: bool,
}
fn admission(state: &zenbench::platform::SystemState, sample_duration: Duration) -> Admission {
    let config = zenbench::GateConfig::strict();
    Admission {
        cpu_load_within_limit: state.cpu_load <= config.max_cpu_load,
        ownership_independent: state.cpu_load.is_finite()
            && state.heavy_process_count <= config.max_heavy_processes
            && state.available_ram_bytes >= config.min_available_ram_bytes
            && state
                .cpu_temp_c
                .is_none_or(|t| t.is_finite() && t <= config.max_cpu_temp_c.unwrap())
            && sample_duration <= Duration::from_secs(2),
    }
}
fn observe(monitor: &zenbench::platform::SystemMonitor, workers: &[u32], phase: &str) -> Value {
    let began = Instant::now();
    let state = monitor.snapshot_excluding_processes(workers);
    observation_value(&state, began.elapsed(), phase)
}
fn observation_value(
    state: &zenbench::platform::SystemState,
    sample_duration: Duration,
    phase: &str,
) -> Value {
    let admission = admission(state, sample_duration);
    json!({"observed_unix_ns":now().to_string(),"sample_duration_ns":sample_duration.as_nanos().min(u64::MAX as u128) as u64,
        "phase":phase,"cpu_load":state.cpu_load,"available_ram_bytes":state.available_ram_bytes,
        "heavy_process_count":state.heavy_process_count,"cpu_temp_c":state.cpu_temp_c,
        "policy":"whole-host-strict-v1","used_for_admission":true,
        "clean":admission.cpu_load_within_limit && admission.ownership_independent,
        "ownership_independent_clean":admission.ownership_independent,"cleanliness_certified":false})
}
fn check_block(
    packet: &mut Packet,
    case: usize,
    block: usize,
    admission: &mut crate::coordination::Reservation,
    start: Instant,
    deadline: Duration,
) -> Result<(), String> {
    let Some(base) = packet
        .invocations
        .iter()
        .find(|i| i.case == case && i.block == block && i.arm == 0 && i.status == "completed")
        .cloned()
    else {
        return Ok(());
    };
    for invocation in packet
        .invocations
        .iter_mut()
        .filter(|i| i.case == case && i.block == block && i.arm != 0 && i.status == "completed")
    {
        let result = match &packet.plan.check {
            Check::Command {
                program,
                args,
                timeout_seconds,
            } => {
                let fields = BTreeMap::from([
                    (
                        "baseline".into(),
                        base.output.to_string_lossy().into_owned(),
                    ),
                    (
                        "candidate".into(),
                        invocation.output.to_string_lossy().into_owned(),
                    ),
                ]);
                let mut command = Command::new(program);
                let stdout = invocation.output.join("checker.stdout.txt");
                let stderr = invocation.output.join("checker.stderr.txt");
                command
                    .args(
                        args.iter()
                            .map(|s| expand(s, &fields))
                            .collect::<Result<Vec<_>, _>>()?,
                    )
                    .stdin(Stdio::null())
                    .stdout(File::create(&stdout).map_err(|e| e.to_string())?)
                    .stderr(File::create(&stderr).map_err(|e| e.to_string())?);
                let remaining = deadline.saturating_sub(start.elapsed());
                let result = if remaining.is_zero() {
                    Err("deadline exhausted before checker".into())
                } else {
                    super::supervisor::run(
                        command,
                        remaining.min(Duration::from_secs(*timeout_seconds)),
                        admission,
                        None,
                        &[stdout.clone(), stderr.clone()],
                        &mut None,
                    )
                    .map_err(|e| {
                        invocation.continuation_safe = false;
                        e.to_string()
                    })
                    .and_then(|s| {
                        if s.success() {
                            Ok(())
                        } else {
                            Err(format!("checker exited with {s}"))
                        }
                    })
                };
                for path in [stdout, stderr] {
                    invocation
                        .artifacts
                        .insert(path.clone(), digest_file(&path)?);
                }
                result
            }
            Check::Unchecked { .. } => Ok(()),
            _ if invocation.correctness == base.correctness => Ok(()),
            _ => Err("correctness differs from baseline in the same block".into()),
        };
        if let Err(e) = result {
            invocation.status = "failed".into();
            invocation.error = Some(e);
        }
    }
    Ok(())
}
fn case_complete(packet: &Packet, case: usize) -> bool {
    let expected =
        packet.plan.blocks * (packet.plan.candidates.len() + 1 + usize::from(packet.plan.aa));
    let rows: Vec<_> = packet
        .invocations
        .iter()
        .filter(|i| i.case == case)
        .collect();
    rows.len() == expected && rows.iter().all(|i| i.status == "completed")
}
fn schedule(cases: usize, blocks: usize, arms: usize, seed: u64) -> Vec<Vec<Vec<usize>>> {
    let mut state = seed;
    let mut next = || {
        state = state.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    };
    (0..cases)
        .map(|_| {
            (0..blocks)
                .map(|_| {
                    let mut order: Vec<_> = (0..arms).collect();
                    for i in (1..arms).rev() {
                        let bound = (i + 1) as u64;
                        let threshold = bound.wrapping_neg() % bound;
                        let r = loop {
                            let n = next();
                            if n >= threshold {
                                break n;
                            }
                        };
                        order.swap(i, (r % bound) as usize);
                    }
                    order
                })
                .collect()
        })
        .collect()
}
fn now() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}
fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn digest_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut hash = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
}
fn read_limited(path: &Path) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|e| e.to_string())?
        .take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > LIMIT {
        return Err("evidence/log exceeds 64 MiB read limit".into());
    }
    Ok(bytes)
}
fn write_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let partial = path.with_extension("json.partial");
    let mut file = File::create_new(&partial).map_err(|e| e.to_string())?;
    serde_json::to_writer(&mut file, value).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    fs::rename(partial, path).map_err(|e| e.to_string())
}
fn append(file: &mut File, value: &impl Serialize) -> Result<(), String> {
    serde_json::to_writer(&mut *file, value).map_err(|e| e.to_string())?;
    file.write_all(b"\n")
        .and_then(|_| file.sync_data())
        .map_err(|e| e.to_string())
}
fn save(path: &Path, packet: &Packet) -> Result<(), String> {
    let partial = path.with_extension("gz.partial");
    let file = File::create_new(&partial).map_err(|e| e.to_string())?;
    let mut writer = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    serde_json::to_writer(&mut writer, packet).map_err(|e| e.to_string())?;
    writer
        .finish()
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())?;
    fs::rename(partial, path).map_err(|e| e.to_string())
}
fn load(path: &Path) -> Result<Packet, String> {
    if path.is_dir() {
        return load_interrupted(path);
    }
    let bytes = packet_bytes(path)?;
    let mut packet: Packet = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if packet.kind != "zenbench_pair" || packet.schema != 1 {
        return Err("unsupported external-process packet schema".into());
    }
    validate_packet(&packet)?;
    packet.summaries = summarize(&packet);
    packet.decision = decision(&packet);
    Ok(packet)
}
fn verify_artifacts(packet: &Packet) -> Result<(), String> {
    for invocation in &packet.invocations {
        for (path, hash) in &invocation.artifacts {
            if digest_file(path)? != *hash {
                return Err(format!("retained artifact changed: {}", path.display()));
            }
        }
    }
    Ok(())
}
fn decision(packet: &Packet) -> Value {
    json!({"accepted":packet.mode == "confirm" && exit_code(packet) == 0,
        "exit_code":exit_code(packet),"mode":packet.mode,"model_conditional":true,
        "accept":packet.plan.confirmation.as_ref().map(|c| &c.accept)})
}
fn csv(packet: &Packet) -> String {
    let quote = |v: &Value| format!("\"{}\"", v.as_str().unwrap_or("").replace('"', "\"\""));
    let mut out = String::from(
        "case,arm,complete_blocks,planned_blocks,latency_change_pct,log_ratio_sd,inference,control_present,correctness_checked,lower_pct,upper_pct,verdict,ratio_median,ratio_min,ratio_mad\n",
    );
    for s in &packet.summaries {
        out.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
            quote(&s["case"]),
            quote(&s["arm"]),
            s["complete_blocks"],
            s["planned_blocks"],
            s["latency_change_pct"],
            s["log_ratio_sd"],
            quote(&s["inference"]),
            s["control_present"],
            s["correctness_checked"],
            s["interval"]["lower_pct"],
            s["interval"]["upper_pct"],
            quote(&s["verdict"]),
            s["paired_ratio"]["median"],
            s["paired_ratio"]["min"],
            s["paired_ratio"]["mad"]
        ));
    }
    out
}
fn report(packet: &Packet) -> String {
    let failures: String = packet
        .invocations
        .iter()
        .filter_map(|event| {
            event.error.as_ref().map(|reason| {
                format!(
                    "failure {}\n",
                    json!({"case":event.case,"block":event.block,"arm":event.arm,
                        "reason":reason,"output":event.output})
                )
            })
        })
        .collect();
    let observed_busy = packet
        .invocations
        .iter()
        .filter(|i| {
            i.observations
                .iter()
                .any(|v| v["cpu_load"].as_f64().is_some_and(|v| v > 0.05))
        })
        .count();
    format!(
        "zenbench_pair schema=1 mode={} complete={} fingerprint={} decision={}\nplacement={} invocations_with_observed_cpu_load_above_5pct={}\n{}\n{failures}Positive change means slower. Intervals assume independent approximately normal block log ratios.\nIterate exit 0 means execution completed only; Confirm requires valid observations and A/A equivalence.\nDetails: {}",
        packet.mode,
        packet.complete,
        packet.fingerprint,
        decision(packet),
        packet.placement["status"],
        observed_busy,
        csv(packet),
        serde_json::to_string(&packet.summaries).unwrap()
    )
}
pub(crate) fn read(path: &Path, format: &str) -> Result<String, String> {
    let packet = load(path)?;
    Ok(match format {
        "json" => serde_json::to_string_pretty(&packet).map_err(|e| e.to_string())?,
        "csv" => csv(&packet),
        _ => report(&packet),
    })
}
pub(crate) fn is_packet(path: &Path) -> bool {
    if path.is_dir() {
        return path.join("experiment.json").is_file();
    }
    let Ok(bytes) = packet_bytes(path) else {
        return false;
    };
    let prefix: String = String::from_utf8_lossy(&bytes[..bytes.len().min(256)])
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    prefix.starts_with("{\"kind\":\"zenbench_pair\"")
}
fn packet_bytes(path: &Path) -> Result<Vec<u8>, String> {
    use std::io::BufRead;
    let mut reader = std::io::BufReader::new(File::open(path).map_err(|e| e.to_string())?);
    let gzip = reader
        .fill_buf()
        .map_err(|e| e.to_string())?
        .starts_with(&[0x1f, 0x8b]);
    let mut bytes = Vec::new();
    if gzip {
        flate2::read::GzDecoder::new(reader)
            .take(LIMIT + 1)
            .read_to_end(&mut bytes)
    } else {
        reader.take(LIMIT + 1).read_to_end(&mut bytes)
    }
    .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > LIMIT {
        return Err("pair packet exceeds read limit".into());
    }
    Ok(bytes)
}
pub(crate) fn diff(before: &Path, after: &Path) -> Result<String, String> {
    let before = load(before)?;
    let after = load(after)?;
    Ok(serde_json::to_string_pretty(&json!({"inference":"exploratory comparison of retained summaries; populations are not pooled","same_plan_and_identity":before.fingerprint==after.fingerprint,"before":before.summaries,"after":after.summaries,"before_complete":before.complete,"after_complete":after.complete})).unwrap())
}

fn validate_packet(packet: &Packet) -> Result<(), String> {
    if packet.kind != "zenbench_pair" || packet.schema != 1 {
        return Err("unsupported external-process packet schema".into());
    }
    packet.plan.validate()?;
    if !matches!(packet.mode.as_str(), "iterate" | "confirm")
        || (packet.mode == "confirm"
            && (packet.plan.confirmation.is_none() || packet.resume_from.is_some()))
    {
        return Err("invalid retained confirmation contract".into());
    }
    if packet.complete && !(0..packet.plan.cases.len()).all(|ci| case_complete(packet, ci)) {
        return Err("packet claims completion with missing or invalid invocations".into());
    }
    let arms = packet.plan.candidates.len() + 1 + usize::from(packet.plan.aa);
    if packet.schedule.len() != packet.plan.cases.len() {
        return Err("invalid case schedule".into());
    }
    for blocks in &packet.schedule {
        if blocks.len() != packet.plan.blocks {
            return Err("invalid block schedule".into());
        }
        for order in blocks {
            let mut sorted = order.clone();
            sorted.sort_unstable();
            if sorted != (0..arms).collect::<Vec<_>>() {
                return Err("invalid arm schedule".into());
            }
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    for i in &packet.invocations {
        if i.case >= packet.plan.cases.len()
            || i.block >= packet.plan.blocks
            || i.order >= arms
            || i.arm >= arms
            || packet.schedule[i.case][i.block][i.order] != i.arm
            || !seen.insert((i.case, i.block, i.arm))
        {
            return Err("invalid/duplicate invocation membership".into());
        }
        if i.status == "completed"
            && (i.values_ns.len() != packet.plan.metric.expected
                || i.values_ns.iter().any(|v| !v.is_finite() || *v <= 0.0))
        {
            return Err("invalid retained completed metric vector".into());
        }
    }
    Ok(())
}
fn load_interrupted(path: &Path) -> Result<Packet, String> {
    let published = path.join("packet.json.gz");
    if published.exists() {
        return load(&published);
    }
    let mut packet: Packet = serde_json::from_slice(&read_limited(&path.join("experiment.json"))?)
        .map_err(|e| e.to_string())?;
    let events = read_limited(&path.join("events.jsonl"))?;
    let text = String::from_utf8(events).map_err(|e| e.to_string())?;
    let mut latest = BTreeMap::new();
    for (n, line) in text.split_inclusive('\n').enumerate() {
        // Keep a truncated final line in the original journal. Earlier malformed
        // records are corruption, not permission to discard evidence.
        if !line.ends_with('\n') {
            break;
        }
        let mut event: Invocation =
            serde_json::from_str(line).map_err(|e| format!("journal line {}: {e}", n + 1))?;
        if event.status == "completed" || event.status == "running" {
            event.status = "interrupted_unchecked".into();
            event.error = Some("interrupted before a checked block checkpoint".into());
        }
        latest.insert((event.case, event.block, event.arm), event);
    }
    for ci in 0..packet.plan.cases.len() {
        for block in 0..packet.plan.blocks {
            let checkpoint = path.join(format!("case-{ci}-block-{block}.json"));
            if checkpoint.exists() {
                let events: Vec<Invocation> = serde_json::from_slice(&read_limited(&checkpoint)?)
                    .map_err(|e| e.to_string())?;
                for event in events {
                    latest.insert((event.case, event.block, event.arm), event);
                }
            }
        }
    }
    packet.invocations = latest.into_values().collect();
    validate_packet(&packet)?;
    packet.summaries = summarize(&packet);
    packet.complete = (0..packet.plan.cases.len()).all(|ci| case_complete(&packet, ci));
    packet.decision = decision(&packet);
    Ok(packet)
}

#[cfg(test)]
mod confirmation_tests {
    use super::super::supervisor::Lifecycle;
    use super::*;
    use zenbench::platform::SystemState;
    fn quiet() -> SystemState {
        SystemState {
            cpu_load: 0.01,
            available_ram_bytes: 8 << 30,
            total_ram_bytes: 16 << 30,
            cpu_temp_c: Some(50.0),
            heavy_process_count: 0,
        }
    }
    fn at(start: Instant, ms: u64) -> Instant {
        start + Duration::from_millis(ms)
    }
    fn lived(start: Instant, alive_ms: u64, exit_ms: u64) -> Lifecycle {
        Lifecycle {
            last_alive: Some(at(start, alive_ms)),
            exit_observed: Some(at(start, exit_ms)),
        }
    }
    // A sampled window as the observer records it.
    fn window(state: SystemState, phase: &str) -> Value {
        let mut value = observation_value(&state, Duration::from_millis(5), phase);
        if phase == "start_transition" {
            value["used_for_admission"] = json!(false);
        }
        value
    }
    fn analyzed(sampled: &[Value]) -> Packet {
        let mut p = packet();
        for invocation in &mut p.invocations {
            invocation.observations = std::iter::once(window(quiet(), "boundary"))
                .chain(sampled.iter().cloned())
                .chain(std::iter::once(window(quiet(), "boundary")))
                .collect();
        }
        p.summaries = summarize(&p);
        p
    }
    fn resource_issue(p: &Packet) -> bool {
        p.summaries[0]["validity_issues"]
            .as_array()
            .unwrap()
            .contains(&json!("missing, stale or unclean resource observations"))
    }
    #[test]
    fn an_exit_during_a_counter_window_is_retained_as_a_transition() {
        let start = Instant::now();
        let foreign = json!({"phase":"during","cpu_load":0.3,"heavy_process_count":1,"clean":false,"ownership_independent_clean":false,"used_for_admission":true,"policy":"whole-host-strict-v1"});
        let terminal = json!({"phase":"during","cpu_load":0.16,"heavy_process_count":0,"clean":false,"ownership_independent_clean":true,"used_for_admission":true,"policy":"whole-host-strict-v1"});
        let samples = finish_resource_observations(
            vec![(start, foreign.clone()), (at(start, 20), terminal.clone())],
            lived(start, 10, 15),
            start,
        );
        assert_eq!(samples.len(), 2);
        // A real foreign observation before the exit observation stays
        // unchanged and still rejects admission.
        let mut expected_foreign = foreign;
        expected_foreign["sample_completed_after_start_ns"] = json!("0");
        expected_foreign["leader_last_alive_after_start_ns"] = json!("10000000");
        expected_foreign["leader_exit_observed_after_start_ns"] = json!("15000000");
        assert_eq!(samples[0], expected_foreign);
        // The window completing after the exit observation may contain the
        // reaped leader's unassignable CPU. Preserve its counters, without
        // misclassifying them as foreign work.
        assert_eq!(samples[1]["phase"], "exit_transition");
        assert_eq!(samples[1]["used_for_admission"], false);
        assert_eq!(samples[1]["admission_waiver"], "cpu_load_magnitude");
        assert_eq!(samples[1]["cpu_load"], terminal["cpu_load"]);
        assert_eq!(samples[1]["clean"], false);
    }
    #[test]
    fn transitions_never_waive_global_resource_failures_or_unknowns() {
        let start = Instant::now();
        for lifecycle in [Lifecycle::default(), lived(start, 10, 15)] {
            for phase in ["during", "start_transition", "exit_transition"] {
                for global in [Some(false), None] {
                    let mut sample = json!({"phase":phase,"cpu_load":0.16,"clean":false,"used_for_admission":true});
                    if let Some(global) = global {
                        sample["ownership_independent_clean"] = json!(global);
                    }
                    let result = finish_resource_observations(
                        vec![(at(start, 20), sample)],
                        lifecycle,
                        start,
                    );
                    assert_eq!(result[0]["used_for_admission"], true);
                    assert_eq!(result[0]["clean"], false);
                    assert_eq!(result[0]["admission_waiver"], Value::Null);
                }
            }
        }
    }
    #[test]
    fn no_proved_live_window_cannot_authorize_confirmation() {
        let start = Instant::now();
        let samples = vec![
            (
                start,
                json!({"phase":"start_transition","ownership_independent_clean":true,"clean":true,"used_for_admission":false,"policy":"whole-host-strict-v1"}),
            ),
            (
                at(start, 5),
                json!({"phase":"during","ownership_independent_clean":true,"clean":true,"used_for_admission":true,"policy":"whole-host-strict-v1"}),
            ),
            (
                at(start, 20),
                json!({"phase":"exit_transition","ownership_independent_clean":true,"clean":true,"used_for_admission":false,"policy":"whole-host-strict-v1"}),
            ),
        ];
        // Control: the same clean windows with an owned lifecycle are admissible.
        let p = analyzed(&finish_resource_observations(
            samples.clone(),
            lived(start, 10, 15),
            start,
        ));
        assert_eq!(exit_code(&p), 0);
        for lifecycle in [
            Lifecycle::default(),
            Lifecycle {
                last_alive: Some(at(start, 10)),
                exit_observed: None,
            },
            Lifecycle {
                last_alive: None,
                exit_observed: Some(at(start, 15)),
            },
            lived(start, 15, 15),
        ] {
            let mut p = packet();
            for invocation in &mut p.invocations {
                invocation.observations =
                    finish_resource_observations(samples.clone(), lifecycle, start);
                assert_eq!(
                    invocation.observations.last().unwrap()["phase"],
                    "lifecycle_unobserved"
                );
            }
            assert_eq!(analyze(&mut p), 2);
            assert_eq!(p.summaries[0]["verdict"], "invalid");
            assert!(resource_issue(&p));
        }
    }
    #[test]
    fn a_live_window_after_the_last_probe_stays_subject_to_admission() {
        // Alive at 100 ms, a foreign-load window completes at 110 ms, the leader
        // exits at 120 ms and the owned probe observes it at 125 ms.
        let start = Instant::now();
        let busy = SystemState {
            cpu_load: 0.3,
            ..quiet()
        };
        let classify = |at_110: Value| {
            finish_resource_observations(
                vec![
                    (start, window(quiet(), "start_transition")),
                    (at(start, 50), window(quiet(), "during")),
                    (at(start, 110), at_110),
                    (at(start, 130), window(busy.clone(), "during")),
                ],
                lived(start, 100, 125),
                start,
            )
        };
        let classified = classify(window(busy.clone(), "during"));
        assert_eq!(classified.len(), 4);
        assert_eq!(classified[2]["phase"], "during");
        assert_eq!(classified[2]["used_for_admission"], true);
        assert_eq!(classified[2]["clean"], false);
        assert_eq!(classified[2]["cpu_load"], 0.3);
        assert_eq!(classified[3]["phase"], "exit_transition");
        assert_eq!(classified[3]["used_for_admission"], false);
        let p = analyzed(&classified);
        assert_eq!(exit_code(&p), 2);
        assert_eq!(p.summaries[0]["verdict"], "invalid");
        assert!(resource_issue(&p));
        // Control: the same schedule with a quiet 110 ms window is admissible.
        let p = analyzed(&classify(window(quiet(), "during")));
        assert_eq!(exit_code(&p), 0);
        assert!(!resource_issue(&p));
    }
    #[test]
    fn only_windows_completing_at_or_after_the_exit_observation_are_terminal() {
        let start = Instant::now();
        let exit = at(start, 125);
        let busy = window(
            SystemState {
                cpu_load: 0.3,
                ..quiet()
            },
            "during",
        );
        let classified = finish_resource_observations(
            vec![
                (exit - Duration::from_nanos(1), busy.clone()),
                (exit, busy.clone()),
            ],
            lived(start, 100, 125),
            start,
        );
        assert_eq!(classified[0]["phase"], "during");
        assert_eq!(classified[0]["used_for_admission"], true);
        assert_eq!(classified[0]["clean"], false);
        assert_eq!(classified[1]["phase"], "exit_transition");
        assert_eq!(classified[1]["used_for_admission"], false);
        assert_eq!(
            classified[1]["leader_exit_observed_after_start_ns"],
            "125000000"
        );
    }
    #[test]
    fn a_missing_exit_observation_never_relabels_or_admits() {
        let start = Instant::now();
        let busy = SystemState {
            cpu_load: 0.3,
            ..quiet()
        };
        let lifecycle = Lifecycle {
            last_alive: Some(at(start, 100)),
            exit_observed: None,
        };
        let classified = finish_resource_observations(
            vec![
                (at(start, 50), window(quiet(), "during")),
                (at(start, 900), window(busy, "during")),
            ],
            lifecycle,
            start,
        );
        assert_eq!(classified[1]["phase"], "during");
        assert_eq!(classified[1]["used_for_admission"], true);
        assert_eq!(classified[1]["clean"], false);
        assert_eq!(
            classified[1]["leader_exit_observed_after_start_ns"],
            Value::Null
        );
        assert_eq!(classified[2]["phase"], "lifecycle_unobserved");
        // All-clean windows without an owned exit observation are still refused.
        let p = analyzed(&finish_resource_observations(
            vec![(at(start, 50), window(quiet(), "during"))],
            lifecycle,
            start,
        ));
        assert_eq!(exit_code(&p), 2);
        assert!(resource_issue(&p));
    }
    #[test]
    fn foreign_heavy_processes_invalidate_start_and_exit_windows() {
        let start = Instant::now();
        let heavy = SystemState {
            heavy_process_count: 1,
            ..quiet()
        };
        // Unassignable worker CPU raises only the whole-host magnitude.
        let start_cpu = SystemState {
            cpu_load: 0.2,
            ..quiet()
        };
        let exit_cpu = SystemState {
            cpu_load: 0.3,
            ..quiet()
        };
        let classify = |first: SystemState, last: SystemState| {
            finish_resource_observations(
                vec![
                    (start, window(first, "start_transition")),
                    (at(start, 50), window(quiet(), "during")),
                    (at(start, 130), window(last, "during")),
                ],
                lived(start, 100, 125),
                start,
            )
        };
        // Control: transitions carrying only unassignable CPU are admissible.
        let control = classify(start_cpu.clone(), exit_cpu.clone());
        for transition in [&control[0], &control[2]] {
            assert_eq!(transition["used_for_admission"], false);
            assert_eq!(transition["admission_waiver"], "cpu_load_magnitude");
        }
        assert_eq!(exit_code(&analyzed(&control)), 0);
        for (phase, first, last) in [
            (
                "start_transition",
                SystemState {
                    heavy_process_count: 1,
                    ..start_cpu.clone()
                },
                exit_cpu.clone(),
            ),
            (
                "exit_transition",
                start_cpu.clone(),
                SystemState {
                    heavy_process_count: 1,
                    ..exit_cpu.clone()
                },
            ),
            ("exit_transition", quiet(), heavy.clone()),
        ] {
            let classified = classify(first, last);
            let transition = classified.iter().find(|o| o["phase"] == phase).unwrap();
            assert_eq!(transition["heavy_process_count"], 1);
            assert_eq!(transition["used_for_admission"], true);
            assert_eq!(transition["clean"], false);
            assert_eq!(transition["admission_waiver"], Value::Null);
            let p = analyzed(&classified);
            assert_eq!(exit_code(&p), 2);
            assert_eq!(p.summaries[0]["verdict"], "invalid");
            assert!(resource_issue(&p));
        }
    }
    #[test]
    fn admission_checks_each_ownership_independent_resource() {
        let config = zenbench::GateConfig::strict();
        let ok = Duration::from_millis(5);
        let both = Admission {
            cpu_load_within_limit: true,
            ownership_independent: true,
        };
        assert_eq!(admission(&quiet(), ok), both);
        // Existing inclusive limits and unknown-temperature policy are unchanged.
        let edge = SystemState {
            cpu_load: config.max_cpu_load,
            available_ram_bytes: config.min_available_ram_bytes,
            cpu_temp_c: config.max_cpu_temp_c,
            heavy_process_count: config.max_heavy_processes,
            ..quiet()
        };
        assert_eq!(admission(&edge, Duration::from_secs(2)), both);
        let unknown_temperature = SystemState {
            cpu_temp_c: None,
            ..quiet()
        };
        assert_eq!(admission(&unknown_temperature, ok), both);
        let hot = config.max_cpu_temp_c.unwrap() + 0.1;
        for (name, state, duration) in [
            (
                "low RAM",
                SystemState {
                    available_ram_bytes: config.min_available_ram_bytes - 1,
                    ..quiet()
                },
                ok,
            ),
            (
                "stale",
                quiet(),
                Duration::from_secs(2) + Duration::from_nanos(1),
            ),
            (
                "over temperature",
                SystemState {
                    cpu_temp_c: Some(hot),
                    ..quiet()
                },
                ok,
            ),
            (
                "non-finite temperature",
                SystemState {
                    cpu_temp_c: Some(f64::NAN),
                    ..quiet()
                },
                ok,
            ),
            (
                "non-finite CPU",
                SystemState {
                    cpu_load: f64::NAN,
                    ..quiet()
                },
                ok,
            ),
            (
                "negative infinite CPU",
                SystemState {
                    cpu_load: f64::NEG_INFINITY,
                    ..quiet()
                },
                ok,
            ),
            (
                "foreign heavy process",
                SystemState {
                    heavy_process_count: config.max_heavy_processes + 1,
                    ..quiet()
                },
                ok,
            ),
        ] {
            assert!(!admission(&state, duration).ownership_independent, "{name}");
            let value = observation_value(&state, duration, "during");
            assert_eq!(value["ownership_independent_clean"], false, "{name}");
            assert_eq!(value["clean"], false, "{name}");
            assert_eq!(value["used_for_admission"], true, "{name}");
            assert_eq!(
                value["heavy_process_count"], state.heavy_process_count,
                "{name}"
            );
            assert_eq!(
                value["available_ram_bytes"], state.available_ram_bytes,
                "{name}"
            );
        }
        // Only the whole-host CPU magnitude depends on assigning worker CPU.
        let busy = SystemState {
            cpu_load: config.max_cpu_load + 0.01,
            ..quiet()
        };
        assert_eq!(
            admission(&busy, ok),
            Admission {
                cpu_load_within_limit: false,
                ownership_independent: true,
            }
        );
        let value = observation_value(&busy, ok, "during");
        assert_eq!(value["clean"], false);
        assert_eq!(value["ownership_independent_clean"], true);
        let value = observation_value(&quiet(), ok, "during");
        assert_eq!(value["clean"], true);
        assert_eq!(value["ownership_independent_clean"], true);
        assert_eq!(value["policy"], "whole-host-strict-v1");
    }
    #[test]
    fn a_live_snapshot_reports_its_own_admission() {
        let monitor = zenbench::platform::SystemMonitor::new();
        let value = observe(&monitor, &[std::process::id()], "boundary");
        let state = SystemState {
            cpu_load: value["cpu_load"].as_f64().unwrap_or(f64::NAN),
            available_ram_bytes: value["available_ram_bytes"].as_u64().unwrap(),
            total_ram_bytes: 0,
            cpu_temp_c: value["cpu_temp_c"].as_f64(),
            heavy_process_count: value["heavy_process_count"].as_u64().unwrap() as usize,
        };
        let duration = Duration::from_nanos(value["sample_duration_ns"].as_u64().unwrap());
        // A fresh monitor waits for the minimum CPU sampling interval.
        assert!(
            duration >= sysinfo::MINIMUM_CPU_UPDATE_INTERVAL / 2,
            "{value}"
        );
        let expected = admission(&state, duration);
        assert_eq!(
            value["ownership_independent_clean"],
            expected.ownership_independent
        );
        assert_eq!(
            value["clean"],
            expected.cpu_load_within_limit && expected.ownership_independent
        );
        assert_eq!(value["phase"], "boundary");
    }
    fn packet() -> Packet {
        let arm = |id| json!({"id":id,"program":"fixture","revision":"oracle"});
        let plan: Plan=serde_json::from_value(json!({"schema":1,"baseline":arm("base"),"candidates":[arm("edit")],
            "cases":[{"id":"one"}],"metric":{"extract":{"kind":"prefix","prefix":"TIME "},"unit":"ns","expected":2,"resolution":0.001},
            "check":{"kind":"files","paths":["result.bin"]},"warmup":"fixture","blocks":5,
            "confirmation":{"margin_pct":1.0,"control_margin_pct":1.0,"family_confidence":0.95,"scope":"oracle"}})).unwrap();
        plan.validate().unwrap();
        let mut packet = Packet {
            kind: "zenbench_pair".into(),
            mode: "confirm".into(),
            schema: 1,
            fingerprint: String::new(),
            plan,
            identities: Value::Null,
            context: Value::Null,
            seed: 1,
            schedule: schedule(1, 5, 3, 1),
            invocations: Vec::new(),
            summaries: Vec::new(),
            decision: Value::Null,
            complete: true,
            placement: json!({"status":"enforced_uniform"}),
            elapsed_seconds: 0.0,
            resume_from: None,
            commands: Value::Null,
        };
        for block in 0..5 {
            for (order, arm) in packet.schedule[0][block].iter().copied().enumerate() {
                packet.invocations.push(Invocation {
                    case: 0,
                    block,
                    arm,
                    order,
                    output: PathBuf::new(),
                    status: "completed".into(),
                    error: None,
                    values_ns: if arm == 1 {
                        vec![80.0, 100.0]
                    } else {
                        vec![90.0, 110.0]
                    },
                    wall_seconds: 0.0,
                    exit_code: Some(0),
                    process_id: Some(1),
                    continuation_safe: true,
                    observations: vec![
                        json!({"phase":"boundary","clean":true,"used_for_admission":true,"policy":"whole-host-strict-v1"}),
                        json!({"phase":"during","clean":true,"used_for_admission":true,"policy":"whole-host-strict-v1"}),
                        json!({"phase":"boundary","clean":true,"used_for_admission":true,"policy":"whole-host-strict-v1"}),
                    ],
                    artifacts: BTreeMap::new(),
                    correctness: Some(json!("same")),
                    priority: Value::Null,
                });
            }
        }
        packet
    }
    fn analyze(p: &mut Packet) -> i32 {
        p.summaries = summarize(p);
        exit_code(p)
    }
    #[test]
    fn fixed_block_decisions_and_indivisible_repetitions() {
        let mut p = packet();
        assert_eq!(analyze(&mut p), 0);
        assert_eq!(p.summaries[0]["verdict"], "improvement");
        assert_eq!(p.summaries[0]["interval"]["blocks"], 5);
        assert_eq!(p.summaries[0]["interval"]["comparisons_in_family"], 2);
        for i in &mut p.invocations {
            if i.arm == 1 {
                i.values_ns = vec![110.0, 130.0];
            }
        }
        assert_eq!(analyze(&mut p), 1);
        assert_eq!(p.summaries[0]["verdict"], "regression");
        for i in &mut p.invocations {
            if i.arm == 1 {
                i.values_ns = vec![90.0, 110.0];
            }
        }
        assert_eq!(analyze(&mut p), 0);
        assert_eq!(p.summaries[0]["verdict"], "equivalent");
    }
    #[test]
    fn incomplete_confirmation_contracts_are_refused_before_execution() {
        for (pointer, value) in [
            ("/blocks", json!(4)),
            ("/aa", json!(false)),
            ("/metric/resolution", json!(0)),
            ("/confirmation/control_margin_pct", json!(0)),
            ("/confirmation/scope", json!("")),
            ("/confirmation/accept", json!("p_value")),
            ("/confirmation/family_confidence", json!(1)),
            ("/check", json!({"kind":"unchecked","reason":"exploration"})),
        ] {
            let mut value_plan = serde_json::to_value(packet().plan).unwrap();
            *value_plan.pointer_mut(pointer).unwrap() = value;
            let plan: Plan = serde_json::from_value(value_plan).unwrap();
            assert!(plan.validate().is_err(), "accepted invalid field {pointer}");
        }
    }
    #[test]
    fn declared_acceptance_rule_is_not_replaced_by_significance() {
        let mut p = packet();
        p.plan.confirmation.as_mut().unwrap().accept = "equivalence".into();
        assert_eq!(analyze(&mut p), 2); // An improvement does not establish equivalence.
        p.plan.confirmation.as_mut().unwrap().accept = "improvement".into();
        assert_eq!(analyze(&mut p), 0);
        for i in &mut p.invocations {
            if i.arm == 1 {
                i.values_ns = vec![90.0, 110.0];
            }
        }
        assert_eq!(analyze(&mut p), 2); // Equivalence does not establish improvement.
    }
    #[test]
    fn failed_control_noise_and_missing_data_cannot_accept() {
        let mut p = packet();
        for i in &mut p.invocations {
            if i.arm == 2 {
                i.values_ns = vec![95.0, 115.0];
            }
        }
        assert_eq!(analyze(&mut p), 2);
        assert_eq!(p.summaries[0]["verdict"], "inconclusive");
        let mut p = packet();
        p.invocations[0].observations[1]["clean"] = json!(false);
        assert_eq!(analyze(&mut p), 2);
        assert_eq!(p.summaries[0]["verdict"], "invalid");
        let mut p = packet();
        p.invocations.pop();
        p.complete = false;
        assert_eq!(analyze(&mut p), 2);
        assert_eq!(p.summaries[0]["complete_blocks"], 4);
        let mut p = packet();
        p.invocations[0].status = "failed".into();
        assert_eq!(analyze(&mut p), 2);
        assert_eq!(p.summaries[0]["complete_blocks"], 4);
        let mut p = packet();
        p.placement["status"] = json!("unrestricted");
        assert_eq!(analyze(&mut p), 2);
    }
    #[test]
    fn resolution_and_all_declared_cells_determine_family() {
        let mut p = packet();
        p.plan.metric.resolution = Some(2.0);
        assert_eq!(analyze(&mut p), 2);
        assert_eq!(p.summaries[0]["control_equivalent"], false);
        let mut p = packet();
        p.plan.cases.push(p.plan.cases[0].clone());
        p.plan.cases[1].id = "missing".into();
        p.complete = false;
        assert_eq!(analyze(&mut p), 2);
        assert_eq!(p.summaries[0]["interval"]["comparisons_in_family"], 4);
        assert_eq!(p.summaries[2]["verdict"], "invalid");
    }
}
