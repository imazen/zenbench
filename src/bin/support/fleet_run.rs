//! A single planned fleet observation. The fleet owns scheduling and retries.
use super::fleet::{self, Bundle};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(clap::Args)]
pub(crate) struct Options {
    pub bundle: PathBuf,
    #[arg(long)]
    pub output: PathBuf,
    #[arg(long)]
    pub experiment: String,
    #[arg(long)]
    pub machine: String,
    #[arg(long)]
    pub domain: String,
    #[arg(long)]
    pub replicate: u32,
    #[arg(long)]
    pub attempt: String,
    /// Maximum admission wait; zero records busy immediately.
    #[arg(long, default_value_t = 0)]
    pub wait: u64,
    /// Creating this file cancels admission/running work and reaps the child group.
    #[arg(long)]
    pub cancel: Option<PathBuf>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    schema: u32,
    program: PathBuf,
    #[serde(default)]
    args: Vec<String>,
    /// Explicit overrides are retained. Inherited environment is not a secret dump.
    #[serde(default)]
    env: BTreeMap<String, String>,
    backend: Backend,
    timeout_seconds: u64,
    operations: u64,
    expected_correctness: Value,
    /// Input/color/layout/reuse/thread/SIMD contract owned by the driver.
    workload: Value,
}
#[derive(Deserialize, Serialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Backend {
    Wall,
    Heaptrack,
    OsPeak,
    Driver,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DriverResult {
    schema: u32,
    operations: u64,
    correctness: Value,
    #[serde(default)]
    memory: Vec<Memory>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Memory {
    quantity: String,
    bytes: Option<u64>,
    unavailable_reason: Option<String>,
    backend: String,
    backend_version: String,
    coverage: String,
    scope: String,
}

fn limited(path: &Path) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(8 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err(io::Error::other("metadata exceeds 8 MiB"));
    }
    Ok(bytes)
}
fn failure(kind: &str, reason: impl ToString) -> Value {
    json!({"kind":kind,"reason":reason.to_string()})
}
fn error(e: impl ToString) -> Value {
    failure("incomplete", e)
}
fn now() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .to_string()
}

pub(crate) fn run(options: Options) -> io::Result<i32> {
    for s in [
        &options.experiment,
        &options.machine,
        &options.domain,
        &options.attempt,
    ] {
        if s.trim().is_empty() {
            return Err(io::Error::other(
                "experiment, machine, domain and attempt must be nonempty",
            ));
        }
    }
    let output = std::path::absolute(&options.output)?;
    fs::create_dir_all(
        output
            .parent()
            .ok_or_else(|| io::Error::other("output has no parent"))?,
    )?;
    fs::create_dir(&output)?;
    fs::write(output.join(".gitignore"), "*\n")?;
    let started = Instant::now();
    let mut journal = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(output.join("events.jsonl"))?;
    let mut last = Instant::now().checked_sub(Duration::from_secs(2)).unwrap();
    let mut poll = || -> io::Result<()> {
        if options.cancel.as_ref().is_some_and(|p| p.exists()) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "cancel file observed",
            ));
        }
        if last.elapsed() >= Duration::from_secs(1) {
            writeln!(
                journal,
                "{}",
                json!({"event":"heartbeat","unix_ns":now(),"elapsed_seconds":started.elapsed().as_secs_f64(),"attempt":options.attempt})
            )?;
            journal.flush()?;
            last = Instant::now();
        }
        Ok(())
    };
    let mut receipt = json!({"kind":"zenbench_fleet_attempt","schema":1,
        "experiment":options.experiment,"machine":options.machine,"domain":options.domain,
        "replicate":options.replicate,"attempt":options.attempt,"status":"incomplete",
        "runner":fleet::capabilities()?,"started_unix_ns":now(),"memory":[],"inference":"descriptive individual observation; no acceptance decision","environment_limit":"inherits worker environment plus retained explicit overrides; not hermetic",
        "commands":{"decode":"zenbench fleet-read ATTEMPT_DIRECTORY","compare":"compare only matching bundle, machine, domain, backend and workload contracts"}});
    fs::write(output.join("started.json"), serde_json::to_vec(&receipt)?)?;
    let result = execute(&options, &output, &mut receipt, &mut poll);
    match result {
        Ok(()) => receipt["status"] = json!("complete"),
        Err(e) => {
            receipt["status"] = e["kind"].clone();
            receipt["failure"] = e;
        }
    }
    receipt["elapsed_seconds"] = json!(started.elapsed().as_secs_f64());
    receipt["finished_unix_ns"] = json!(now());
    let mut artifacts = BTreeMap::new();
    for entry in fs::read_dir(&output)? {
        let entry = entry?;
        if entry.file_type()?.is_file() && entry.file_name() != ".gitignore" {
            artifacts.insert(
                entry.file_name().to_string_lossy().into_owned(),
                fleet::hash_file(&entry.path())?,
            );
        }
    }
    receipt["artifacts"] = json!(artifacts);
    fs::write(output.join("packet.json"), serde_json::to_vec(&receipt)?)?;
    println!("{}", serde_json::to_string(&receipt)?);
    Ok(if receipt["status"] == "complete" {
        0
    } else {
        2
    })
}

