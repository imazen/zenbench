//! Descriptive fleet comparisons and small, standalone version-control records.
use super::{fleet, fleet_run};
use serde_json::{Value, json};
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub(crate) fn export(output: &Path, directories: &[PathBuf]) -> io::Result<()> {
    let mut records = Vec::new();
    for directory in directories {
        let v = fleet_run::read(directory)?;
        records.push(json!({"kind":"zenbench_fleet_summary","schema":1,
            "packet_sha256":fleet::hash_file(&directory.join("packet.json"))?,
            "experiment":v["experiment"],"machine":v["machine"],"domain":v["domain"],
            "replicate":v["replicate"],"attempt":v["attempt"],"status":v["status"],
            "bundle_sha256":v["bundle_sha256"],"runner_sha256":v["runner"]["runner"]["sha256"],
            "plan":v["plan"],"memory":v["memory"],"elapsed_seconds":v["elapsed_seconds"],
            "failure":v["failure"],"inference":v["inference"],
            "commands":{"decode":"each line is standalone JSON; retrieve the packet by packet_sha256 for full evidence",
            "verify":"zenbench fleet-read ATTEMPT_DIRECTORY",
            "compare":"zenbench fleet-diff LEFT_ATTEMPT RIGHT_ATTEMPT"}}));
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    for record in records {
        writeln!(file, "{}", serde_json::to_string(&record)?)?;
    }
    Ok(())
}

pub(crate) fn diff(left: &Path, right: &Path) -> io::Result<Value> {
    compare(&fleet_run::read(left)?, &fleet_run::read(right)?)
}
fn compare(a: &Value, b: &Value) -> io::Result<Value> {
    for v in [a, b] {
        if v["status"] != "complete" {
            return Err(io::Error::other(
                "comparison requires complete observations; inspect failed attempts with fleet-read",
            ));
        }
    }
    // Binary hashes may differ intentionally, but workload, measurement and
    // testbed contracts must match. No acceptance test is inferred from two runs.
    for pointer in [
        "/machine",
        "/domain",
        "/plan",
        "/runner/runner/sha256",
        "/runner/runner/os",
        "/runner/runner/process_arch",
        "/placement",
    ] {
        if a.pointer(pointer).is_none_or(Value::is_null) || a.pointer(pointer) != b.pointer(pointer)
        {
            return Err(io::Error::other(format!(
                "comparison contract differs or is missing: {pointer}"
            )));
        }
    }
    let memories = |v: &Value| -> io::Result<std::collections::BTreeMap<String, Value>> {
        let mut rows = std::collections::BTreeMap::new();
        for row in v["memory"]
            .as_array()
            .ok_or_else(|| io::Error::other("missing memory records"))?
        {
            let key = serde_json::to_string(&json!([
                row["quantity"],
                row["scope"],
                row["backend"],
                row["backend_version"],
                row["coverage"]
            ]))?;
            if rows.insert(key, row.clone()).is_some() {
                return Err(io::Error::other(
                    "ambiguous duplicate memory quantity/scope/backend",
                ));
            }
        }
        Ok(rows)
    };
    let aa = memories(a)?;
    let bb = memories(b)?;
    if aa.keys().ne(bb.keys()) {
        return Err(io::Error::other(
            "memory quantity/scope/backend contracts differ",
        ));
    }
    let mut deltas = Vec::new();
    for (key, row) in aa {
        let other = &bb[&key];
        let delta = match (row["bytes"].as_u64(), other["bytes"].as_u64()) {
            (Some(a), Some(b)) => Some((i128::from(b) - i128::from(a)).to_string()),
            _ => None,
        };
        deltas.push(json!({"left":row,"right":other,"right_minus_left_bytes":delta}));
    }
    Ok(json!({"kind":"zenbench_fleet_diff","schema":1,
        "inference":"descriptive only; individual observations do not establish a performance change",
        "left":{"attempt":a["attempt"],"bundle_sha256":a["bundle_sha256"],"replicate":a["replicate"]},
        "right":{"attempt":b["attempt"],"bundle_sha256":b["bundle_sha256"],"replicate":b["replicate"]},
        "memory":deltas,"elapsed_seconds":{"left":a["elapsed_seconds"],"right":b["elapsed_seconds"],
        "meaning":"supervisor duration including startup and cleanup; not native operation latency"}}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn comparisons_preserve_integer_bytes_and_refuse_changed_contracts() {
        let a = json!({"status":"complete","machine":"cpu","domain":"p","plan":{},
            "runner":{"runner":{"sha256":"runner","os":"linux","process_arch":"aarch64"}},
            "placement":{},"memory":[{"quantity":"requested_live_heap_peak","scope":"score",
                "backend":"control","backend_version":"1","coverage":"Rust",
                "bytes":9007199254740992u64}]});
        let mut b = a.clone();
        b["memory"][0]["bytes"] = json!(9007199254740993u64);
        assert_eq!(
            compare(&a, &b).unwrap()["memory"][0]["right_minus_left_bytes"],
            "1"
        );
        for (key, value) in [
            ("machine", json!("other")),
            ("domain", json!("e")),
            ("status", json!("busy")),
            ("plan", json!({"changed":true})),
        ] {
            let mut other = b.clone();
            other[key] = value;
            assert!(compare(&a, &other).is_err());
        }
        b["memory"][0]["bytes"] = Value::Null;
        b["memory"][0]["unavailable_reason"] = json!("unsupported");
        assert!(compare(&a, &b).unwrap()["memory"][0]["right_minus_left_bytes"].is_null());
    }
}
