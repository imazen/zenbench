"""Independent SciPy oracle for fixed-process log-ratio/latency intervals."""
import json
from pathlib import Path
import numpy as np
import scipy
from scipy.stats import t

cases = []
for baseline, candidate in [
    ([100, 110, 90], [95, 101, 88]),
    ([100, 120, 97, 108, 110], [100, 120, 97, 108, 110]),
    ([2, 4, 6, 8, 10, 12], [3, 5, 7, 9, 11, 13]),
    ([100, 101, 99, 102, 98], [110, 114, 107, 116, 105]),
]:
    b, c = np.log(baseline), np.log(candidate)
    for family in [1, 10]:
        d = c-b
        se = np.std(d, ddof=1) / np.sqrt(len(d))
        width = t.ppf(1-0.05/family/2, len(d)-1) * se
        paired = (100*np.expm1(np.mean(d)), 100*np.expm1(np.mean(d)-width), 100*np.expm1(np.mean(d)+width))
        bv, cv = np.var(b, ddof=1)/len(b), np.var(c, ddof=1)/len(c)
        df = (bv+cv)**2/(bv*bv/(len(b)-1)+cv*cv/(len(c)-1))
        width = t.ppf(1-0.05/family/2, df)*np.sqrt(bv+cv)
        m = np.mean(c)-np.mean(b)
        independent = (100*np.expm1(m), 100*np.expm1(m-width), 100*np.expm1(m+width))
        cases.append(dict(base=baseline, candidate=candidate, family=family, paired=paired, independent=independent))
output = Path('tests/fixtures/process_intervals.json')
if output.exists():
    raise SystemExit('Refusing to replace existing oracle expectations without explicit review')
output.write_text(json.dumps({'oracle':'SciPy '+scipy.__version__, 'cases':cases}, indent=2)+'\n')
print(output)
