"""Local mode/exit/evidence oracle. Retain every attempt, including failures."""
import argparse
import gzip
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time

p = argparse.ArgumentParser()
p.add_argument('binary', type=Path)
p.add_argument('output', type=Path)
p.add_argument('--case', action='append', help='Run only explicitly selected control cases')
a = p.parse_args()
a.output.mkdir(parents=True, exist_ok=False)
base = {k: v for k, v in os.environ.items() if not k.startswith('ZENBENCH_')}
base['ZENBENCH_CPU_POLICY'] = 'auto'
base['ZENBENCH_GATE_TRACE'] = '1'
records = []
cases = [
    ('iterate', [], {}, 0),
    ('confirm', ['--mode=confirm'], {}, 1),
    ('evidence', ['--mode=evidence', '--scope=single-cpu/fold-1024'], {}, 1),
    ('dryrun', ['--mode=confirm', '--dry-run'], {}, 0),
    ('deadline', ['--mode=confirm', '--timeout=1'], {'PROTOCOL_STALL': '1'}, 2),
    ('missing_scope', ['--mode=evidence'], {}, 2),
    ('disabled_gate', ['--mode=confirm'], {'ZENBENCH_NO_BUSY_GATE': '1'}, 2),
    ('empty', ['--mode=confirm', '--group=does-not-exist'], {}, 2),
    ('typo', ['--mod=confirm'], {}, 2),
    ('bad_format', ['--format=jsno'], {}, 2),
]
for name, flags, extra, expected in cases:
    if a.case and name not in a.case:
        continue
    stem = a.output / name
    env = base | extra | {'ZENBENCH_DATA_DIR': str(stem.resolve()) + '.data'}
    command = [str(a.binary), *([] if name == 'bad_format' else ['--format=json']), *flags]
    start = time.perf_counter()
    with stem.with_suffix('.json').open('wb') as out, stem.with_suffix('.log').open('wb') as err:
        run = subprocess.run(command, env=env, stdout=out, stderr=err, timeout=330)
    row = {'case': name, 'command': command, 'exit': run.returncode, 'expected': expected,
           'wall_seconds': time.perf_counter() - start}
    records.append(row)
    (a.output / 'records.json').write_text(json.dumps(records, indent=2) + '\n')
    print(json.dumps(row), flush=True)
    assert run.returncode == expected, f'{name}: see retained stderr'
    if name == 'dryrun':
        data = json.loads(stem.with_suffix('.json').read_bytes())
        assert data['plan']['processes'] == 5
        assert len(data['groups']) == 1
        assert data['runtime_estimate'] is None
    if name in ('iterate', 'confirm', 'evidence'):
        data = json.loads(stem.with_suffix('.json').read_bytes())
        assert data['execution']['plan']['mode'] == name
        assert data['comparisons'][0]['completed_rounds'] == (30 if name == 'iterate' else 150)
        if name != 'iterate':
            assert len(data['replicates']) == 5
            assert len(data['process_comparisons']) == 1
            assert data['process_comparisons'][0]['verdict'] == 'regression'
            assert all(len(r['comparisons'][0]['samples']) == 30 for r in data['replicates'])
            assert all(not r['comparisons'][0]['config']['auto_rounds'] for r in data['replicates'])
        references = list(Path(env['ZENBENCH_DATA_DIR']).joinpath('runs').glob('*.json'))
        assert references
        for ref in references:
            meta = json.loads(ref.read_bytes())
            raw = (ref.parent.parent / 'objects' / (meta['sha256'] + '.json.gz')).read_bytes()
            assert hashlib.sha256(raw).hexdigest() == meta['sha256']
            json.loads(gzip.decompress(raw))
