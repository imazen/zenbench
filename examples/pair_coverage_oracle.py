"""Fixed-seed model checks, not empirical hardware coverage guarantees.

The arm-specific correlated offset is deliberately outside the model: A/A
cannot detect a confounder affecting only B. This negative control MUST expose
false acceptance; it is not a passing certification of correlated data.
"""
import json
from pathlib import Path
import numpy as np
import scipy
from scipy.stats import t
rng=np.random.default_rng(290012)
ntrials=4000
results=[]
for blocks in (5,20):
    for name in ('iid_null','iid_slow','linear_drift','carryover','arm_correlated_confounder'):
        ratios=[]
        for trial in range(ntrials):
            logs=np.empty((blocks,3))
            previous=None
            offset=rng.normal(0,0.04) if name=='arm_correlated_confounder' else 0
            for block in range(blocks):
                for position,arm in enumerate(rng.permutation(3)):
                    value=rng.normal(0,0.002)
                    if name=='iid_slow' and arm==1: value+=np.log(1.1)
                    if name=='linear_drift': value+=(block*3+position)*0.01
                    if name=='carryover' and previous==1: value+=0.02
                    if arm==1: value+=offset
                    logs[block,arm]=value
                    previous=arm
            d=logs[:,1:]-logs[:,0,None]
            m=d.mean(axis=0)
            width=t.ppf(1-0.05/4,blocks-1)*d.std(axis=0,ddof=1)/np.sqrt(blocks)
            lo,hi=np.expm1(m-width)*100,np.expm1(m+width)*100
            control=lo[1]>=-1 and hi[1]<=1
            ratios.append((lo[0]<= (10 if name=='iid_slow' else 0)<=hi[0],
                           control and hi[0]<-1,control and lo[0]>1,control))
        data=np.array(ratios)
        results.append(dict(model=name,blocks=blocks,trials=ntrials,
            ab_coverage=int(data[:,0].sum()),accepted_improvement=int(data[:,1].sum()),
            accepted_regression=int(data[:,2].sum()),control_equivalent=int(data[:,3].sum())))
# The fixed oracle detects both the intended model and its deliberate violation.
for row in results:
    if row['model']=='iid_null':
        assert row['ab_coverage']>=3800
        assert row['accepted_improvement']+row['accepted_regression']<=200
    if row['model']=='iid_slow': assert row['accepted_regression']>=3000
    if row['model']=='arm_correlated_confounder':
        assert row['accepted_improvement']+row['accepted_regression']>1000
out=Path('tests/fixtures/pair_coverage.json')
if out.exists(): raise SystemExit('Refusing to replace reviewed oracle expectations')
out.write_text(json.dumps(dict(oracle='SciPy '+scipy.__version__,seed=290012,results=results),indent=2)+'\n')
print(out.read_text())
