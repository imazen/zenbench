# External paired-process inference and validation

The implemented entry point is [`zenbench pair`](external-process.md). Its
population is fresh child invocations grouped into fixed randomized blocks,
not the internal timing vector and not historical before/after runs. Every arm
appears once in each block; launches are serial. Correctness failures invalidate
whole blocks. Confirm additionally requires the entire declared experiment.

## Estimator and uncertainty

For each child, average its normalized internal timings arithmetically. For block
`j`, set `d[j] = log(mean(candidate[j])) - log(mean(baseline[j]))`. Estimate the
geometric latency ratio by `exp(mean(d))`, reporting `100 * (ratio - 1)` percent.
Minimum, median and MAD of invocation means and of raw paired ratios are separate
descriptive columns; they are not substituted for this estimand.

With `n` planned complete blocks, the statistical log interval is
`mean(d) ± t(1 - (1-confidence)/(2*family), n-1) * sd(d)/sqrt(n)`.
The family counts all selected case/candidate contrasts and A/A controls, including
failed cells. Bonferroni does not require the contrasts to be independent; the
individual t intervals still assume independent, approximately normal block log
ratios. A common baseline induces dependence among contrasts and is not additional
independent data. See [NIST's t interval](https://www.itl.nist.gov/div898/handbook/eda/section3/eda352.htm)
and [SciPy's paired test and interval](https://docs.scipy.org/doc/scipy/reference/generated/scipy.stats.ttest_rel.html).

The declared absolute error bound `r` is normalized to nanoseconds/work. A child
mean must exceed `r`. For each block the bounded true ratio lies between
`(a-r)/(b+r)` and `(a+r)/(b-r)`. Average the corresponding log bounds, then extend
outward by the t uncertainty. This adds bounded measurement uncertainty rather
than shrinking it with `sqrt(n)` or assuming independent quantization error. It
covers a conservative absolute error contract, not all systematic timer bias.
The independent SciPy oracle checks endpoints with nonzero resolution, identical
measurements, unequal measurements and different family sizes.

## Decisions

The A/A interval must fit within the declared symmetric percentage control margin.
It is an equivalence requirement, not failure to reject a difference from zero.
A candidate interval wholly below `-margin` is an improvement; wholly above
`+margin` is a regression; wholly inside the margin is equivalent; with only its
upper endpoint at or below `+margin`, it is non-inferior. Otherwise it is
inconclusive. The declared `accept` rule determines which verdicts exit zero.
A/A must pass before any candidate is accepted. An invalid resource, correctness,
placement or completion contract takes precedence. No observed A/A bias is
subtracted, and an effect larger than the A/A point estimate is diagnostic only.

There is no adaptive stopping or automatic extension. Confirm refuses resume;
separate attempts remain separate populations. Explore first, then freeze the
case set, binaries, inputs, settings, margins and block budget. Do not keep trying
until one study passes. A new study after a failed one must preserve the earlier
attempt and disclose the selection process in any consequential claim.

## Independent controls

Reproduction sources:

- [`pair_inference_oracle.py`](../examples/pair_inference_oracle.py) and
  [numerical expectations](../tests/fixtures/pair_intervals.json).
- [`pair_coverage_oracle.py`](../examples/pair_coverage_oracle.py) and
  [fixed-seed model results](../tests/fixtures/pair_coverage.json).
- Rust numerical/decision tests in `src/bin/support/pair/analysis.rs` and
  `src/bin/support/pair.rs`; real process/extraction/correctness tests in
  `tests/pair_cli.rs`. Run `just pair-confirm-check` through the host's resource
  wrapper. Oracle generators refuse to overwrite reviewed expectations.

The model experiment used SciPy 1.18.1, seed 290012, 4,000 trials per cell, two
contrasts, 95% family confidence and 1% candidate/control margins. These are
simulated measurements, not hardware benchmarks. At five blocks, the iid null
interval covered zero in 3,896/4,000 trials and accepted no improvement/regression;
the known 10% slowdown was accepted in 3,978/4,000 trials. Randomized linear drift
accepted no change and established A/A equivalence in 0/4,000 trials. The tested
carryover model accepted no change; its coverage deteriorated at twenty blocks.
All cells, including failures, are retained in the linked results.

The deliberately invalid candidate-specific correlated-offset model exposed
**2,887/4,000 false accepted changes at five blocks despite mostly passing A/A**.
This is a negative control and a limitation, not calibrated validity under
correlation. No generic A/A test can identify a disturbance that affects only B
and is observationally indistinguishable from B's real effect. More repetitions
within the same confounded setting can strengthen the wrong conclusion. Inspect
order/raw data, control workload differences, and use independent machines/build
layouts/studies for broader claims. Zenbench's verdict is conditional on its
stated model and measured contract; it is not universal certification.

## Resource observations and wall cost

Iterate retains its four-block default and boundary observations. Confirm requires
at least five blocks and samples known-worker-adjusted foreign activity during
execution as well. It requires at least one complete during-child observation;
very short child processes may need a longer fixed internal workload. The OS
sampling interval, observer overhead and the process-exit attribution gap are
specified in the [runner contract](external-process.md#resource-and-priority-observations).
They do not disappear because the internal metric excludes warmup.

For one candidate with A/A, four blocks schedule 12 invocations and five blocks
schedule 15. These are exact launch counts, not a runtime multiplier. Monitor,
settle, checker, startup and cleanup costs are all retained in total elapsed time;
outer invocation wall time is distinct from the internal metric. No general
wall-time penalty is claimed from those counts. Codec acceptance measurements
are reported in the [acceptance record](pair-acceptance.md) with their actual
commands and artifact identities.

The optional intercept/slope analysis over a numeric case axis is tracked
separately in [issue 39](https://github.com/imazen/zenbench/issues/39). It does not
change fixed-block confirmation or its family of comparisons.
