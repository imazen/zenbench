"""Archive full native rounds on verified storage, with a compact Git pointer.

The packet excludes SSH command headers; benchmark stderr and all timing rounds
remain intact. Existing content-addressed files must match before reuse.
"""
import argparse
import gzip
import hashlib
import json
from pathlib import Path
import re
import shlex
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('log', type=Path)
    parser.add_argument('label')
    parser.add_argument('source_commit')
    parser.add_argument('--archive-host', required=True)
    parser.add_argument('--date', default='2026-10-11')
    args = parser.parse_args()
    if not re.fullmatch(r'[a-z0-9_-]+', args.label):
        parser.error('label must be a filename token')
    if not re.fullmatch(r'[0-9a-f]{40}', args.source_commit):
        parser.error('source commit must be the full recorded commit ID')
    if not re.fullmatch(r'\d{4}-\d{2}-\d{2}', args.date):
        parser.error('date must be YYYY-MM-DD')
    if args.archive_host.startswith('-') or any(c.isspace() for c in args.archive_host):
        parser.error('archive host must be one trusted SSH alias')
    packet = next(json.loads(line) for line in args.log.read_text().splitlines()
                  if line.startswith('{'))
    data = gzip.compress(json.dumps(packet, separators=(',', ':')).encode(), mtime=0)
    digest = hashlib.sha256(data).hexdigest()
    filename = f'{args.label}_{digest}.json.gz'
    primary = f'/mnt/v/output/zenbench/windows-verification/{args.date}/{filename}'
    mirror = f'/mnt/tower/output/zenbench/windows-verification/{args.date}/{filename}'
    program = '''import hashlib,pathlib,sys
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
'''
    command = shlex.join(['python3', '-B', '-c', program, digest, primary, mirror])
    subprocess.run(['ssh', '-o', 'BatchMode=yes', '-o', 'StrictHostKeyChecking=yes',
                    '--', args.archive_host, command], input=data, check=True, timeout=90)
    source_hashes = {}
    for path in ['examples/windows_repeatability.rs', 'examples/windows_repeatability.ps1',
                 'src/engine.rs', 'src/stats.rs', 'Cargo.lock']:
        source = subprocess.run(['git', 'show', f'{args.source_commit}:{path}'],
                                capture_output=True, check=True).stdout
        source_hashes[path] = hashlib.sha256(source).hexdigest()
    pointer = Path('benchmarks') / f'{args.label}_{args.date}.pointer.md'
    text = (f'# Native Windows repeatability: {args.label}\n\n'
            f'Observed {args.date}. Source commit `{args.source_commit}`.\n'
            f'Executable SHA-256: `{packet["binary_sha256"]}`.\n\n'
            f'Full rounds, execution orders, native RSS, and benchmark stderr are in\n'
            f'`{primary}` on `{args.archive_host}`, with a verified copy at\n'
            f'`{mirror}` on the same host. Both paths were checked during archival.\n'
            f'Compressed bytes: {len(data)}. SHA-256: `{digest}`.\n\n'
            'Build: `just windows-build-example windows_repeatability` (add\n'
            '`--no-default-features` for the QPC-only executable). Run:\n'
            '`just windows-repeatability-remote <trusted-windows-alias> <CPU-sets> <repeats>`.\n'
            'Exact CPU sets/repetitions and timing mode are in the packet. The native\n'
            'runner uses a 2 GiB job memory cap and idle priority. Resource admission\n'
            'is disabled explicitly; these observations do not certify quietness or\n'
            'maximum frequency.\n\n'
            'Source hashes at the recorded commit:\n\n')
    text += '\n'.join(f'- `{p}`: `{h}`' for p, h in source_hashes.items()) + '\n'
    if pointer.exists():
        assert pointer.read_text() == text, 'Existing pointer differs; preserve before replacing'
    else:
        pointer.write_text(text)
    print(pointer)


if __name__ == '__main__':
    main()
