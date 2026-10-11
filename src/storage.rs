//! Durable compressed evidence is ignored by Git; snapshots are explicit exports.
use crate::{RunId, SuiteResult};
use flate2::{Compression, GzBuilder, read::GzDecoder};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

// A malformed or unexpectedly large packet must not allocate unbounded memory.
const MAX_PACKET_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
struct Reference {
    zenbench_evidence: u32,
    sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    data_root: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    usage: Option<serde_json::Value>,
}

// Argument arrays preserve spaces and shell metacharacters without suggesting
// that stored metadata should ever be evaluated as shell code.
pub(crate) fn usage(input: &Path) -> serde_json::Value {
    serde_json::json!({
        "syntax": "command argument arrays; replace <BASELINE> with a saved JSON/gzip/reference path",
        "read": ["zenbench", "read", input, "--format", "llm"],
        "decode": ["zenbench", "read", input, "--format", "json"],
        "compare": ["zenbench", "compare", "<BASELINE>", input],
        "compare_exit": {"0": "confirmed non-inferiority", "1": "confirmed regression", "2": "invalid or inconclusive"},
    })
}

pub(crate) fn data_dir() -> PathBuf {
    std::env::var_os("ZENBENCH_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".zenbench/data"))
}

pub(crate) fn prepare_dir() -> io::Result<PathBuf> {
    let root = data_dir();
    ignore_raw_dir(&root)?;
    Ok(root)
}

pub(crate) fn ignore_raw_dir(root: &Path) -> io::Result<()> {
    fs::create_dir_all(root)?;
    // Each data root protects itself, including a user-selected root inside Git.
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join(".gitignore"))
    {
        Ok(mut file) => {
            file.write_all(b"# Raw benchmark evidence; export a selected snapshot instead.\n*\n")?
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    Ok(())
}

pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let temporary = parent.join(format!(".zenbench-write-{}", RunId::generate()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn encoded(result: &SuiteResult) -> io::Result<Vec<u8>> {
    encode_limited(result, MAX_PACKET_BYTES)
}

fn encode_limited(result: &SuiteResult, limit: u64) -> io::Result<Vec<u8>> {
    // Never publish a packet that our own bounded reader cannot reopen.
    struct Limited<W> {
        writer: W,
        remaining: u64,
    }
    impl<W: Write> Write for Limited<W> {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if bytes.len() as u64 > self.remaining {
                return Err(io::Error::other(
                    "benchmark packet exceeds storage limit; split the suite",
                ));
            }
            let count = self.writer.write(bytes)?;
            self.remaining -= count as u64;
            Ok(count)
        }
        fn flush(&mut self) -> io::Result<()> {
            self.writer.flush()
        }
    }
    let mut limited = Limited {
        writer: GzBuilder::new()
            .comment("zenbench JSON evidence; read: zenbench read FILE --format llm; decode: zenbench read FILE --format json; compare: zenbench compare BASELINE FILE")
            .write(Vec::new(), Compression::default()),
        remaining: limit,
    };
    serde_json::to_writer(&mut limited, result).map_err(io::Error::other)?;
    let bytes = limited.writer.finish()?;
    if bytes.len() as u64 > limit {
        return Err(io::Error::other(
            "compressed benchmark packet exceeds storage limit",
        ));
    }
    Ok(bytes)
}

pub(crate) fn save(result: &SuiteResult, path: &Path) -> io::Result<()> {
    let bytes = if path.extension().is_some_and(|e| e == "gz") {
        encoded(result)?
    } else {
        serde_json::to_vec(result).map_err(io::Error::other)?
    };
    if bytes.len() as u64 > MAX_PACKET_BYTES {
        return Err(io::Error::other(
            "benchmark packet exceeds storage limit; split the suite",
        ));
    }
    atomic_write(path, &bytes)
}

fn limited_read(reader: impl Read) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(MAX_PACKET_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_PACKET_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "benchmark packet exceeds 256 MiB limit",
        ));
    }
    Ok(bytes)
}

pub(crate) fn load(path: &Path) -> io::Result<SuiteResult> {
    let bytes = limited_read(fs::File::open(path)?)?;
    if bytes.starts_with(&[0x1f, 0x8b]) {
        return decode(&bytes);
    }
    if let Ok(reference) = serde_json::from_slice::<Reference>(&bytes) {
        if reference.zenbench_evidence != 1
            || reference.sha256.len() != 64
            || !reference.sha256.bytes().all(|c| c.is_ascii_hexdigit())
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid evidence reference",
            ));
        }
        let root = reference
            .data_root
            .as_deref()
            .or_else(|| path.parent().and_then(Path::parent))
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "reference has no data root")
            })?;
        let object = root
            .join("objects")
            .join(format!("{}.json.gz", reference.sha256));
        let compressed = limited_read(fs::File::open(object)?)?;
        if digest(&compressed) != reference.sha256 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "evidence hash mismatch",
            ));
        }
        return decode(&compressed);
    }
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

