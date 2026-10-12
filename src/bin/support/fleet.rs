//! Portable local execution contracts. Scheduling and storage belong to zenfleet.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

pub(crate) fn hash_file(path: &Path) -> io::Result<String> {
    let mut input = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = input.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

pub(crate) fn capabilities() -> io::Result<Value> {
    // sysinfo can fall back to the process architecture. Do not advertise that
    // as an independently verified native-machine architecture.
    Ok(json!({
        "kind":"zenbench_capabilities", "schema":1,
        "runner":{"version":env!("CARGO_PKG_VERSION"),
            "sha256":hash_file(&std::env::current_exe()?)?,
            "os":std::env::consts::OS, "process_arch":std::env::consts::ARCH,
            "reported_machine_arch":sysinfo::System::cpu_arch(),
            "native_arch_verified":false},
        "protocols":{"bundle":1,"pair":1,"fleet_attempt":1,"driver":1},
        "topology":zenbench::platform::detect_cpu_topology(),
        "placement":zenbench::platform::check_cpu_placement(),
        "coordination":{"directory":crate::coordination::directory()?,"scope":"participants sharing this local directory"},
        "memory":{
            "heaptrack":{"implemented":cfg!(target_os="linux"),"recorder":tool("heaptrack"),"analyzer":tool("heaptrack_print"),"coverage":"intercepted allocations; whole child only","probe":"not_run"},
            "os_peak":{"implemented":cfg!(any(target_os="linux",target_os="macos",windows)),"tool":if cfg!(windows) {Some(PathBuf::from("GetProcessMemoryInfo"))} else {Path::new("/usr/bin/time").is_file().then(||PathBuf::from("/usr/bin/time"))},"probe":"not_run"},
            "driver":{"implemented":true,"coverage":"declared by instrumented driver; not independently certified"}
        },
        "commands":{"compare":"zenbench pair --help","guide":"zenbench guide"}
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleSpec {
    schema: u32,
    target: String,
    /// Caller-owned logical plan. Paths name bundle files, never staging paths.
    plan: Value,
    files: BTreeMap<PathBuf, SourceFile>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceFile {
    source: PathBuf,
    executable: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Bundle {
    pub schema: u32,
    pub target: String,
    pub plan: Value,
    pub files: BTreeMap<PathBuf, Artifact>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Artifact {
    pub sha256: String,
    pub bytes: u64,
    pub executable: bool,
}

pub(crate) fn relative(path: &Path) -> io::Result<()> {
    // Reject Windows spellings on Unix too, so a bundle has identical meaning
    // when moved between hosts. Reject case collisions separately.
    let s = path
        .to_str()
        .ok_or_else(|| io::Error::other("bundle paths must be UTF-8"))?;
    if s.is_empty()
        || s.contains(['\\', ':'])
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(io::Error::other(
            "bundle paths must be relative normal components",
        ));
    }
    Ok(())
}

pub(crate) fn bundle(spec: &Path, output: &Path) -> io::Result<Value> {
    let spec_path = spec.canonicalize()?;
    let spec: BundleSpec = serde_json::from_slice(&fs::read(&spec_path)?)?;
    if spec.schema != 1 || spec.files.is_empty() || spec.target.trim().is_empty() {
        return Err(io::Error::other(
            "bundle requires schema 1, target and explicit files",
        ));
    }
    let mut names = std::collections::BTreeSet::new();
    for name in spec.files.keys() {
        relative(name)?;
        let key = name.to_string_lossy().to_ascii_lowercase();
        if key == "bundle.json" || !names.insert(key) {
            return Err(io::Error::other("reserved or case-colliding bundle path"));
        }
    }
    fs::create_dir(output)?;
    let mut files = BTreeMap::new();
    for (name, source) in spec.files {
        let src = spec_path.parent().unwrap().join(source.source);
        if !fs::metadata(&src)?.is_file() {
            return Err(io::Error::other("bundle source is not a regular file"));
        }
        let expected = hash_file(&src)?;
        let dest = output.join(&name);
        fs::create_dir_all(dest.parent().unwrap())?;
        fs::copy(&src, &dest)?;
        if hash_file(&dest)? != expected || hash_file(&src)? != expected {
            return Err(io::Error::other("bundle source changed while copying"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                &dest,
                fs::Permissions::from_mode(if source.executable { 0o755 } else { 0o644 }),
            )?;
        }
        files.insert(
            name,
            Artifact {
                sha256: expected,
                bytes: fs::metadata(dest)?.len(),
                executable: source.executable,
            },
        );
    }
    let manifest = Bundle {
        schema: 1,
        target: spec.target,
        plan: spec.plan,
        files,
    };
    fs::write(output.join("bundle.json"), serde_json::to_vec(&manifest)?)?;
    Ok(
        json!({"kind":"zenbench_bundle","schema":1,"manifest_sha256":hash_file(&output.join("bundle.json"))?,"files":manifest.files.len(),"target":manifest.target,"verify":"zenbench bundle-check BUNDLE_DIRECTORY"}),
    )
}

pub(crate) fn verify(root: &Path) -> io::Result<Bundle> {
    let root = root.canonicalize()?;
    let manifest: Bundle = serde_json::from_slice(&fs::read(root.join("bundle.json"))?)?;
    if manifest.schema != 1 || manifest.files.is_empty() {
        return Err(io::Error::other("unsupported or empty bundle"));
    }
    for (name, artifact) in &manifest.files {
        relative(name)?;
        let file = root.join(name).canonicalize()?;
        if !file.starts_with(&root)
            || !file.is_file()
            || fs::metadata(&file)?.len() != artifact.bytes
            || hash_file(&file)? != artifact.sha256
        {
            return Err(io::Error::other(format!(
                "bundle verification failed: {}",
                name.display()
            )));
        }
    }
    Ok(manifest)
}

pub(crate) fn tool(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let file = dir.join(name);
        if file.is_file() {
            return file.canonicalize().ok();
        }
        #[cfg(windows)]
        {
            let file = dir.join(format!("{name}.exe"));
            if file.is_file() {
                return file.canonicalize().ok();
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn portable_paths_reject_escape_and_platform_ambiguity() {
        for path in ["", "../data", "/data", "C:/data", "dir\\data"] {
            assert!(relative(Path::new(path)).is_err(), "{path}");
        }
        assert!(relative(Path::new("bin/metric.exe")).is_ok());
    }
}
