"""Validate the published JSON examples through a supplied prebuilt CLI's dry-run."""
import copy
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile


def main():
    site = Path(__file__).parents[1]
    binary = Path(sys.argv[1]).resolve(strict=True)
    content = (site / "content/external-programs.md").read_text()
    plan = json.loads(re.findall(r"```json\n(.*?)\n```", content, re.S)[0])
    fragment = json.loads(re.findall(r"```json\n(.*?)\n```", (site / "content/external-confirmation.md").read_text(), re.S)[0])
    scratch = Path(os.environ.setdefault("TMPDIR", str(Path.home() / "tmp")))
    scratch.mkdir(parents=True, exist_ok=True)
    retained = Path(tempfile.mkdtemp(prefix="zenbench-site-plans-", dir=scratch))
    (retained / "inputs").mkdir()
    (retained / "inputs/small.bin").write_bytes(b"documentation parser control\n")
    plan["baseline"]["program"] = sys.executable
    plan["candidates"][0]["program"] = sys.executable
    variants = [("complete-iterate", plan, "iterate")]
    confirmed = copy.deepcopy(plan)
    confirmed.update(fragment)
    variants.append(("complete-confirm", confirmed, "confirm"))
    # The table contains literal JSON objects, including their actual escaping.
    for value in re.findall(r"`(\{\"kind\":.*?\})`", content):
        obj = json.loads(value)
        variant = copy.deepcopy(plan)
        if obj["kind"] in {"prefix", "tsv", "regex", "wall"} or "pointer" in obj:
            variant["metric"]["extract"] = obj
            if obj["kind"] == "wall":
                variant["metric"].update(unit="ns", expected=1)
        else:
            variant["check"] = obj
            if obj["kind"] == "command":
                variant["check"]["program"] = sys.executable
        variants.append((f"table-{obj['kind']}-{len(variants)}", variant, "iterate"))
    environment = dict(os.environ, ZENBENCH_CPU_POLICY="unrestricted")
    for name, variant, mode in variants:
        path = retained / f"{name}.json"
        path.write_text(json.dumps(variant))
        command = [str(binary), "pair", str(path), f"--mode={mode}", "--case", "small", "--dry-run"]
        result = subprocess.run(command, env=environment, capture_output=True, text=True, timeout=30)
        (retained / f"{name}.stdout.json").write_text(result.stdout)
        (retained / f"{name}.stderr.txt").write_text(result.stderr)
        if result.returncode != 0:
            raise SystemExit(f"{name} failed ({result.returncode}): {result.stderr}\nRetained: {retained}")
        resolved = json.loads(result.stdout)
        expected = variant["blocks"] * (len(variant["candidates"]) + 1 + int(variant["aa"]))
        if resolved["invocation_count"] != expected or resolved["mode"] != mode:
            raise SystemExit(f"{name}: resolved launch contract differs from example; retained: {retained}")
    print(f"Plan check: {len(variants)} actual CLI dry-runs passed; no workloads executed")
    print(f"Retained parser evidence: {retained}")


if __name__ == "__main__":
    main()
