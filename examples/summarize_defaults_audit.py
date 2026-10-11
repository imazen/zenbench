"""Verify retained local audit packets and produce a compact cost summary."""
import argparse
import hashlib
import json
from pathlib import Path
import statistics


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("directory", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    records = json.loads((args.directory / "records.json").read_text())
    # Only complete plans have this file; an interrupted plan is not complete.
    json.loads((args.directory / "summary.json").read_text())
    cases = {}
    for record in records:
        path = args.directory / f"{record['case']}-{record['repeat']}.json"
        raw = path.read_bytes()
        assert hashlib.sha256(raw).hexdigest() == record["sha256"]
        assert len(raw) == record["json_bytes"]
        packet = json.loads(raw)
        assert packet["cpu_placement"]["status"] == "enforced_uniform"
        for group in packet["comparisons"]:
            n = len(group["benchmarks"])
            for sample in group.get("samples", []):
                assert sample["iterations"] > 0
                assert sorted(sample["execution_order"]) == list(range(n))
                assert len(sample["elapsed_ns"]) == len(sample["compensated_ns"]) == n
        cases.setdefault(record["case"], []).append((record, packet))
    summary = []
    for name, pairs in cases.items():
        rows = [r for r, _ in pairs]
        by_position = {}
        for _, packet in pairs:
            for group in packet["comparisons"]:
                for sample in group.get("samples", []):
                    for position, arm in enumerate(sample["execution_order"]):
                        by_position.setdefault(position, []).append(
                            sample["elapsed_ns"][arm] / sample["iterations"]
                        )
        summary.append({
            "case": name,
            "invocations": len(rows),
            "child_processes": sum(r["child_count"] for r in rows),
            "wall_total_seconds": sum(r["wall_seconds"] for r in rows),
            "wall_median_seconds": statistics.median(r["wall_seconds"] for r in rows),
            "wall_min_seconds": min(r["wall_seconds"] for r in rows),
            "wall_max_seconds": max(r["wall_seconds"] for r in rows),
            "reported_total_millis": [r["reported_total_time"]["millis"] for r in rows],
            "completed_rounds": [r["rounds"] for r in rows],
            "child_rounds": [r["child_rounds"] for r in rows if r["child_count"]],
            "timed_batch_seconds": sum(r["timed_batch_ns"] for r in rows) / 1e9,
            "json_bytes": sum(r["json_bytes"] for r in rows),
            "json_gzip_bytes": sum(r["gzip_bytes"] for r in rows),
            "child_json_bytes": sum(r["child_json_bytes"] for r in rows),
            "child_json_gzip_bytes": sum(r["child_gzip_bytes"] for r in rows),
            "llm_report_bytes": sum(r["llm_report_bytes"] for r in rows),
            "gate_clean": {k: sum(r["gate_clean"][k] for r in rows) for k in ["True", "False", "None"]},
            "mean_ns_by_execution_position": {str(k): statistics.mean(v) for k, v in by_position.items()},
        })
    packet = {"kind": "measured_harness_cost_not_a_new_mode", "cases": summary}
    args.output.write_text(json.dumps(packet, indent=2) + "\n")
    print(args.output)


if __name__ == "__main__":
    main()
