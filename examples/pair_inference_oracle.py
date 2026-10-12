"""Independent SciPy paired-block oracle; refuses to overwrite expectations.

References: https://docs.scipy.org/doc/scipy/reference/generated/scipy.stats.ttest_rel.html
https://www.itl.nist.gov/div898/handbook/eda/section3/eda352.htm
Run from repository root: python3 examples/pair_inference_oracle.py
"""
import json
from pathlib import Path
import numpy as np
import scipy
from scipy import stats
cases=[]
for baseline, arm in [([100,103,99,101,104],[91,92,90,89,94]),
                      ([100]*5,[100]*5), ([20,25,24,23,27],[21,24,23,25,28])]:
    b,a=np.array(baseline,dtype=float),np.array(arm,dtype=float)
    for family in [2,12]:
        for resolution in [0.001,1.0]:
            logs=np.log(a)-np.log(b)
            # SciPy's independent implementation of the paired mean interval.
            width=stats.t.ppf(1-0.05/(2*family),len(a)-1)*stats.sem(logs)
            low=np.mean(np.log((a-resolution)/(b+resolution)))-width
            high=np.mean(np.log((a+resolution)/(b-resolution)))+width
            cases.append(dict(base=baseline,arm=arm,resolution=resolution,family=family,
                estimate_pct=100*np.expm1(np.mean(logs)),lower_pct=100*np.expm1(low),upper_pct=100*np.expm1(high)))
out=Path('tests/fixtures/pair_intervals.json')
if out.exists(): raise SystemExit('Refusing to replace reviewed oracle expectations')
out.write_text(json.dumps(dict(oracle='SciPy '+scipy.__version__,cases=cases),indent=2)+'\n')
print(out)
