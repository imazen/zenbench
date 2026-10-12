+++
title = "Confirm an external comparison"
description = "Freeze a serial A/B experiment with timing error bounds and an A/A acceptance control."
weight = 5
+++

Use this after [external Iterate exploration](@/external-programs.md) selects a
correct candidate. Confirm applies to the same binary under different environments
or to two retained binaries. It freezes fresh serial A/B blocks and makes a
process-aware decision. Keep the inputs, work, build settings, placement and timing
boundary matched, with only the intended intervention different.

## 1. Freeze the plan and timing error bound

Create a new `confirm-plan.json` from your exploratory plan. Do not edit the plan
or data for the earlier attempt. Declare the full case inventory, candidate set,
block count, practical margin, control margin and family confidence before running.
Confirm requires:

- at least **five fixed blocks**;
- `aa: true` and checked correctness (`files`, `json` or `command`);
- a positive `metric.resolution` in the extracted unit, **before** `divide_by`;
- a `confirmation` object with margins, confidence and nonempty scope;
- verified placement, all required blocks and valid strict resource observations.

`resolution` is a conservative absolute error bound for **each reported timing**.
Determine it from the timing source and reporting path. Printed decimal places
alone do not establish an error bound. If you cannot justify one, keep the result
exploratory.

For the earlier millisecond-prefix example, replace the corresponding plan fields
with the following **schema example**. The `0.1` bound is illustrative: substitute
your established conservative bound before running. It is not a measured property
of your program.

```json
{
  "blocks": 5,
  "aa": true,
  "metric": {
    "extract": {"kind": "prefix", "prefix": "elapsed_ms="},
    "stream": "stdout",
    "unit": "ms",
    "expected": 3,
    "divide_by": 1,
    "resolution": 0.1
  },
  "confirmation": {
    "margin_pct": 1.0,
    "control_margin_pct": 1.0,
    "family_confidence": 0.95,
    "scope": "frozen-study-v1",
    "accept": "non_inferiority"
  }
}
```

This fragment is not a standalone plan: retain schema, arms, cases, correctness,
warmup and deadlines from your complete plan. Set `accept` to `non_inferiority`
(default), `improvement` or `equivalence` according to the claim you intend to require. All candidate cells must meet that
chosen rule. The margins are percentages of baseline latency. A different workload
or sampling plan is a separately named experiment, not an extension after seeing the result.

## 2. Inspect the frozen launches

After configuring [a verified CPU boundary](@/cpu-placement.md):

```sh
zenbench pair confirm-plan.json --mode=confirm --dry-run
```

Check the selected cases, resolved executable/input hashes, per-arm environments,
work count, correctness contract, timing bound, schedule and scope. With five
blocks, one baseline, one candidate and A/A, one case launches 15 fresh processes.
That is a count, not a runtime estimate. Native suite `--processes`/`--rounds`
flags do not configure `pair`; its block budget is in the JSON plan.

## 3. Run a fresh complete attempt

Choose a new output directory and a bounded admission wait:

```sh
zenbench pair confirm-plan.json --mode=confirm --output target/pair-confirm-01 --wait 60
zenbench data read target/pair-confirm-01/packet.json.gz
zenbench data read target/pair-confirm-01/packet.json.gz --format json
```

All blocks must complete with matching correctness and valid observations.
Confirm also samples whole-host resource checks during execution. Placement and
sampled checks do not certify exclusive use or continuous quietness on every core.
A busy machine can refuse or invalidate the experiment; preserve that outcome.
Each child needs a complete during-process sample after the initial process-counter
calibration window. Startup and exit transition windows remain in the evidence,
but cannot certify load. Very short children may finish before a qualifying sample
is available. That is invalid confirmation evidence: use Iterate or predeclare
a longer internal work budget with the same estimand. Do not weaken observations
to force a verdict.

Each child contributes one arithmetic mean of its internal vector. Each complete
block contributes one log ratio. Fixed Student-t intervals use block log ratios,
with Bonferroni correction across selected cases × (candidates + A/A). Bounded
timing error widens the interval; identical observations do not grant perfect
precision. Raw timings remain available with their invocation boundaries.

## 4. Read the control and decision

The report includes per-arm and paired-ratio minimum, median and MAD, the process
intervals and explicit invalid/inconclusive reasons. Positive latency change means
slower. The A/A interval must fit wholly inside the declared control margin before
a candidate can be accepted. Its observed bias is never automatically subtracted.

| Exit | Meaning | Next step |
|---|---|---|
| 0 | Every candidate cell passes the chosen acceptance rule | Accept only within the declared margin and scope |
| 1 | Confirmed regression | Reject or fix the candidate; keep the control |
| 2 | Invalid, inconclusive, incomplete or not-run | Retain the attempt and inspect its reasons |

Invalid/inconclusive cells take precedence over regressions in a mixed experiment;
inspect every cell's reasons. The accepted default is non-inferiority; exit 0 alone
does not mean “faster”. Read the candidate verdict to distinguish improvement from equivalence/non-inferiority.
A wide interval or a failed A/A control is unresolved evidence, not a pass.

Confirm cannot resume, pool attempts, drop outliers, extend a plan after inspection
or stop when a favorable interval appears. Fix an invalid experiment's cause, or
predeclare a new study with a suitable budget. Keep all attempts and disclose the
new plan. Iterate's `--resume` remains available only for exploratory work.

## Keep the claim within the experiment's assumptions

Intervals assume independent, approximately normal block log ratios. Fresh children
and A/A do not establish those assumptions or detect every candidate-specific
confounder. Binary identity does not identify all runtime libraries or build
provenance. Inspect controls, schedule and environment changes; retain build/input
manifests. Consequential claims need independently planned studies and coverage
of their intended workloads/machines.

Keep the full attempt, including failures, and export only selected CSV rows with
packet pointers. [Storage](@/results.md) explains review artifacts. External Confirm
produces its own pair decision; `data pair-diff` is a descriptive comparison of
packets, and suite baseline promotion remains a separate contract.

The [external-process contract](https://github.com/imazen/zenbench/blob/main/docs/external-process.md)
and [pair inference validation](https://github.com/imazen/zenbench/blob/main/docs/pair-inference.md)
contain the full current rules and controls.
