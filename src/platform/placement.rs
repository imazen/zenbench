//! Process-level CPU class policy. No pool ownership or thread enumeration is
//! needed when the OS enforces a fixed process boundary.

use serde::{Deserialize, Serialize};
#[cfg(any(target_os = "linux", target_os = "windows", test))]
use std::collections::BTreeMap;
#[cfg(target_os = "linux")]
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CpuPolicy {
    /// Require uniform hardware, or an enforced process set in one known class.
    #[default]
    Auto,
    /// Require enforced placement on the highest reported performance class.
    Performance,
    /// Require enforced placement on the lowest reported performance class.
    Efficiency,
    /// Explicitly permit unverified placement. Results retain this opt-out.
    Unrestricted,
}

impl CpuPolicy {
    pub(crate) fn parse(value: &str) -> Result<Self, String> {
        match value {
            "auto" => Ok(Self::Auto),
            "performance" => Ok(Self::Performance),
            "efficiency" => Ok(Self::Efficiency),
            "unrestricted" => Ok(Self::Unrestricted),
            _ => Err(format!(
                "invalid CPU policy {value:?}; use auto, performance, efficiency, or unrestricted"
            )),
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Performance => "performance",
            Self::Efficiency => "efficiency",
            Self::Unrestricted => "unrestricted",
        }
    }
}