fn decode(bytes: &[u8]) -> io::Result<SuiteResult> {
    let json = limited_read(GzDecoder::new(bytes))?;
    serde_json::from_slice(&json).map_err(io::Error::other)
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub(crate) fn evidence_digest(result: &SuiteResult) -> io::Result<String> {
    encoded(result).map(|bytes| digest(&bytes))
}

pub(crate) fn binary_digest() -> io::Result<String> {
    let mut file = fs::File::open(std::env::current_exe()?)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Publish content once and a small reference only after the object is durable.
pub(crate) fn persist(result: &SuiteResult) -> io::Result<PathBuf> {
    persist_at(result, &prepare_dir()?)
}

fn persist_at(result: &SuiteResult, root: &Path) -> io::Result<PathBuf> {
    let objects = root.join("objects");
    let runs = root.join("runs");
    fs::create_dir_all(&objects)?;
    fs::create_dir_all(&runs)?;
    let bytes = encoded(result)?;
    let sha256 = digest(&bytes);
    let object = objects.join(format!("{sha256}.json.gz"));
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&object)
    {
        Ok(mut file) => {
            file.write_all(&bytes)?;
            file.sync_all()?;
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            if digest(&limited_read(fs::File::open(&object)?)?) != sha256 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "existing evidence object is incomplete or corrupt",
                ));
            }
        }
        Err(e) => return Err(e),
    }
    // A fresh publication ID avoids reusing user-supplied or historical run IDs.
    let reference = runs.join(format!("{}.json", RunId::generate()));
    let bytes = serde_json::to_vec(&Reference {
        zenbench_evidence: 1,
        sha256,
        data_root: None,
        usage: Some(usage(&reference)),
    })
    .map_err(io::Error::other)?;
    atomic_write(&reference, &bytes)?;
    Ok(reference)
}

pub(crate) fn save_reference(result: &SuiteResult, path: &Path) -> io::Result<()> {
    let reference = persist(result)?;
    let mut record: Reference =
        serde_json::from_slice(&fs::read(reference)?).map_err(io::Error::other)?;
    record.data_root = Some(prepare_dir()?.canonicalize()?);
    record.usage = Some(usage(path));
    atomic_write(
        path,
        &serde_json::to_vec(&record).map_err(io::Error::other)?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_packets_are_refused_before_publication() {
        let result = SuiteResult::default();
        assert!(encode_limited(&result, 32).is_err());
        let encoded = encode_limited(&result, 4096).unwrap();
        assert_eq!(
            serde_json::to_value(decode(&encoded).unwrap()).unwrap(),
            serde_json::to_value(result).unwrap()
        );
    }

    #[test]
    fn historical_float_values_keep_every_bit_when_archived() {
        let values = [983.8325645756457_f64, 0.48636161947106676_f64];
        for value in values {
            let mut result = SuiteResult {
                loop_overhead_ns: value,
                ..Default::default()
            };
            let json = serde_json::to_vec(&result).unwrap();
            result = serde_json::from_slice(&json).unwrap();
            assert_eq!(result.loop_overhead_ns.to_bits(), value.to_bits());
            let restored = decode(&encoded(&result).unwrap()).unwrap();
            assert_eq!(restored.loop_overhead_ns.to_bits(), value.to_bits());
        }
    }

    #[test]
    fn compressed_evidence_round_trips_and_detects_corruption() {
        let root =
            crate::paths::target_zenbench_dir().join(format!("storage-test-{}", RunId::generate()));
        let result = SuiteResult::default();
        let reference = persist_at(&result, &root).unwrap();
        let restored = load(&reference).unwrap();
        assert_eq!(
            serde_json::to_value(restored).unwrap(),
            serde_json::to_value(&result).unwrap()
        );
        let second = persist_at(&result, &root).unwrap();
        assert_ne!(reference, second);
        assert_eq!(fs::read_dir(root.join("objects")).unwrap().count(), 1);
        let object = fs::read_dir(root.join("objects"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        fs::write(object, b"corrupt").unwrap();
        assert!(
            load(&reference)
                .unwrap_err()
                .to_string()
                .contains("hash mismatch")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn snapshot_size_depends_on_benchmarks_not_rounds() {
        let mut result = SuiteResult::default();
        let mut group = crate::ComparisonResult {
            group_name: "group".into(),
            ..Default::default()
        };
        let bench = crate::BenchmarkResult {
            name: "bench".into(),
            ..Default::default()
        };
        group.benchmarks.push(bench);
        result.comparisons.push(group);
        let snapshot = result.to_snapshot();
        result.comparisons[0].samples = vec![crate::RoundSample::default(); 10_000];
        assert_eq!(snapshot, result.to_snapshot());
        assert_eq!(snapshot.lines().count(), 2);
        assert!(snapshot.len() < 256);
    }
}