fn execute(
    options: &Options,
    output: &Path,
    receipt: &mut Value,
    poll: &mut dyn FnMut() -> io::Result<()>,
) -> Result<(), Value> {
    poll().map_err(classify)?;
    let root = options.bundle.canonicalize().map_err(error)?;
    let bundle = fleet::verify(&root).map_err(|e| failure("identity_mismatch", e))?;
    let plan: Plan =
        serde_json::from_value(bundle.plan.clone()).map_err(|e| failure("invalid_plan", e))?;
    validate(&bundle, &plan).map_err(|e| failure("invalid_plan", e))?;
    receipt["bundle_sha256"] = json!(fleet::hash_file(&root.join("bundle.json")).map_err(error)?);
    receipt["plan"] = serde_json::to_value(&plan).map_err(error)?;
    let placement = zenbench::platform::check_cpu_placement()
        .map_err(|e| failure("unsupported_placement", e))?;
    receipt["placement"] = serde_json::to_value(&placement).map_err(error)?;
    let mut reservation = crate::coordination::Reservation::acquire_controlled(
        "fleet observation",
        Duration::from_secs(options.wait),
        poll,
    )
    .map_err(|e| {
        if e.kind() == io::ErrorKind::Interrupted {
            classify(e)
        } else {
            failure("busy", e)
        }
    })?;
    let monitor = zenbench::platform::SystemMonitor::new();
    let admission = Instant::now();
    loop {
        poll().map_err(classify)?;
        let state = monitor.snapshot();
        receipt["resources_before"] = state_json(&state);
        if state.available_ram_bytes >= 1024 * 1024 * 1024 && state.cpu_load <= 0.8 {
            break;
        }
        if admission.elapsed() >= Duration::from_secs(options.wait) {
            return Err(failure("busy", "resource admission expired"));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if plan.backend == Backend::OsPeak && cfg!(unix) && !Path::new("/usr/bin/time").is_file() {
        return Err(failure(
            "unsupported",
            "/usr/bin/time is required for OS resident high-water accounting",
        ));
    }
    let program = root.join(&plan.program);
    if plan.backend == Backend::Heaptrack {
        let mut identities = BTreeMap::new();
        for name in ["heaptrack", "heaptrack_print"] {
            let path = fleet::tool(name)
                .ok_or_else(|| failure("unsupported", format!("{name} is not installed")))?;
            identities.insert(
                name,
                json!({"path":path,"sha256":fleet::hash_file(&path).map_err(error)?}),
            );
        }
        receipt["profiler_tools"] = json!(identities);
    }
    let mut command = match plan.backend {
        Backend::Wall | Backend::Driver => Command::new(&program),
        Backend::OsPeak => {
            if cfg!(windows) {
                Command::new(&program)
            } else if cfg!(target_os = "linux") {
                let mut c = Command::new("/usr/bin/time");
                c.args(["-f", "%M", "-o"])
                    .arg(output.join("max-rss-kib.txt"))
                    .arg("--")
                    .arg(&program);
                c
            } else if cfg!(target_os = "macos") {
                let mut c = Command::new("/usr/bin/time");
                c.arg("-l").arg(&program);
                c
            } else {
                return Err(failure(
                    "unsupported",
                    "OS high-water backend unsupported on this OS",
                ));
            }
        }
        Backend::Heaptrack => {
            if !cfg!(target_os = "linux") {
                return Err(failure("unsupported", "heaptrack adapter requires Linux"));
            }
            let mut c = Command::new(
                receipt["profiler_tools"]["heaptrack"]["path"]
                    .as_str()
                    .unwrap(),
            );
            c.arg("--record-only")
                .arg("-o")
                .arg(output.join("heaptrack.gz"))
                .arg(&program);
            c
        }
    };
    command
        .args(&plan.args)
        .current_dir(&root)
        .envs(&plan.env)
        .env("ZENBENCH_RESULT_FILE", output.join("driver.json"))
        .env("ZENBENCH_OPERATIONS", plan.operations.to_string())
        .stdin(Stdio::null())
        .stdout(File::create(output.join("stdout.txt")).map_err(error)?)
        .stderr(File::create(output.join("stderr.txt")).map_err(error)?);
    let begin = Instant::now();
    let mut pid = None;
    let status = super::supervisor::run_controlled(
        command,
        Duration::from_secs(plan.timeout_seconds),
        &mut reservation,
        None,
        &[
            output.join("stdout.txt"),
            output.join("stderr.txt"),
            output.join("driver.json"),
        ],
        &mut pid,
        poll,
    );
    receipt["process_id"] = json!(pid);
    receipt["wall_seconds"] = json!(begin.elapsed().as_secs_f64());
    receipt["resources_after"] = state_json(&monitor.snapshot());
    let outcome = status.map_err(classify)?;
    let status = outcome.status;
    receipt["exit_code"] = json!(status.code());
    if !status.success() {
        return Err(failure("child_failed", format!("{status}")));
    }
    if zenbench::platform::check_cpu_placement().map_err(error)? != placement {
        return Err(failure(
            "placement_changed",
            "CPU placement changed during execution",
        ));
    }
    fleet::verify(&root).map_err(|e| failure("identity_mismatch", e))?;
    let driver: DriverResult =
        serde_json::from_slice(&limited(&output.join("driver.json")).map_err(error)?)
            .map_err(error)?;
    if driver.schema != 1 || driver.operations != plan.operations {
        return Err(failure(
            "wrong_output",
            "driver schema or operation count mismatch",
        ));
    }
    if driver.correctness != plan.expected_correctness {
        return Err(failure("wrong_output", "correctness mismatch"));
    }
    receipt["correctness"] = driver.correctness;
    let mut memory = Vec::new();
    match plan.backend {
        Backend::Wall => {
            if !driver.memory.is_empty() {
                return Err(failure(
                    "invalid_measurement",
                    "wall runs must not contain instrumented memory evidence",
                ));
            }
        }
        Backend::Driver => {
            if driver.memory.is_empty() {
                return Err(failure(
                    "invalid_measurement",
                    "driver must return memory evidence",
                ));
            }
            for m in &driver.memory {
                validate_memory(m).map_err(error)?;
            }
            memory = driver.memory;
        }
        Backend::OsPeak => {
            if let Some(counters) = outcome.windows_peaks {
                for (index, quantity, counter) in [
                    (0, "resident_high_water", "PeakWorkingSetSize"),
                    (1, "private_commit_high_water", "PeakPagefileUsage"),
                ] {
                    let (bytes, reason) = match &counters {
                        Ok((rss, commit)) => (Some(if index == 0 { *rss } else { *commit }), None),
                        Err(e) => (None, Some(e.clone())),
                    };
                    memory.push(Memory {quantity:quantity.into(),bytes,unavailable_reason:reason,backend:format!("GetProcessMemoryInfo/{counter}"),backend_version:sysinfo::System::long_os_version().unwrap_or_else(||"unknown Windows version".into()),coverage:"OS process-lifetime counter; excludes child processes; not requested heap".into(),scope:"whole_child".into()});
                }
            } else {
                let (path, quantity, backend, bytes) = if cfg!(target_os = "linux") {
                    let path = output.join("max-rss-kib.txt");
                    let text = String::from_utf8(limited(&path).map_err(error)?).map_err(error)?;
                    let kib = text.trim().parse::<u64>().map_err(error)?;
                    (
                        path,
                        "resident_high_water",
                        "gnu_time_ru_maxrss",
                        kib.checked_mul(1024).ok_or_else(|| error("RSS overflow"))?,
                    )
                } else {
                    let path = output.join("stderr.txt");
                    let text = String::from_utf8(limited(&path).map_err(error)?).map_err(error)?;
                    let values: Vec<_> = text
                        .lines()
                        .filter_map(|line| line.trim().strip_suffix(" maximum resident set size"))
                        .collect();
                    if values.len() != 1 {
                        return Err(error("expected one macOS time resident high-water counter"));
                    }
                    let bytes = values[0].trim().parse::<u64>().map_err(error)?;
                    (path, "resident_high_water", "macos_time_ru_maxrss", bytes)
                };
                receipt["os_counter_source"] = json!(path.file_name());
                memory.push(Memory {quantity:quantity.into(),bytes:Some(bytes),unavailable_reason:None,backend:backend.into(),backend_version:fleet::hash_file(Path::new("/usr/bin/time")).map_err(error)?,coverage:"OS wait resource accounting; not heap; descendant high-water accounting is not a simultaneous tree sum".into(),scope:"whole_child".into()});
            }
        }
        Backend::Heaptrack => {
            let traces: Vec<_> = fs::read_dir(output)
                .map_err(error)?
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .unwrap()
                        .to_string_lossy()
                        .starts_with("heaptrack")
                        && p.extension().is_some_and(|e| e == "gz" || e == "zst")
                })
                .collect();
            if traces.len() != 1 {
                return Err(error("expected exactly one heaptrack trace"));
            }
            let mut parser = Command::new(
                receipt["profiler_tools"]["heaptrack_print"]["path"]
                    .as_str()
                    .unwrap(),
            );
            parser
                .arg("-f")
                .arg(&traces[0])
                .arg("--print-flamegraph")
                .arg(output.join("peak.stacks"))
                .args([
                    "--flamegraph-cost-type=peak",
                    "--print-peaks=0",
                    "--print-allocators=0",
                    "--print-temporary=0",
                    "--print-leaks=0",
                ])
                .stdout(File::create(output.join("heaptrack-summary.txt")).map_err(error)?)
                .stderr(File::create(output.join("heaptrack-parser-stderr.txt")).map_err(error)?);
            let status = super::supervisor::run_controlled(
                parser,
                Duration::from_secs(plan.timeout_seconds),
                &mut reservation,
                None,
                &[output.join("peak.stacks")],
                &mut None,
                poll,
            )
            .map_err(classify)?;
            if !status.status.success() {
                return Err(error("heaptrack_print failed"));
            }
            let text = String::from_utf8(limited(&output.join("peak.stacks")).map_err(error)?)
                .map_err(error)?;
            let peak = stack_peak(&text).map_err(error)?;
            memory.push(Memory {quantity:"requested_live_heap_peak".into(),bytes:Some(peak),unavailable_reason:None,backend:"heaptrack_global_peak_stacks".into(),backend_version:receipt["profiler_tools"]["heaptrack_print"]["sha256"].as_str().unwrap().into(),coverage:"intercepted allocator events in instrumented execution; excludes direct mappings, hidden arenas and device allocations".into(),scope:"whole_child".into()});
        }
    }
    receipt["memory"] = serde_json::to_value(memory).map_err(error)?;
    Ok(())
}
fn classify(e: io::Error) -> Value {
    failure(
        match e.kind() {
            io::ErrorKind::Interrupted => "cancelled",
            io::ErrorKind::TimedOut => "timeout",
            _ => "incomplete",
        },
        e,
    )
}
fn validate(bundle: &Bundle, p: &Plan) -> io::Result<()> {
    if p.schema != 1
        || p.operations == 0
        || p.timeout_seconds == 0
        || p.workload.is_null()
        || p.expected_correctness.is_null()
    {
        return Err(io::Error::other(
            "schema 1, positive operations/timeout, workload and correctness are required",
        ));
    }
    fleet::relative(&p.program)?;
    if !bundle.files.get(&p.program).is_some_and(|f| f.executable) {
        return Err(io::Error::other("program must be a listed executable"));
    }
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        s => s,
    };
    if !bundle
        .target
        .starts_with(&format!("{}-", std::env::consts::ARCH))
        || !bundle.target.contains(os)
    {
        return Err(io::Error::other(
            "bundle target does not match runner process OS/architecture",
        ));
    }
    for k in p.env.keys() {
        if k.is_empty()
            || k.contains(['=', '\0'])
            || k.to_ascii_uppercase().starts_with("ZENBENCH_COORDINATION_")
            || k.eq_ignore_ascii_case("ZENBENCH_RESULT_FILE")
        {
            return Err(io::Error::other("invalid or reserved environment override"));
        }
    }
    Ok(())
}
fn validate_memory(m: &Memory) -> io::Result<()> {
    if m.bytes.is_none()
        != m.unavailable_reason
            .as_ref()
            .is_some_and(|s| !s.trim().is_empty())
        || [
            &m.quantity,
            &m.backend,
            &m.backend_version,
            &m.coverage,
            &m.scope,
        ]
        .iter()
        .any(|s| s.trim().is_empty())
    {
        return Err(io::Error::other(
            "memory requires named quantity/backend/version/coverage/scope and bytes OR unavailable reason",
        ));
    }
    Ok(())
}
fn stack_peak(text: &str) -> io::Result<u64> {
    // heaptrack's second pass sets every allocation.peak to its live count
    // at the SAME global maximum. These are additive contributions, not
    // independent call-site maxima. Keep integers; never parse rounded MB.
    let mut total = 0u64;
    let mut count = 0usize;
    for line in text.lines() {
        let (_, bytes) = line
            .rsplit_once(' ')
            .ok_or_else(|| io::Error::other("invalid peak stack row"))?;
        let bytes = bytes.parse::<u64>().map_err(io::Error::other)?;
        total = total
            .checked_add(bytes)
            .ok_or_else(|| io::Error::other("peak overflow"))?;
        count += 1;
    }
    if count == 0 {
        return Err(io::Error::other(
            "heaptrack export contains no byte contributions",
        ));
    }
    Ok(total)
}
pub(crate) fn read(directory: &Path) -> io::Result<Value> {
    let v: Value = serde_json::from_slice(&limited(&directory.join("packet.json"))?)?;
    if v["kind"] != "zenbench_fleet_attempt" || v["schema"] != 1 {
        return Err(io::Error::other("unsupported fleet packet"));
    }
    Ok(v)
}
fn state_json(s: &zenbench::platform::SystemState) -> Value {
    json!({"cpu_load":s.cpu_load,"available_ram_bytes":s.available_ram_bytes,"total_ram_bytes":s.total_ram_bytes,"cpu_temp_c":s.cpu_temp_c,"heavy_process_count":s.heavy_process_count,"cleanliness_certified":false})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_integer_global_peak_contributions() {
        assert_eq!(
            stack_peak("root;worker 9007199254740992\nroot;other 1\n").unwrap(),
            9007199254740993
        );
        assert!(stack_peak("root 1.2MB").is_err());
        assert!(stack_peak("").is_err());
        assert!(stack_peak("root 18446744073709551615\nother 1\n").is_err());
    }
}
