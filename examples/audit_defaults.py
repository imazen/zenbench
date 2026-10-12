"""Measure local harness cost and retained bytes. No fleet orchestration."""
import argparse
import gzip
import hashlib
import json
import os
from pathlib import Path
import statistics
import subprocess
import time


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("binary", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--plan", choices=["smoke", "full", "ablation", "monitor"], default="full")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    if "TMPDIR" in os.environ:
        Path(os.environ["TMPDIR"]).mkdir(parents=True, exist_ok=True)
    base = os.environ.copy()
    for key in list(base):
        if key.startswith(("AUDIT_", "ZENBENCH_")):
            del base[key]
    base["ZENBENCH_CPU_POLICY"] = "auto"
    plans = [
        ("empty", 3, {"AUDIT_GROUPS": "0"}, []),
        ("default", 3, {}, []),
        ("gate_disabled", 3, {"ZENBENCH_NO_BUSY_GATE": "1"}, []),
        ("warmup_100ms", 3, {"AUDIT_WARMUP_MS": "100"}, []),
        ("three_processes", 2, {}, ["--mean-of-processes=3"]),
        ("five_processes", 1, {}, ["--mean-of-processes=5"]),
        ("twelve_invocations", 12, {}, []),
        ("twelve_three_process_invocations", 12, {}, ["--mean-of-processes=3"]),
        ("six_arms", 2, {"AUDIT_ARMS": "6"}, []),
        ("small_input", 2, {"AUDIT_WORDS": "16"}, []),
        ("large_input", 2, {"AUDIT_WORDS": "65536"}, []),
        ("fixed_200_rounds", 1, {"AUDIT_ROUNDS": "200"}, []),
    ]
    if args.plan == "smoke":
        plans = [("empty", 1, {"AUDIT_GROUPS": "0"}, [])]
    elif args.plan == "ablation":
        # Alternate equal-round controls; do not conflate convergence with
        # the cost of the resource sampling interval.
        plans = [
            ("fixed30_gate_on_a", 1, {"AUDIT_ROUNDS": "30"}, []),
            ("fixed30_gate_off_a", 1, {"AUDIT_ROUNDS": "30", "ZENBENCH_NO_BUSY_GATE": "1"}, []),
            ("fixed30_gate_on_b", 1, {"AUDIT_ROUNDS": "30"}, []),
            ("fixed30_gate_off_b", 1, {"AUDIT_ROUNDS": "30", "ZENBENCH_NO_BUSY_GATE": "1"}, []),
            ("fixed200_gate_off", 1, {"AUDIT_ROUNDS": "200", "ZENBENCH_NO_BUSY_GATE": "1"}, []),
        ]
    elif args.plan == "monitor":
        plans = [
            ("fixed200_gate_on", 3, {"AUDIT_ROUNDS": "200"}, []),
            ("fixed200_gate_off", 3, {"AUDIT_ROUNDS": "200", "ZENBENCH_NO_BUSY_GATE": "1"}, []),
        ]
    records = []
    for name, repeats, overrides, flags in plans:
        for repeat in range(repeats):
            env = base | overrides
            command = [str(args.binary), "--format=json", *flags]
            stem = args.output / f"{name}-{repeat}"
            env["CARGO_TARGET_DIR"] = str(stem.resolve()) + ".target"
            env["ZENBENCH_DATA_DIR"] = str(stem.resolve()) + ".data"
            env["AUDIT_CHILD_ARCHIVE"] = str(stem.resolve()) + ".children"
            start = time.perf_counter()
            with stem.with_suffix(".json").open("wb") as stdout, stem.with_suffix(".log").open("wb") as stderr:
                proc = subprocess.run(command, env=env, stdout=stdout, stderr=stderr, timeout=300)
            elapsed = time.perf_counter() - start
            if proc.returncode:
                raise RuntimeError(f"{stem}: exit {proc.returncode}; retained logs")
            raw = stem.with_suffix(".json").read_bytes()
            packet = json.loads(raw)
            groups = packet["comparisons"]
            samples = [s for g in groups for s in g.get("samples", [])]
            children = list(Path(env["AUDIT_CHILD_ARCHIVE"]).glob("*.json"))
            child_raw = [p.read_bytes() for p in children]
            child_packets = [json.loads(raw) for raw in child_raw]
            reports = list(Path(env["CARGO_TARGET_DIR"]).rglob("*.txt"))
            record = {
                "case": name, "repeat": repeat, "wall_seconds": elapsed,
                "reported_total_time": packet["total_time"],
                "rounds": [g["completed_rounds"] for g in groups],
                "retained_rounds": len(samples),
                "timed_batch_ns": sum(sum(s["elapsed_ns"]) for s in samples),
                "gate_clean": {str(v): sum(s["gate_clean"] == v for s in samples) for v in [True, False, None]},
                "json_bytes": len(raw), "gzip_bytes": len(gzip.compress(raw, mtime=0)),
                "sha256": hashlib.sha256(raw).hexdigest(),
                "placement": packet["cpu_placement"]["status"],
                "unreliable": packet["unreliable"],
                "child_count": len(children),
                "child_json_bytes": sum(map(len, child_raw)),
                "child_gzip_bytes": sum(len(gzip.compress(raw, mtime=0)) for raw in child_raw),
                "child_rounds": [g["completed_rounds"] for p in child_packets for g in p["comparisons"]],
                "llm_report_bytes": sum(p.stat().st_size for p in reports),
                "overrides": overrides, "flags": flags,
            }
            records.append(record)
            (args.output / "records.json").write_text(json.dumps(records, indent=2) + "\n")
            print(json.dumps(record), flush=True)
    summary = []
    for name, *_ in plans:
        rows = [r for r in records if r["case"] == name]
        summary.append({
            "case": name, "runs": len(rows),
            "wall_total_seconds": sum(r["wall_seconds"] for r in rows),
            "wall_median_seconds": statistics.median(r["wall_seconds"] for r in rows),
            "wall_min_seconds": min(r["wall_seconds"] for r in rows),
            "wall_max_seconds": max(r["wall_seconds"] for r in rows),
            "json_bytes_total": sum(r["json_bytes"] for r in rows),
            "gzip_bytes_total": sum(r["gzip_bytes"] for r in rows),
        })
    (args.output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")


if __name__ == "__main__":
    main()
