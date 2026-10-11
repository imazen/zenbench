"""Independent raw-round/SciPy replay and measured compact-export sizes."""
import argparse
import gzip
import hashlib
import json
import math
import os
from pathlib import Path
import subprocess
import numpy as np
from scipy.stats import t

p = argparse.ArgumentParser()
p.add_argument('cli', type=Path)
p.add_argument('directory', type=Path)
a = p.parse_args()
output = a.directory / 'exports'
output.mkdir(exist_ok=False)
records = []
for name in ['iterate', 'confirm', 'evidence']:
    source = a.directory / (name + '.json')
    packet = json.loads(source.read_bytes())
    archive = output / (name + '.json.gz')
    snapshot = output / (name + '.csv')
    analysis = output / (name + '.analysis.json')
    env = os.environ | {'ZENBENCH_DATA_DIR': str(output / 'data')}
    for command, dest in [('archive', archive), ('snapshot', snapshot), ('reanalyze', analysis)]:
        subprocess.run([str(a.cli), command, str(source), '--output', str(dest)], env=env, check=True)
    restored = json.loads(gzip.decompress(archive.read_bytes()))
    assert restored == packet, 'lossless packet round trip'
    decision = json.loads(analysis.read_bytes())
    assert decision['comparisons'] == packet.get('process_comparisons', [])
    if name != 'iterate':
        assert decision['validity_issues'] == []
        for c in packet['process_comparisons']:
            ratios = []
            floors = [[], []]
            for r in packet['replicates']:
                g = next(g for g in r['comparisons'] if g['group_name'] == c['group'])
                values = []
                for k, arm in enumerate([c['baseline'], c['candidate']]):
                    index = next(i for i,b in enumerate(g['benchmarks']) if b['name'] == arm)
                    means = [s['compensated_ns'][index]/s['iterations'] for s in g['samples']]
                    mean = sum(means)/len(means)
                    values.append(mean)
                    floors[k].append(2*r['timer_resolution_ns']/min(s['iterations'] for s in g['samples'])/g['benchmarks'][index]['summary']['mean'])
                ratios.append(math.log(values[1])-math.log(values[0]))
            center = float(np.mean(ratios))
            alpha = (1-c['family_confidence'])/c['comparisons_in_family']
            width = max(float(t.ppf(1-alpha/2, len(ratios)-1))*float(np.std(ratios, ddof=1))/math.sqrt(len(ratios)), sum(max(f) for f in floors))
            expected = [100*math.expm1(x) for x in (center, center-width, center+width)]
            actual = [c[k] for k in ('estimate_pct','lower_pct','upper_pct')]
            assert np.allclose(actual, expected, rtol=1e-8, atol=1e-8), (actual, expected)
    records.append(dict(case=name, pretty_json_bytes=source.stat().st_size,
                        gzip_bytes=archive.stat().st_size, snapshot_bytes=snapshot.stat().st_size,
                        analysis_bytes=analysis.stat().st_size,
                        binary_sha256=packet['execution']['binary_sha256'],
                        gzip_sha256=hashlib.sha256(archive.read_bytes()).hexdigest(),
                        retained_rounds=sum(len(g['samples']) for r in packet.get('replicates',[]) or [packet] for g in r['comparisons'])))
(output/'sizes.json').write_text(json.dumps(records, indent=2)+'\n')
print(json.dumps(records, indent=2))
