"""Deterministic SciPy controls; correlated processes/optional stopping are exclusions."""
import json
import math
from pathlib import Path
import scipy
from scipy.stats import t

state = 42
mask = (1 << 64) - 1
def uniform():
    global state
    state = (state * 6364136223846793005 + 1442695040888963407) & mask
    return ((state >> 11) + 0.5) / (1 << 53)
def normal():
    return math.sqrt(-2 * math.log(uniform())) * math.cos(2 * math.pi * uniform())
def excludes(values, truth=0.0):
    mean = sum(values)/len(values)
    var = sum((v-mean)**2 for v in values)/(len(values)-1)
    width = critical[len(values)] * math.sqrt(var/len(values))
    return truth < mean-width or truth > mean+width

trials = 2000
critical = {n: float(t.ppf(.975, n-1)) for n in range(3, 21)}
counts = dict(null=0, known_effect_misses=0, known_effect_detected=0, correlated=0, optional_stopping=0)
for _ in range(trials):
    values = [0.03*normal() for _ in range(20)]
    counts['null'] += excludes(values[:5])
    shifted = [v+math.log(1.1) for v in values[:5]]
    counts['known_effect_misses'] += excludes(shifted, math.log(1.1))
    counts['known_effect_detected'] += excludes(shifted)
    correlated = [values[0]]
    for v in values[1:5]:
        correlated.append(.9*correlated[-1]+math.sqrt(1-.9**2)*v)
    counts['correlated'] += excludes(correlated)
    counts['optional_stopping'] += any(excludes(values[:n]) for n in range(3,21))
output = Path('tests/fixtures/process_coverage.json')
if output.exists():
    raise SystemExit('Refusing to replace existing expectations without explicit review')
output.write_text(json.dumps(dict(oracle='SciPy '+scipy.__version__, seed=42, trials=trials, counts=counts), indent=2)+'\n')
print(output.read_text())