impl std::str::FromStr for CpuPolicy {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl std::fmt::Display for CpuPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum TopologyKind {
    Homogeneous,
    Asymmetric,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CoreClass {
    pub name: String,
    /// Logical CPU IDs; empty when the OS exposes counts without an ID mapping.
    pub cpus: Vec<usize>,
    pub logical_cpu_count: usize,
    /// Larger ranks mean higher performance according to the OS/CPU interface.
    /// Cache-size differences alone do not establish a performance ranking.
    pub performance_rank: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CpuTopology {
    pub kind: TopologyKind,
    pub classes: Vec<CoreClass>,
    pub source: String,
    pub diagnostic: Option<String>,
}

impl CpuTopology {
    fn unknown(message: impl Into<String>) -> Self {
        Self {
            kind: TopologyKind::Unknown,
            classes: vec![],
            source: "unavailable".into(),
            diagnostic: Some(message.into()),
        }
    }

    fn class_for(&self, cpus: &[usize]) -> Option<&CoreClass> {
        if cpus.is_empty() {
            return None;
        }
        self.classes
            .iter()
            .find(|class| cpus.iter().all(|cpu| class.cpus.contains(cpu)))
    }

    /// Select a completely known performance/efficiency class for a launcher.
    /// Unranked cache asymmetry requires an explicit CPU set instead.
    pub fn select(&self, policy: CpuPolicy) -> Result<Vec<usize>, String> {
        if self.kind == TopologyKind::Unknown
            || !matches!(policy, CpuPolicy::Performance | CpuPolicy::Efficiency)
        {
            return Err(
                "class selection requires complete topology and a performance/efficiency policy"
                    .into(),
            );
        }
        if self.source == "windows_process_cpuid" && self.kind == TopologyKind::Asymmetric {
            let required = if policy == CpuPolicy::Performance {
                "performance"
            } else {
                "efficiency"
            };
            if !self.classes.iter().any(|c| c.name.starts_with(required)) {
                return Err(format!(
                    "requested {required} CPUs are unavailable in this process mask; launch a fresh process with the desired class"
                ));
            }
        }
        let ranks: Vec<_> = self
            .classes
            .iter()
            .filter_map(|c| c.performance_rank)
            .collect();
        if ranks.len() != self.classes.len() || ranks.is_empty() {
            return Err(
                "CPU classes have no complete performance ranking; select explicit --cpu-set=IDs"
                    .into(),
            );
        }
        let rank = if policy == CpuPolicy::Performance {
            ranks.iter().max()
        } else {
            ranks.iter().min()
        }
        .unwrap();
        let candidates: Vec<_> = self
            .classes
            .iter()
            .filter(|c| c.performance_rank == Some(*rank))
            .collect();
        if candidates.len() != 1 || candidates[0].cpus.is_empty() {
            return Err("CPU class cannot be mapped to one unambiguous CPU set; select explicit --cpu-set=IDs on a supported platform".into());
        }
        Ok(candidates[0].cpus.clone())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PlacementStatus {
    /// Known uniform CPU classes; an OS placement restriction is not required.
    UniformHardware,
    /// An OS process boundary was verified to contain one known CPU class.
    EnforcedUniform,
    /// The caller explicitly opted out; uniform placement is not certified.
    Unrestricted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CpuPlacement {
    pub policy: CpuPolicy,
    pub topology: CpuTopology,
    pub status: PlacementStatus,
    pub core_class: Option<String>,
    pub effective_cpus: Vec<usize>,
    pub requested_cpus: Option<Vec<usize>>,
    /// Scoped mask requested for the coordinator and zenbench-owned workers.
    pub thread_affinity_requested: Option<Vec<usize>>,
    /// Coordinator's observed mask; opaque external workers are not queried.
    pub thread_affinity_effective: Option<Vec<usize>>,
    /// "cpuset", "job_affinity", "process_mask", or "none".
    /// A thread mask is never enforcement.
    pub enforcement: String,
}

impl CpuPlacement {
    /// Logical CPUs inside this already-verified process boundary but
    /// **outside the coordinator/measurement mask**, offered to the resource
    /// observer when measurement threads are pinned to a subset.
    ///
    /// Returns the difference `effective_cpus - measurement`: only on
    /// Linux/Windows, only when the boundary is verified
    /// ([`PlacementStatus::UniformHardware`] or
    /// [`PlacementStatus::EnforcedUniform`]), and only when it is non-empty.
    /// `None` keeps ordinary inherited affinity: no explicit measurement pin,
    /// an unverified/unrestricted boundary, or nothing outside the pin.
    ///
    /// This is selection *outside the coordinator mask*, not proven dedicated
    /// isolation: pre-existing opaque external thread pools (created before
    /// the group pin, never queried by zenbench) may already run on those
    /// CPUs. See `docs/cpu-placement-verification.md` "Existing external
    /// workers".
    pub(crate) fn observer_cpus(&self, measurement: Option<&[usize]>) -> Option<Vec<usize>> {
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        {
            if !matches!(
                self.status,
                PlacementStatus::UniformHardware | PlacementStatus::EnforcedUniform
            ) {
                return None;
            }
            let measurement = measurement?;
            let mut spare: Vec<usize> = self
                .effective_cpus
                .iter()
                .copied()
                .filter(|cpu| !measurement.contains(cpu))
                .collect();
            spare.sort_unstable();
            spare.dedup();
            (!spare.is_empty()).then_some(spare)
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        {
            let _ = measurement;
            None
        }
    }
}

impl CpuPlacement {
    pub(crate) fn llm(&self) -> String {
        format!(
            "cpu_policy={} cpu_placement={} cpu_class={} cpu_enforcement={} cpu_effective_set={} cpu_requested_set={} cpu_thread_requested={} cpu_thread_effective={} cpu_class_uniform={} pool_thread_count=unknown cpu_frequency_verified=false",
            self.policy.name(),
            match self.status {
                PlacementStatus::UniformHardware => "uniform_hardware",
                PlacementStatus::EnforcedUniform => "enforced_uniform",
                PlacementStatus::Unrestricted => "UNVERIFIED_UNRESTRICTED",
            },
            serde_json::to_string(&self.core_class).expect("serializing CPU class"),
            self.enforcement,
            serde_json::to_string(&self.effective_cpus).expect("serializing CPU IDs"),
            serde_json::to_string(&self.requested_cpus).expect("serializing requested CPUs"),
            serde_json::to_string(&self.thread_affinity_requested)
                .expect("serializing requested thread CPUs"),
            serde_json::to_string(&self.thread_affinity_effective)
                .expect("serializing effective thread CPUs"),
            self.status != PlacementStatus::Unrestricted
        )
    }

    pub(crate) fn compatible(&self, other: &Self) -> bool {
        self.process_compatible(other)
            && self.thread_affinity_requested == other.thread_affinity_requested
            && self.thread_affinity_effective == other.thread_affinity_effective
    }

    pub(crate) fn process_compatible(&self, other: &Self) -> bool {
        self.status == other.status
            && self.core_class == other.core_class
            && self.effective_cpus == other.effective_cpus
            && self.enforcement == other.enforcement
            && self.topology.kind == other.topology.kind
    }

    pub(crate) fn verify_thread(&self) -> Result<(), String> {
        if let Some(expected) = &self.thread_affinity_effective {
            let current = super::current_thread_affinity().map_err(|e| e.to_string())?;
            if &current != expected {
                return Err("coordinator affinity changed during the run; restore nested affinity scopes before returning".into());
            }
        }
        Ok(())
    }
}

#[cfg(any(target_os = "linux", target_os = "windows", test))]
fn group_classes(
    entries: Vec<(usize, String, Option<u32>)>,
    source: &str,
    incomplete: bool,
) -> CpuTopology {
    let mut map: BTreeMap<(String, Option<u32>), Vec<usize>> = BTreeMap::new();
    for (cpu, name, rank) in entries {
        map.entry((name, rank)).or_default().push(cpu);
    }
    let classes: Vec<_> = map
        .into_iter()
        .map(|((name, performance_rank), mut cpus)| {
            cpus.sort_unstable();
            cpus.dedup();
            CoreClass {
                name,
                logical_cpu_count: cpus.len(),
                cpus,
                performance_rank,
            }
        })
        .collect();
    let kind = if incomplete || classes.is_empty() {
        TopologyKind::Unknown
    } else if classes.len() > 1 {
        TopologyKind::Asymmetric
    } else {
        TopologyKind::Homogeneous
    };
    CpuTopology {
        kind,
        classes,
        source: source.into(),
        diagnostic: incomplete.then(|| "some online CPUs could not be classified".into()),
    }
}

#[cfg(target_os = "linux")]
fn read_cpus(path: &Path) -> Result<Vec<usize>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    super::affinity::parse_cpu_list(text.trim()).map_err(|e| e.to_string())
}

#[cfg(any(
    target_arch = "x86_64",
    all(target_arch = "x86", target_feature = "sse")
))]
fn x86_base_class() -> (String, Option<u32>, bool) {
    use raw_cpuid::native_cpuid::cpuid_count as cpuid;
    let maximum = cpuid(0, 0).eax;
    let hybrid = maximum >= 7 && cpuid(7, 0).edx & (1 << 15) != 0;
    if hybrid {
        if maximum < 0x1a {
            return ("unknown".into(), None, false);
        }
        let native = cpuid(0x1a, 0).eax & 0x00ff_ffff;
        return match cpuid(0x1a, 0).eax >> 24 {
            0x40 => (format!("performance_model{native}"), Some(2), true),
            0x20 => (format!("efficiency_model{native}"), Some(1), true),
            _ => ("unknown".into(), None, false),
        };
    }
    if cpuid(0x8000_0000, 0).eax >= 0x8000_0026 {
        for level in 0..8 {
            let value = cpuid(0x8000_0026, level);
            if (value.ecx >> 8) & 0xff == 1 {
                let rank = (value.ebx >> 16) & 0xff;
                let core_type = value.ebx >> 28;
                // Power-efficiency ranking is not a performance ranking.
                // Preserve the reported identity without choosing faster cores.
                let native = (value.ebx >> 24) & 0xf;
                return (
                    format!("amd_type{core_type}_model{native}_power{rank}"),
                    None,
                    true,
                );
            }
            if value.ebx & 0xffff == 0 {
                break;
            }
        }
    }
    ("uniform".into(), Some(0), true)
}

#[cfg(any(
    target_arch = "x86_64",
    all(target_arch = "x86", target_feature = "sse")
))]
fn x86_class() -> (String, Option<u32>, bool) {
    let (mut name, rank, known) = x86_base_class();
    if let Some(caches) = raw_cpuid::CpuId::new().get_cache_parameters() {
        let mut sizes: Vec<_> = caches
            .filter(|c| c.level() == 3)
            .map(|c| {
                c.associativity() as u64
                    * c.physical_line_partitions() as u64
                    * c.coherency_line_size() as u64
                    * c.sets() as u64
            })
            .collect();
        sizes.sort_unstable();
        sizes.dedup();
        for size in sizes {
            name.push_str(&format!("_l3bytes{size}"));
        }
    }
    (name, rank, known)
}

