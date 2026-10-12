#!/usr/bin/env python3
"""Process/decision oracle with synthetic metrics, not a performance benchmark.

The fixed sleep permits full OS observation windows. The reported value is
explicitly synthetic; this checks supervision and inference plumbing only.
"""
import json
import os
from pathlib import Path
import sys
import time
if len(sys.argv)>1:
    root=Path(sys.argv[1]).resolve()
    root.mkdir(parents=True,exist_ok=False)
    def arm(name,value):
        return dict(id=name,program=sys.executable,args=[str(Path(__file__).resolve())],
                    env=dict(PAIR_CONTROL_VALUE=str(value)),revision='synthetic-control-v1')
    plan=dict(schema=1,baseline=arm('baseline',100),candidates=[arm('improvement',90)],
        cases=[dict(id='synthetic-process-control',inputs=dict(driver=str(Path(__file__).resolve())))],
        metric=dict(extract=dict(kind='prefix',prefix='TIME '),unit='ns',expected=1,resolution=0.001),
        check=dict(kind='json',prefix='CHECK ',pointers=['/output']),
        warmup='synthetic fixed values; no hardware latency claim',blocks=5,
        confirmation=dict(margin_pct=1,control_margin_pct=1,family_confidence=0.95,
                          scope='process-contract-control',accept='improvement'),seed=29)
    (root/'plan.json').write_text(json.dumps(plan,indent=2)+'\n')
else:
    time.sleep(0.6)
    print('TIME '+os.environ['PAIR_CONTROL_VALUE'])
    print('CHECK {"output":"identical"}')
