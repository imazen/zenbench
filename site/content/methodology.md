+++
title = "Understand the methodology"
description = "Separate exploratory paired rounds from fixed-process acceptance."
weight = 12
+++

A benchmark measures the timing boundary you wrote. Setup, allocation, destruction,
dispatch, synchronization, warmup and cache reuse change the question. Verify output
correctness and equal work before interpreting a faster number.

## What is repeated?

| Unit | Meaning |
|---|---|
| Iteration | One operation requested from the callback |
| Sample | A timed batch, possibly containing many iterations |
| Round | One sample of every arm, in shuffled serial order |
| Process | Fresh execution containing warmup and rounds |
| Build | A separately compiled executable |
| Study cell | Workload × machine/class × configuration |

Many iterations or rounds in one process do not become independent process
repetitions. Many fresh processes of one executable do not measure variation
between builds. Hardware classes remain separate cells.

## Interleave while exploring

Native Suite groups run one sample per arm per round in shuffled order:

```text
Round 1: B then A
Round 2: A then B
Round 3: B then A
...
```

Nearby comparisons can share slowly changing conditions. They do not experience
identical conditions: a spike can hit one arm, memory/cache pressure can affect
arms differently, and carryover can change the next arm's state. Randomization
and pairing reduce some biases without guaranteeing cancellation.

Iterate normally uses one process, up to 30 rounds and 100 ms warmup per arm.
Explicit group settings apply. Paired IQR filtering and bootstrap intervals are
exploratory diagnostics. MAD describes spread robustly; Wilcoxon, effect size and
drift help investigate an experiment, not vote on CI acceptance. Adaptive
convergence belongs to exploration. A narrow interval or “significant” flag is
not a Confirm verdict.

## Confirm the frozen experiment

Native Confirm fixes processes and rounds, retains every observation and uses
one mean latency per process and arm reconstructed from raw rounds. Same-binary
arm comparisons use Student-t intervals on process log ratios. Separate saved
baseline/candidate populations use Welch intervals on process log latencies.
Both report a geometric latency ratio, which can differ from the descriptive
arithmetic means. Positive change means slower.

The declared family confidence covers the suite's comparison family through
Bonferroni correction. It does not cover a whole campaign's earlier candidate
selection or every hardware cell. Timer resolution supplies a minimum interval
width. The entire interval determines acceptance against the predeclared margin:

| Interval relative to the margin | Interpretation |
|---|---|
| Entirely above the positive margin | Regression |
| Entirely below the negative margin | Improvement |
| Entirely inside both margins | Equivalent within the margin |
| Upper bound at or below the positive margin | Non-inferior within the margin |
| Otherwise | Inconclusive |

[Confirmation and CI](@/ci-regression.md) covers the required evidence, exits and
promotion. Fresh execution does not prove independence or approximately normal
process log values. The recorded numerical/model controls validate a stated model;
they do not prove coverage for every real workload.

## Observe resources and verify placement

CPU-class admission is separate from resource observation. Linux strict placement
uses a hard cpuset; supported Windows topology uses an inherited Job boundary.
macOS advisory hints cannot certify P/E class confinement. Explicit unrestricted
placement permits exploration only. A boundary does not reserve the CPU, shared
cache, SMT sibling or memory bandwidth.

The engine primes a background resource sampler before warmup; rounds read cached
observations without a blocking refresh. Ordinary load remains visible in Iterate.
Confirm requires fresh clean observations and complete fixed data; flagged or
unknown observations invalidate it. Monitoring uses whole-host counters and
cannot certify continuous quietness on the selected domain. A FIFO cooperative
queue reduces overlap among participating jobs; unwrapped work still runs.
[CPU and shared-machine guidance](@/cpu-placement.md) explains the practical setup.

## Timing diagnostics have limits

Supported precise timers use TSC on x86_64 or the architectural counter on AArch64;
other paths use `Instant`. Calibration, batching and overhead compensation help
interpret small operations, but sub-nanosecond batch averages do not prove an
individual operation was resolved below one timer tick. Timer ticks are not
retired instructions or frequency-dependent execution-core cycles.

Warmup does not prove maximum frequency or steady state. Optional cache spoiling
does not reset an entire machine, and stack jitter does not randomize code/heap
layout or implement STABILIZER. The minimum is a useful low observation; it is
not universal true performance. Use the confirmation estimand for acceptance.

[Profiler runs](@/profiling.md) answer separate memory/instruction questions.
Instrumentation can perturb timing and scheduling. Confirm native latency with
an uninstrumented build.

## External A/B and provenance

[External pair plans](@/external-programs.md) randomize fresh serial processes,
retain internal vectors in their invocation clusters, and check outputs and A/A
controls. Their default Iterate summaries remain exploratory.
[External Confirm](@/external-confirmation.md) uses fixed Student-t intervals on
block log ratios, preserves invocation clustering, adds bounded timing error and
requires the A/A interval within its control margin. It cannot resume or extend
a plan after inspection. The same independence assumptions and candidate-specific
confounding limits still need review.

Runtime executable hashes identify executed bytes. Runtime Git HEAD does not
identify the source used to build an old binary; clean/dirty state and a complete
build manifest remain external. Keep exact builds/inputs, controls, plans,
unsuccessful attempts and [raw archives](@/results.md) behind selected summaries.

## Read the full protocol and research context

The [full measurement protocol](https://github.com/imazen/zenbench/blob/main/METHODOLOGY.md)
contains equations, source mappings, numerical controls, measured local cost and
threats to validity. Its historical measurements are not speed predictions for
your workload. Background reading:

- Mytkowicz et al., [Producing Wrong Data Without Doing Anything Obviously Wrong](https://sape.inf.usi.ch/publications/asplos09.html), ASPLOS 2009: layout and setup bias.
- Curtsinger & Berger, [STABILIZER](https://people.cs.umass.edu/~emery/pubs/stabilizer-asplos13.pdf), ASPLOS 2013: repeated layout randomization.
- Chen & Revels, [Robust Benchmarking in Noisy Environments](https://arxiv.org/abs/1608.04295), HPEC 2016: timer resolution, batching and noisy distributions.
- Kalibera & Jones, [Rigorous Benchmarking in Reasonable Time](https://kar.kent.ac.uk/33611/), ISMM 2013: variation at iteration, execution and build levels.
- Tratt, [What Metric to Use When Benchmarking?](https://tratt.net/laurie/blog/2022/what_metric_to_use_when_benchmarking.html), 2022: interpreting summary metrics.
