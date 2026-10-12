#!/usr/bin/env python3
"""Compare Rust ledger's lifetime peak to heaptrack's Rust allocation share."""
import json
from pathlib import Path
import subprocess
import sys
root=Path(sys.argv[1]).resolve()
root.mkdir(parents=True,exist_ok=False)
program=Path("target/debug/examples/heap_phases").resolve()
with (root/"stdout.txt").open("w") as stdout, (root/"stderr.txt").open("w") as stderr:
    subprocess.run(["heaptrack","--record-only","-o",str(root/"trace"),str(program)],stdout=stdout,stderr=stderr,check=True,timeout=30)
traces=list(root.glob("trace*"))
assert len(traces)==1,traces
with (root/"analysis.txt").open("w") as stdout:
    subprocess.run(["heaptrack_print","-f",str(traces[0]),"--print-flamegraph",str(root/"peak.stacks"),"--flamegraph-cost-type=peak"],stdout=stdout,check=True,timeout=30)
rows=[json.loads(s) for s in (root/"stdout.txt").read_text().splitlines() if s.startswith('{')]
assert len(rows)==1,rows
reported=rows[0]["lifetime"]["lifetime_peak_bytes"]
stacks=(root/"peak.stacks").read_text().splitlines()
observed=sum(int(s.rsplit(' ',1)[1]) for s in stacks if 'HeapProfiler' in s)
assert observed==reported,(observed,reported)
print(json.dumps({"oracle":"heaptrack-global-peak Rust allocator contributions","ledger_bytes":reported,"heaptrack_bytes":observed}))