/// Discover CPU classes without inferring them from logical CPU numbering or
/// maximum frequency. Missing or incomplete information stays unknown.
pub fn detect_cpu_topology() -> CpuTopology {
    #[cfg(target_os = "linux")]
    {
        return linux_topology(
            Path::new("/sys/devices/system/cpu"),
            Path::new("/sys/devices"),
        );
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(levels) = sysctl("hw.nperflevels").and_then(|s| s.parse::<usize>().ok()) {
            if levels > 0 && levels < 32 {
                let mut classes = vec![];
                for level in 0..levels {
                    let Some(count) = sysctl(&format!("hw.perflevel{level}.logicalcpu"))
                        .and_then(|s| s.parse().ok())
                        .filter(|&count| count > 0)
                    else {
                        return CpuTopology::unknown("missing macOS performance-level count");
                    };
                    let Some(name) = sysctl(&format!("hw.perflevel{level}.name")) else {
                        return CpuTopology::unknown("missing macOS performance-level name");
                    };
                    classes.push(CoreClass {
                        name,
                        cpus: vec![],
                        logical_cpu_count: count,
                        performance_rank: Some((levels - level) as u32),
                    });
                }
                return CpuTopology {
                    kind: if classes.len() > 1 {
                        TopologyKind::Asymmetric
                    } else {
                        TopologyKind::Homogeneous
                    },
                    classes,
                    source: "sysctl_perflevels".into(),
                    diagnostic: None,
                };
            }
        }
        #[cfg(target_arch = "x86_64")]
        {
            let (name, _, _) = x86_class();
            return CpuTopology::unknown(format!(
                "macOS performance levels unavailable; one calling CPU ({name}) cannot establish whole-machine topology"
            ));
        }
    }
    #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
    {
        return match super::windows_topology::discover() {
            Ok(entries) => group_classes(entries, "windows_native_topology", false),
            Err(error) => CpuTopology::unknown(error),
        };
    }
    #[cfg(all(
        target_os = "windows",
        any(
            target_arch = "x86_64",
            all(target_arch = "x86", target_feature = "sse")
        )
    ))]
    {
        let mut system = sysinfo::System::new();
        system.refresh_cpu_list(sysinfo::CpuRefreshKind::nothing());
        if system.cpus().is_empty() || system.cpus().len() > usize::BITS as usize {
            return CpuTopology::unknown("Windows multi-group CPU placement is unsupported");
        }
        let Ok(cpus) = affinity::get_process_affinity() else {
            return CpuTopology::unknown("Windows affinity query failed");
        };
        let mut entries = vec![];
        for cpu in cpus {
            let Ok(_guard) = super::pin_current_thread(&[cpu]) else {
                return CpuTopology::unknown(
                    "Windows CPU class query could not visit every available CPU",
                );
            };
            let (name, rank, known) = x86_class();
            if !known {
                return CpuTopology::unknown("CPUID reported an unknown core type");
            }
            entries.push((cpu, name, rank));
        }
        let mut topology = group_classes(entries, "windows_process_cpuid", false);
        if raw_cpuid::native_cpuid::cpuid_count(7, 0).edx & (1 << 15) != 0 {
            topology.kind = TopologyKind::Asymmetric;
        }
        return topology;
    }
    #[allow(unreachable_code)]
    CpuTopology::unknown("CPU class discovery is unsupported on this target")
}

