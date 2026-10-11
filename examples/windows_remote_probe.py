"""Run a read-only PowerShell oracle through existing SSH credentials.

Optional relay separates the credential host from the network jump. Host keys
remain checked; an explicitly supplied known-hosts file must already be trusted.
No remote files are written and no private keys are copied.
"""
import argparse
import base64
import gzip
from pathlib import Path
import shlex
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('host')
    parser.add_argument('--relay')
    parser.add_argument('--network-jump')
    parser.add_argument('--known-hosts', type=Path)
    parser.add_argument('--cpu-id', type=int,
                        help='Optional 0..63 argument for the process-mask oracle')
    parser.add_argument('--script', type=Path,
                        default=Path(__file__).with_name('windows_topology_probe.ps1'))
    args = parser.parse_args()
    if args.cpu_id is not None and not 0 <= args.cpu_id < 64:
        parser.error('--cpu-id must be in 0..63')
    for host in [args.host, args.relay, args.network_jump]:
        if host is not None and (not host or host.startswith('-') or any(c.isspace() for c in host)):
            parser.error('SSH aliases must be nonempty single destination tokens')
    base = ['ssh', '-o', 'BatchMode=yes', '-o', 'StrictHostKeyChecking=yes',
            '-o', 'ConnectTimeout=10']
    command = base.copy()
    if args.known_hosts:
        command += ['-o', f'HostKeyAlias={args.host}']
        if not args.relay:
            command += ['-o', f'UserKnownHostsFile={args.known_hosts.resolve()}']
        else:
            entries = [line for line in args.known_hosts.read_text().splitlines()
                       if line.split() and line.split()[0] == args.host]
            if len(entries) != 1 or len(entries[0].split()) != 3:
                parser.error('Relay pin file must contain one trusted unqualified host-key entry for the alias')
            # KnownHostsCommand expands % tokens before exec; %% becomes %.
            # printf prints the pinned public record; no relay trust files change.
            pin = '/usr/bin/printf %%s ' + shlex.quote(entries[0])
            command += ['-o', 'UserKnownHostsFile=/dev/null',
                        '-o', f'KnownHostsCommand={pin}']
    if args.network_jump:
        jump = base + ['-W', '%h:%p', '--', args.network_jump]
        command += ['-o', f'ProxyCommand={shlex.join(jump)}']
    # The stdin/Console.ReadToEnd transport timed out on the native test host.
    # Compress the bounded script into an ASCII command instead; no remote files
    # or interactive parsing. Reject scripts beyond cmd.exe's command-line limit.
    payload = base64.b64encode(gzip.compress(args.script.read_bytes(), mtime=0)).decode('ascii')
    decoder = (
        f"$b=[Convert]::FromBase64String('{payload}');"
        "$m=[IO.MemoryStream]::new($b);"
        "$z=[IO.Compression.GZipStream]::new($m,[IO.Compression.CompressionMode]::Decompress);"
        "$r=[IO.StreamReader]::new($z);"
        "try { & ([ScriptBlock]::Create($r.ReadToEnd())) "
        + (str(args.cpu_id) if args.cpu_id is not None else '')
        + " } finally { $r.Dispose();$z.Dispose();$m.Dispose() }"
    )
    powershell = 'powershell.exe -NoProfile -Command "' + decoder + '"'
    if len(powershell) > 7500:
        parser.error('Compressed script exceeds the supported Windows command-line size')
    command += ['--', args.host, powershell]
    if args.relay:
        command = base + ['--', args.relay, shlex.join(command)]
    result = subprocess.run(command, input=b'', stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, timeout=90)
    sys.stdout.buffer.write(result.stdout)
    sys.stderr.buffer.write(result.stderr)
    raise SystemExit(result.returncode)


if __name__ == '__main__':
    main()
