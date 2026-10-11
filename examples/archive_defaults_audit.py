"""Archive audit inputs/outputs by digest and retain a small Git pointer."""
import argparse
import base64
import gzip
import hashlib
import json
from pathlib import Path
import re
import shlex
import subprocess


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("manifest", type=Path)
    parser.add_argument("binary", type=Path)
    parser.add_argument("directories", type=Path, nargs="+")
    parser.add_argument("--archive-host", required=True)
    parser.add_argument("--date", required=True)
    parser.add_argument("--label", default="defaults_audit")
    args = parser.parse_args()
    if not re.fullmatch(r"\d{4}-\d{2}-\d{2}", args.date):
        parser.error("date must be YYYY-MM-DD")
    if not re.fullmatch(r"[a-z][a-z0-9_]*", args.label):
        parser.error("label must contain lowercase letters, digits or underscores")
    if args.archive_host.startswith("-") or any(c.isspace() for c in args.archive_host):
        parser.error("archive host must be one trusted SSH alias")
    payload = {"manifest": json.loads(args.manifest.read_text()), "files": {}, "binary_files": {}}
    for directory in args.directories:
        assert directory.is_dir(), directory
        for path in sorted(directory.rglob("*")):
            if path.is_file():
                key = str(Path(directory.name) / path.relative_to(directory))
                assert key not in payload["files"], key
                try:
                    payload["files"][key] = path.read_text()
                except UnicodeDecodeError:
                    payload["binary_files"][key] = base64.b64encode(path.read_bytes()).decode("ascii")
    binary = args.binary.read_bytes()
    binary_sha = hashlib.sha256(binary).hexdigest()
    payload["binary_sha256"] = binary_sha
    payload["binary_base64"] = base64.b64encode(binary).decode("ascii")
    data = gzip.compress(json.dumps(payload, separators=(",", ":")).encode(), mtime=0)
    digest = hashlib.sha256(data).hexdigest()
    filename = f"{args.label}_{digest}.json.gz"
    paths = [f"{root}/output/zenbench/defaults-audit/{args.date}/{filename}"
             for root in ["/mnt/v", "/mnt/tower"]]
    program = """import hashlib,pathlib,sys
data=sys.stdin.buffer.read(64*1024*1024+1)
assert len(data)<=64*1024*1024
assert hashlib.sha256(data).hexdigest()==sys.argv[1]
for name in sys.argv[2:]:
 p=pathlib.Path(name); p.parent.mkdir(parents=True,exist_ok=True)
 if p.exists():
  assert hashlib.sha256(p.read_bytes()).hexdigest()==sys.argv[1]
 else:
  with p.open('xb') as f: f.write(data)
 assert hashlib.sha256(p.read_bytes()).hexdigest()==sys.argv[1]
print('Archived and verified primary/mirror sha256='+sys.argv[1])
"""
    command = shlex.join(["python3", "-B", "-c", program, digest, *paths])
    subprocess.run(["ssh", "-o", "BatchMode=yes", "-o", "StrictHostKeyChecking=yes",
                    "--", args.archive_host, command], input=data, check=True, timeout=60)
    pointer = Path("benchmarks") / f"{args.label}_{args.date}.pointer.md"
    assert not pointer.exists(), "do not overwrite an existing audit pointer"
    pointer.write_text(
        f"# Defaults audit artifacts ({args.date})\n\n"
        f"Full raw results, child results, logs, manifest and executable are retained\n"
        f"in `{paths[0]}` on `{args.archive_host}`, with a hash-verified mirror at\n"
        f"`{paths[1]}` on that same host. Both paths were verified during archival.\n\n"
        f"Compressed bytes: {len(data)}. SHA-256: `{digest}`.\n"
        f"Executable SHA-256: `{binary_sha}`.\n\n"
        "The payload is gzip-compressed JSON. Text artifacts are under `files`;\n"
        "binary artifacts are base64 under `binary_files`, and the executable\n"
        "is base64 under `binary_base64`. `manifest` records source\n"
        "provenance and commands. Reanalysis does not require another benchmark run.\n\n"
        "See [the audit](../docs/defaults-audit.md) for measured costs and limits.\n"
    )
    print(pointer)


if __name__ == "__main__":
    main()