#[cfg(target_os = "macos")]
fn sysctl(name: &str) -> Option<String> {
    let output = std::process::Command::new("sysctl")
        .args(["-n", name])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(target_os = "linux")]
fn linux_topology(root: &Path, devices: &Path) -> CpuTopology {
    let Ok(online) = read_cpus(&root.join("online")) else {
        return CpuTopology::unknown("cannot read online Linux CPU IDs");
    };
    let mut pmu = BTreeMap::new();
    for (device, name, rank) in [
        ("cpu_core", "performance", 2),
        ("cpu_atom", "efficiency", 1),
        ("cpu_lowpower", "lowpower", 0),
    ] {
        if let Ok(cpus) = read_cpus(&devices.join(device).join("cpus")) {
            for cpu in cpus {
                pmu.insert(cpu, (name.to_string(), Some(rank)));
            }
        }
    }
    let mut entries = vec![];
    let mut incomplete = false;
    for cpu in online {
        let directory = root.join(format!("cpu{cpu}"));
        let capacity = std::fs::read_to_string(directory.join("cpu_capacity"))
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok());
        let base = if let Some(value) = pmu.get(&cpu) {
            Some(value.clone())
        } else if let Some(value) = capacity {
            Some((format!("capacity{value}"), Some(value)))
        } else {
            #[cfg(any(
                target_arch = "x86_64",
                all(target_arch = "x86", target_feature = "sse")
            ))]
            {
                match super::pin_current_thread(&[cpu]) {
                    Ok(_guard) => {
                        let (name, rank, known) = x86_class();
                        known.then_some((name, rank))
                    }
                    Err(_) => None,
                }
            }
            #[cfg(not(any(
                target_arch = "x86_64",
                all(target_arch = "x86", target_feature = "sse")
            )))]
            {
                None
            }
        };
        let Some((mut name, rank)) = base else {
            incomplete = true;
            continue;
        };
        // Unequal last-level cache capacities are an asymmetry too (for example
        // different CCD cache capacities). Cache size does not rank performance.
        let mut cache = vec![];
        if let Ok(indices) = std::fs::read_dir(directory.join("cache")) {
            for index in indices.flatten() {
                let path = index.path();
                let level = std::fs::read_to_string(path.join("level")).ok();
                if level.as_deref().is_some_and(|s| s.trim() == "3") {
                    if let Ok(size) = std::fs::read_to_string(path.join("size")) {
                        cache.push(size.trim().to_string());
                    }
                }
            }
        }
        cache.sort();
        cache.dedup();
        if !cache.is_empty() {
            name.push_str(&format!("_l3{}", cache.join("+")));
        }
        entries.push((cpu, name, rank));
    }
    group_classes(entries, "linux_pmu_capacity_cpuid_cache", incomplete)
}

fn cli_value(flag: &str) -> Result<Option<String>, String> {
    let mut args = std::env::args().skip(1);
    let mut value = None;
    while let Some(arg) = args.next() {
        if arg == "--" {
            break;
        }
        let candidate = if arg == flag {
            Some(args.next().ok_or_else(|| format!("{flag} needs a value"))?)
        } else {
            arg.strip_prefix(&format!("{flag}=")).map(String::from)
        };
        if let Some(candidate) = candidate {
            if value.replace(candidate).is_some() {
                return Err(format!("{flag} was specified twice"));
            }
        }
    }
    Ok(value)
}

