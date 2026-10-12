#!/usr/bin/env python3
"""Exercise explicit external backends; missing tools fail, never skip."""
import argparse
import json
import os
from pathlib import Path
import platform
import subprocess

p = argparse.ArgumentParser()
p.add_argument("output", type=Path)
p.add_argument("--backends", default="wall,os_peak,heaptrack")
a = p.parse_args()
a.output.mkdir(parents=True, exist_ok=False)
root = a.output.resolve()
cli = Path("target/debug/zenbench").resolve()
driver = Path("target/debug/examples/fleet_memory_fixture").resolve()
arch = {"AMD64": "x86_64", "arm64": "aarch64"}.get(platform.machine(), platform.machine())
os_name = {"Darwin": "darwin", "Windows": "windows", "Linux": "linux"}[platform.system()]
env = dict(os.environ, ZENBENCH_CPU_POLICY="unrestricted", ZENBENCH_COORDINATION_DIR=str(root / "queue"))
(root / "queue").mkdir()
for backend in a.backends.split(","):
    spec = {"schema": 1, "target": f"{arch}-test-{os_name}", "files": {
        "driver": {"source": str(driver), "executable": True}}, "plan": {
        "schema": 1, "program": "driver", "backend": backend, "operations": 1,
        "timeout_seconds": 30, "expected_correctness": "ok", "workload": {"fixture": "transient-1MiB"}}}
    path = root / f"{backend}.json"
    path.write_text(json.dumps(spec))
    subprocess.run([cli, "bundle", path, root / f"bundle-{backend}"], env=env, check=True)
    subprocess.run([cli, "fleet-run", root / f"bundle-{backend}", "--output", root / backend,
        "--experiment", "backend-control", "--machine", "test-cpu", "--domain", "unrestricted",
        "--replicate", "0", "--attempt", backend], env=env, check=True)
    packet = json.loads((root / backend / "packet.json").read_text())
    assert packet["status"] == "complete", packet
    if backend != "wall":
        assert packet["memory"][0]["bytes"] >= 1024 * 1024, packet
    print(backend, "PASS", packet["memory"])