fn environment(name: &str) -> Result<Option<String>, String> {
    match std::env::var(name) {
        Ok(v) => Ok(Some(v)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(_) => Err(format!("{name} is not Unicode")),
    }
}

fn configuration() -> Result<(Option<CpuPolicy>, Option<Vec<usize>>), String> {
    let policy = cli_value("--cpu-policy")?
        .or(environment("ZENBENCH_CPU_POLICY")?)
        .map(|v| CpuPolicy::parse(&v))
        .transpose()?;
    let cpus = cli_value("--cpu-set")?
        .or(environment("ZENBENCH_CPU_SET")?)
        .map(|v| super::affinity::parse_cpu_list(&v).map_err(|e| e.to_string()))
        .transpose()?;
    Ok((policy, cpus))
}

#[cfg(target_os = "linux")]
fn cpuset_boundary() -> Result<Vec<usize>, String> {
    let membership = std::fs::read_to_string("/proc/self/cgroup").map_err(|e| e.to_string())?;
    let group = membership
        .lines()
        .find_map(|line| line.strip_prefix("0::"))
        .ok_or("cgroup v2 membership is unavailable")?;
    if group.split('/').any(|part| part == "..") {
        return Err("cgroup membership contains parent traversal".into());
    }
    let mounts = std::fs::read_to_string("/proc/self/mountinfo").map_err(|e| e.to_string())?;
    for line in mounts.lines() {
        let Some((left, right)) = line.split_once(" - ") else {
            continue;
        };
        if right.split_whitespace().next() != Some("cgroup2") {
            continue;
        }
        let fields: Vec<_> = left.split_whitespace().collect();
        if fields.len() < 5 {
            continue;
        }
        let root = decode_mount(fields[3]);
        let mount = PathBuf::from(decode_mount(fields[4]));
        let Ok(relative) = Path::new(group).strip_prefix(&root) else {
            continue;
        };
        let mut path = mount.join(relative);
        loop {
            let effective = path.join("cpuset.cpus.effective");
            if effective.exists() {
                return read_cpus(&effective);
            }
            if path == mount || !path.pop() {
                break;
            }
        }
    }
    Err("an enabled cpuset controller is unavailable".into())
}

#[cfg(target_os = "linux")]
fn decode_mount(value: &str) -> String {
    value
        .replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\012", "\n")
        .replace("\\134", "\\")
}

fn boundary() -> Result<(Vec<usize>, &'static str), String> {
    #[cfg(target_os = "linux")]
    {
        return cpuset_boundary().map(|cpus| (cpus, "cpuset"));
    }
    #[cfg(target_os = "windows")]
    {
        return affinity::get_process_affinity()
            .map(|cpus| {
                (
                    cpus,
                    if super::windows_job::installed() {
                        "job_affinity"
                    } else {
                        "process_mask"
                    },
                )
            })
            .map_err(|e| e.to_string());
    }
    #[allow(unreachable_code)]
    Err(
        "enforced process placement is unsupported on this target; Mach tags and QoS are hints"
            .into(),
    )
}

fn resolve(
    topology: CpuTopology,
    policy: CpuPolicy,
    requested: Option<&[usize]>,
    enforced: Result<(Vec<usize>, &str), String>,
) -> Result<CpuPlacement, String> {
    let boundary_error = enforced.as_ref().err().cloned();
    let (effective, enforcement) = enforced.unwrap_or((vec![], "none"));
    if policy == CpuPolicy::Unrestricted {
        if requested.is_some() {
            return Err("unrestricted policy cannot certify a requested --cpu-set; remove the set or use a checked policy".into());
        }
        return Ok(CpuPlacement {
            policy,
            core_class: None,
            topology,
            status: PlacementStatus::Unrestricted,
            effective_cpus: effective,
            requested_cpus: requested.map(Vec::from),
            thread_affinity_requested: None,
            thread_affinity_effective: None,
            enforcement: enforcement.into(),
        });
    }
    if topology.kind == TopologyKind::Homogeneous
        && policy == CpuPolicy::Auto
        && requested.is_none()
        && topology.source != "windows_process_cpuid"
    {
        return Ok(CpuPlacement {
            policy,
            core_class: topology.classes.first().map(|c| c.name.clone()),
            topology,
            status: PlacementStatus::UniformHardware,
            effective_cpus: effective,
            requested_cpus: requested.map(Vec::from),
            thread_affinity_requested: None,
            thread_affinity_effective: None,
            enforcement: enforcement.into(),
        });
    }
    let class = topology.class_for(&effective).ok_or_else(|| format!(
        "CPU placement refused: {:?} topology; the process is not confined to one known class. Classes: {}. Launch with `zenbench exec --cpu-policy performance PROGRAM -- ARGS`, or supply an enabled cpuset/process mask and --cpu-set=IDs. A group affinity mask and ZENBENCH_PIN are insufficient. Inspect `zenbench cpu-info` and `zenbench exec --help`. Exploratory opt-out: --cpu-policy=unrestricted (UNVERIFIED_UNRESTRICTED; cannot confirm or promote baselines).",
        topology.kind, format_args!("{}; process boundary {effective:?}; {}", topology.classes.iter().map(|c| format!("{}={:?}", c.name, c.cpus)).collect::<Vec<_>>().join("; "), boundary_error.unwrap_or_default())))?;
    if let Some(cpus) = requested {
        if effective != cpus {
            return Err(format!(
                "CPU placement refused: requested {cpus:?}, OS process boundary is {effective:?}; the exact set was not enforced"
            ));
        }
    }
    if matches!(policy, CpuPolicy::Performance | CpuPolicy::Efficiency) {
        let selected = topology.select(policy)?;
        if !effective.iter().all(|cpu| selected.contains(cpu)) {
            return Err(format!(
                "CPU placement refused: {} class requested, process boundary {effective:?} is in {}",
                policy.name(),
                class.name
            ));
        }
    }
    let core_class = Some(class.name.clone());
    Ok(CpuPlacement {
        policy,
        topology,
        status: PlacementStatus::EnforcedUniform,
        core_class,
        effective_cpus: effective,
        requested_cpus: requested.map(Vec::from),
        thread_affinity_requested: None,
        thread_affinity_effective: None,
        enforcement: enforcement.into(),
    })
}

/// Check the default/CLI/environment process policy. Errors occur before any
/// timed work, and a calling-thread mask never substitutes for a process bound.
pub fn check_cpu_placement() -> Result<CpuPlacement, String> {
    CpuSession::start(None, None).map(|session| session.placement)
}

pub(crate) struct CpuSession {
    pub(crate) placement: CpuPlacement,
}

impl CpuSession {
    pub(crate) fn start(policy: Option<CpuPolicy>, cpus: Option<&[usize]>) -> Result<Self, String> {
        let (configured, configured_cpus) = configuration()?;
        let policy = configured.or(policy).unwrap_or_default();
        let cpus = configured_cpus.as_deref().or(cpus);
        #[cfg(target_os = "windows")]
        static PROCESS_CPUS: std::sync::Mutex<Option<Vec<usize>>> = std::sync::Mutex::new(None);
        #[cfg(target_os = "windows")]
        let mut process_cpus = PROCESS_CPUS
            .lock()
            .map_err(|_| "CPU policy mutex poisoned")?;
        let topology = detect_cpu_topology();
        #[cfg(target_os = "windows")]
        if policy != CpuPolicy::Unrestricted {
            let selected = if let Some(cpus) = cpus {
                Some(cpus.to_vec())
            } else if matches!(policy, CpuPolicy::Performance | CpuPolicy::Efficiency) {
                Some(topology.select(policy)?)
            } else {
                None
            };
            if let Some(selected) = selected {
                if selected.is_empty() || selected.iter().any(|&cpu| cpu >= usize::BITS as usize) {
                    return Err("invalid process CPU mask".into());
                }
                if topology.class_for(&selected).is_none() {
                    return Err("requested CPUs are not in one known class".into());
                }
                if process_cpus
                    .as_ref()
                    .is_some_and(|previous| previous != &selected)
                {
                    return Err("process CPU placement is fixed for its lifetime; launch a fresh process to change classes".into());
                }
                if !super::windows_job::installed() {
                    affinity::set_process_affinity(&selected).map_err(|e| e.to_string())?;
                }
            }
        }
        #[allow(unused_mut)]
        let mut placement = resolve(topology, policy, cpus, boundary())?;
        #[cfg(target_os = "windows")]
        if placement.status == PlacementStatus::EnforcedUniform {
            if process_cpus
                .as_ref()
                .is_some_and(|previous| previous != &placement.effective_cpus)
            {
                return Err("process CPU boundary changed since first admission".into());
            }
            super::windows_job::install(&placement.effective_cpus)?;
            let (actual, enforcement) = boundary()?;
            if actual != placement.effective_cpus {
                return Err(
                    "CPU affinity job did not preserve the requested process boundary".into(),
                );
            }
            placement.enforcement = enforcement.into();
            *process_cpus = Some(placement.effective_cpus.clone());
        }
        Ok(Self { placement })
    }

    pub(crate) fn verify(&self) -> Result<(), String> {
        if self.placement.status == PlacementStatus::EnforcedUniform {
            let (cpus, mechanism) = boundary()?;
            if cpus != self.placement.effective_cpus || mechanism != self.placement.enforcement {
                return Err(
                    "CPU process boundary changed during the run; measurements are invalid".into(),
                );
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn hybrid() -> CpuTopology {
        group_classes(
            vec![(0, "P".into(), Some(2)), (1, "E".into(), Some(1))],
            "fixture",
            false,
        )
    }
    #[test]
    fn mixed_process_rejected_even_if_caller_was_pinned() {
        assert!(
            resolve(hybrid(), CpuPolicy::Auto, None, Ok((vec![0, 1], "cpuset")))
                .unwrap_err()
                .contains("not confined")
        );
        assert!(resolve(hybrid(), CpuPolicy::Auto, None, Err("no controller".into())).is_err());
    }
    #[test]
    fn only_verified_single_class_passes() {
        let placement = resolve(hybrid(), CpuPolicy::Auto, None, Ok((vec![0], "cpuset"))).unwrap();
        assert_eq!(placement.status, PlacementStatus::EnforcedUniform);
        assert_eq!(placement.core_class.as_deref(), Some("P"));
        assert!(
            resolve(
                hybrid(),
                CpuPolicy::Performance,
                None,
                Ok((vec![1], "cpuset"))
            )
            .is_err()
        );
        assert!(
            resolve(
                hybrid(),
                CpuPolicy::Auto,
                Some(&[0, 1]),
                Ok((vec![0], "cpuset"))
            )
            .is_err()
        );
    }
    #[test]
    fn unknown_is_not_uniform_and_opt_out_is_labeled() {
        let topology = CpuTopology::unknown("unavailable");
        assert!(resolve(topology.clone(), CpuPolicy::Auto, None, Err("none".into())).is_err());
        let placement =
            resolve(topology, CpuPolicy::Unrestricted, None, Err("none".into())).unwrap();
        assert_eq!(placement.status, PlacementStatus::Unrestricted);
        assert!(placement.llm().contains("cpu_class_uniform=false"));
        assert!(placement.llm().contains("UNVERIFIED_UNRESTRICTED"));
    }
    #[test]
    fn homogeneous_hardware_needs_no_process_restriction() {
        let topology = group_classes(vec![(0, "uniform".into(), Some(0))], "fixture", false);
        assert_eq!(
            resolve(topology, CpuPolicy::Auto, None, Err("none".into()))
                .unwrap()
                .status,
            PlacementStatus::UniformHardware
        );
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn cache_asymmetry_requires_explicit_class_and_cpu_ids_are_not_inferred() {
        let directory = crate::paths::target_zenbench_dir()
            .join(format!("cpu-topology-test-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("online"), "0-1").unwrap();
        for (cpu, size) in [(0, "96M"), (1, "32M")] {
            let root = directory.join(format!("cpu{cpu}"));
            let cache = root.join("cache/index3");
            std::fs::create_dir_all(&cache).unwrap();
            std::fs::write(root.join("cpu_capacity"), "1024").unwrap();
            std::fs::write(cache.join("level"), "3").unwrap();
            std::fs::write(cache.join("size"), size).unwrap();
        }
        let topology = linux_topology(&directory, &directory);
        assert_eq!(topology.kind, TopologyKind::Asymmetric);
        assert!(topology.select(CpuPolicy::Performance).is_err());
        assert!(
            resolve(
                topology.clone(),
                CpuPolicy::Auto,
                None,
                Ok((vec![0, 1], "cpuset"))
            )
            .is_err()
        );
        assert!(
            resolve(
                topology,
                CpuPolicy::Auto,
                Some(&[0]),
                Ok((vec![0], "cpuset"))
            )
            .is_ok()
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn exact_set_conflicts_and_unranked_classes_are_rejected() {
        assert!(
            resolve(
                hybrid(),
                CpuPolicy::Unrestricted,
                Some(&[0]),
                Ok((vec![0], "cpuset"))
            )
            .is_err()
        );
        let topology = group_classes(vec![(7, "unknown-rank".into(), None)], "fixture", false);
        assert!(topology.select(CpuPolicy::Performance).is_err());
        let partial = group_classes(vec![(7, "known".into(), Some(2))], "fixture", true);
        assert_eq!(partial.kind, TopologyKind::Unknown);
        assert!(
            resolve(
                partial.clone(),
                CpuPolicy::Auto,
                None,
                Ok((vec![7], "cpuset"))
            )
            .is_ok()
        );
        assert!(resolve(partial, CpuPolicy::Auto, None, Ok((vec![7, 8], "cpuset"))).is_err());
    }
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    #[test]
    fn coordinator_changes_invalidate_recorded_thread_placement() {
        let mut placement =
            resolve(hybrid(), CpuPolicy::Auto, None, Ok((vec![0], "cpuset"))).unwrap();
        let current = super::super::current_thread_affinity().unwrap();
        placement.thread_affinity_effective = Some(current.clone());
        assert!(placement.verify_thread().is_ok());
        let mut changed = current;
        changed.push(usize::MAX);
        placement.thread_affinity_effective = Some(changed);
        assert!(
            placement
                .verify_thread()
                .unwrap_err()
                .contains("coordinator affinity changed")
        );
    }

    fn uniform_topology(cpus: &[usize]) -> CpuTopology {
        group_classes(
            cpus.iter()
                .map(|&cpu| (cpu, "uniform".into(), Some(0)))
                .collect(),
            "fixture",
            false,
        )
    }

    fn four_uniform() -> CpuTopology {
        uniform_topology(&[0, 1, 2, 3])
    }

    fn enforced_uniform(cpus: &[usize]) -> CpuPlacement {
        resolve(
            uniform_topology(cpus),
            CpuPolicy::Auto,
            Some(cpus),
            Ok((cpus.to_vec(), "cpuset")),
        )
        .unwrap()
    }

    #[test]
    fn observer_cpus_are_the_verified_boundary_minus_the_measurement_pin() {
        let placement = enforced_uniform(&[0, 1, 2, 3]);
        assert_eq!(
            placement.status,
            PlacementStatus::EnforcedUniform,
            "fixture must be a verified single-class boundary"
        );
        // Exact difference: the sampler gets the spare CPUs, never the pin.
        assert_eq!(placement.observer_cpus(Some(&[0])), Some(vec![1, 2, 3]));
        assert_eq!(placement.observer_cpus(Some(&[1, 3])), Some(vec![0, 2]));
        // Pin order and duplicates do not change the spare set.
        assert_eq!(placement.observer_cpus(Some(&[3, 1, 1])), Some(vec![0, 2]));
        // A pin naming CPUs outside the boundary subtracts nothing; the
        // earlier admission panic is what rejects such pins at runtime.
        assert_eq!(placement.observer_cpus(Some(&[0, 9])), Some(vec![1, 2, 3]));
        // No explicit measurement pin keeps ordinary inherited affinity.
        assert_eq!(placement.observer_cpus(None), None);
    }

    #[test]
    fn observer_cpus_also_apply_on_verified_uniform_hardware() {
        // Homogeneous hardware needs no OS boundary but is still verified
        // single-class: the sampler may use the unmeasured CPUs.
        let placement = resolve(
            four_uniform(),
            CpuPolicy::Auto,
            None,
            Ok((vec![0, 1, 2, 3], "cpuset")),
        )
        .unwrap();
        assert_eq!(placement.status, PlacementStatus::UniformHardware);
        assert_eq!(placement.observer_cpus(Some(&[0])), Some(vec![1, 2, 3]));
        assert_eq!(placement.observer_cpus(None), None);
    }

    #[test]
    fn observer_cpus_refuse_when_no_spare_or_boundary_is_unverified() {
        // Pinning the whole boundary leaves the sampler nowhere safe to go.
        assert_eq!(
            enforced_uniform(&[0, 1, 2, 3]).observer_cpus(Some(&[0, 1, 2, 3])),
            None
        );
        // Single-CPU boundary: the only spare candidate is the measured CPU.
        assert_eq!(enforced_uniform(&[2]).observer_cpus(Some(&[2])), None);
        // Unverified (opt-out) placement never selects an observer mask.
        let unrestricted = resolve(
            CpuTopology::unknown("unavailable"),
            CpuPolicy::Unrestricted,
            None,
            Err("none".into()),
        )
        .unwrap();
        assert_eq!(unrestricted.status, PlacementStatus::Unrestricted);
        assert_eq!(unrestricted.observer_cpus(Some(&[0])), None);
    }

    #[test]
    fn observer_cpus_proof_against_the_current_process_mask() {
        // Real permitted CPUs on this host; every host executes real
        // assertions, including the single-CPU fallback.
        let permitted = crate::platform::current_thread_affinity()
            .unwrap_or_else(|_| (0..std::thread::available_parallelism().unwrap().get()).collect());
        assert!(!permitted.is_empty());
        let placement = resolve(
            uniform_topology(&permitted),
            CpuPolicy::Auto,
            Some(&permitted),
            Ok((permitted.clone(), "cpuset")),
        )
        .unwrap();
        let pin = [permitted[0]];
        match placement.observer_cpus(Some(&pin)) {
            Some(spare) => {
                let mut expected = permitted.clone();
                expected.retain(|cpu| *cpu != pin[0]);
                assert_eq!(spare, expected);
                assert!(
                    !expected.is_empty(),
                    "spare set exists only when it is non-empty"
                );
            }
            None => assert_eq!(
                permitted.len(),
                1,
                "no spare is legal only when the boundary is a single CPU"
            ),
        }
    }
}
