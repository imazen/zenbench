# Zenbench measurement protocol and its limits

This is the current methodology for the unreleased Iterate/Confirm/Evidence
implementation, audited against commit `798315f6694a0123378e01eb024250e8db2ecc56`
on 2026-10-12. It describes what the implementation measures, how decisions are
made, and where an experiment still needs its owner's judgment. The source map
at the end identifies the implementation behind each part of the protocol.
Historical measurements below name their own revisions; they are not benchmarks
of that audit commit or of a released version.

For commands, start with [workflows](docs/benchmark-workflows.md),
[mode configuration](docs/benchmark-modes.md), and `zenbench guide`.
For SIMD development, use the [archmage/magetypes workflow](docs/simd-optimization.md).

Contents: [question and units](#1-define-the-question-before-choosing-the-statistic),
[modes](#2-three-modes-two-levels-of-inference),
[placement and admission](#3-placement-warmup-and-admission),
[measurement algorithm](#4-within-process-measurement-algorithm),
[exploratory statistics](#5-exploratory-summaries),
[confirmation equations](#6-fixed-process-confirmation-exact-estimand-and-decision),
[busy workstations](#7-useful-measurements-on-a-busy-development-machine),
[external A/B](#8-external-executables-and-environment-ab-issue-29),
[evidence](#9-evidence-provenance-and-small-version-control-diffs),
[measured cost](#10-measured-wall-time-and-storage-cost),
[limitations](#11-threats-to-validity-and-what-remains-external),
[references](#12-research-context-and-references).

## Abstract

Zenbench measures competing functions in randomized, serial rounds inside a
process. This shortens the time between observations of competing implementations
and makes round-paired diagnostics useful during optimization. It does not make
the observations independent or remove all interference. The default Iterate
mode prioritizes a short feedback loop and labels its comparisons exploratory.
Confirm repeats a fixed experiment in fresh processes and bases acceptance on
process-level uncertainty, a declared practical margin, complete retained samples,
and placement/resource checks. Evidence uses the same protocol for a named study
cell. Neither mode substitutes for workload correctness, representative inputs,
build provenance, or an appropriate machine population.

The October 2026 changes removed a blocking resource-counter refresh from each
round, reduced exploratory defaults, introduced fixed-process confirmation, and
retained losslessly compressed evidence separately from small Git summaries.
An unchanged 200-round control went from 41.229 seconds with the old monitor to
a 1.967-second median with the new monitor. A separate final-mode control took
0.766 seconds in Iterate and 3.828 seconds in five-process Confirm. These are
local measurements with different purposes, not a general speedup prediction.

## 1. Define the question before choosing the statistic

A benchmark needs an explicit estimand: the quantity the experiment intends to
estimate. Zenbench's usual observation is elapsed nanoseconds divided by a batch's
iteration count. It measures the callback's chosen boundary. That might be a hot
kernel, a complete operation including allocation, a synchronization workload,
or work submitted to a pool and awaited to completion. These are different
questions, even when they call the same implementation.

Define and retain:

- the semantic operation and correctness contract, input identities and weights;
- included work: dispatch, setup, allocation, teardown, I/O and synchronization;
- cache/reuse regime, pool size, concurrency and CPU/cache class;
- compiler/build configuration and the executable bytes actually run;
- baseline, candidate, comparisons of interest, practical margin and stopping plan.

Correctness comes first. A faster implementation that changes required output,
quality, frame count or work performed is not an optimization under that contract.
For approximate algorithms, predeclare the permitted error or quality constraint;
byte equality is appropriate only when the contract requires it. The general
closure API cannot verify these conditions automatically.

Within a process, descriptive rows summarize round-level per-iteration latencies.
Confirmation estimates a geometric ratio of process mean latencies, described in
section 6. It does not estimate tail latency, a production request distribution,
energy, or a universal speed of an algorithm. A hot repeated kernel and a service
under concurrent requests require different experiments. Throughput labels are
only as correct as the work count supplied by the benchmark owner.

### Units of repetition

| Unit | Meaning | What it can expose |
|---|---|---|
| Iteration | One requested unit of callback work | Cost inside the chosen timing boundary |
| Sample/batch | One timed batch, possibly many iterations | Timer amortization and short-term variation |
| Round | One sample of every arm in shuffled order | Nearby comparisons under changing conditions |
| Process | Fresh execution, containing warmup and rounds | Variation between executions of a fixed executable |
| Build | A separately produced executable | Compiler, link and layout differences |
| Study cell | Workload × machine/class × configuration | The declared scope of a claim |

Many iterations in one process do not become many independent process samples.
Many processes from one build do not measure variation between builds. Pooling
hardware classes does not create a representative population unless that
population and its weights were defined in advance.

## 2. Three modes, two levels of inference

| Mode | Default execution | Intended use | Acceptance meaning |
|---|---|---|---|
| Iterate | One process, up to 30 rounds per group, 100 ms warmup per arm | Edit, inspect assembly, compare local alternatives | Exit 0 means execution succeeded |
| Confirm | Five fresh processes, fixed group rounds, normally 30 each | Evaluate a frozen candidate or save a baseline population | Process-level validity and margin decision |
| Evidence | Confirm plus required scope identity | One cell of a reproducible study | Same inference; matrix coverage remains external |

Confirm/Evidence require a caller-supplied workload ID, at least three processes,
and at least two fixed rounds. Defaults are a 1% practical margin, 95% family
confidence and a 300-second supervisor deadline. Five processes is a default
budget, not a power guarantee. Two rounds is an accepted minimum, not a claim
that every workload stabilizes after two rounds. A scope identifies a cell;
it does not automatically run every architecture, cache class or corpus.

The plan resolves CLI settings before environment settings before suite defaults.
`--dry-run` prints the resolved plan, selected groups, placement, executable hash
and data directory. Registration has already run, but benchmark callbacks have
not. Budgets are not predictions of elapsed time.

Exploration can be adaptive. Confirmation freezes the number of processes and
rounds and retains every observation. It cannot reuse an Iterate packet as if
it came from a fixed confirmation plan. Immediate-mode criterion compatibility
also lacks the required process evidence. Legacy best-of repetition policies
remain descriptive and cannot authorize baseline promotion.

A useful development cycle is: establish correctness, explore cheaply, choose
one candidate and a margin, freeze the experiment, then confirm on fresh data.
Repeatedly looking at a confirmation and adding samples until it passes is a
different statistical procedure; the current intervals do not support it.

## 3. Placement, warmup and admission

### CPU classes and process boundaries

By default, unknown or asymmetric CPU topology requires an explicit verified
uniform placement. Hybrid performance/efficiency cores and unequal last-level
cache capacities can change results. More cache does not imply that a class is
faster for every workload. Run selected classes separately and retain their
identities. `zenbench cpu-info` reports the observed topology.

Thread affinity confines one thread. It does not retroactively configure an
opaque Rayon pool. Strict placement therefore needs a process boundary:

| Platform | Current boundary and limitation |
|---|---|
| Linux | An effective cgroup cpuset restricts the process's threads; the launcher can use a delegated controller or an existing suitable boundary. A `taskset` mask alone is insufficient to prevent a library widening its affinity. |
| Supported Windows x86/x64 topology | An inherited affinity-limited Job Object constrains the process and descendants. Multiple processor groups and Windows ARM core-class discovery are not supported for strict class selection. |
| macOS | Mach affinity tags and pthread QoS are advisory. They do not certify P/E placement; strict class binding is unsupported. |

Group thread-affinity constraints must fit within the process boundary. Zenbench
applies them to its own workers and checks placement around rounds. Ordinary
preexisting workers need the process boundary; already separate services need
their own controls. A privileged actor changing containment between checks is
outside the enforcement model.

A boundary is not a reservation: unrelated tasks can still use those CPUs, SMT
siblings, caches, memory controllers and package power. Equal cache capacity
also does not prove that threads share one LLC or NUMA domain. Explicit
`unrestricted` permits exploration and records unverified placement; it cannot
confirm a baseline. [Placement details and native controls](docs/process-affinity.md)
describe the OS-specific evidence.

### Warmup is a declared workload phase

Before measurement, each arm repeatedly runs with one requested iteration until
its warmup duration is reached. The default is 100 ms per arm. Iteration estimation
then makes additional untimed calls. macOS also runs a separate 100 ms demand
loop before calibration. None of these certify maximum frequency, thermal steady
state, warmed external workers, or a workload-independent steady distribution.
A callback longer than the requested duration can exceed that budget.

Warmup changes the question being measured: caches, branch history, allocator
state and data reuse can differ from a first request. Set a regime appropriate
to deployment. The `cold_start` option requests one iteration and cache spoiling;
it does not create a new process for every round or reset page cache, predictors,
allocator, or external services. Explicit warmup still precedes sampling. The
`cold_start_ns` field is the first iteration-estimation call **after** that warmup,
not a certified cold-machine observation.

### Resource observations

The engine uses a background sampler, primed before warmup. Rounds read its latest
observation without a counter-refresh sleep. The sample includes whole-host CPU
load, available RAM, available temperature information and foreign heavy-process
counts. CPU accounting excludes the benchmark owner and explicitly registered
workers without counting Linux task entries twice. Foreign work remains visible.
Generic `SystemMonitor` callers without that ownership contract retain their
normal global-load behavior.

Local defaults allow 20% CPU load, require 512 MiB available RAM, check 85 °C when
a temperature is available, and allow one heavy foreign process. CI defaults are
more permissive. Confirm tightens CPU load to at most 5%, heavy-process count to
zero, and tolerated flagged observations to zero. Monitoring must stay enabled.
An unknown/stale observation or any flagged round invalidates confirmation;
observations older than one second are stale. A missing temperature sensor is
not itself a flagged round: “clean” means the implemented available checks passed.

Checks at round boundaries do not prove continuous quietness throughout an arm.
Global CPU percentages can miss contention concentrated on one selected CPU and
can flag unrelated activity elsewhere. A cooperative benchmark lock and a scan
for known benchmark process names reduce accidental overlap; they do not detect
all possible workloads. The named-rival scan can wait up to 30 seconds per group.
Resource monitoring has its own CPU/cache cost even outside the timed callback.

## 4. Within-process measurement algorithm

For each selected, nonempty group:

1. Establish and verify placement. Start the group wall clock, perform benchmark
   admission and prime background monitoring.
2. Warm each arm, then estimate its iteration count. Estimation considers timer
   resolution, the default 1 ms sample target, and a 5 ms floor when fewer than
   20 iterations would fit the target. Counts are clamped to the group's limits.
   These are heuristics; they do not impose an exact duration on each sample.
3. Use the lower median of arm estimates as the common group count. Each normal
   round jitters this count by an integer percentage from −20 through +20; every
   arm in that round uses the same count, at least one. Different arm costs mean
   different sample durations even with equal counts.
4. Shuffle arm order using Fisher–Yates and a process/group-specific random seed.
   Each arm runs serially. Keep the realized order and actual iteration count.
5. Record the latest resource observation. Allocate round bookkeeping before
   starting benchmark timers. Apply optional cache spoiling/stack jitter, run
   each callback, and retain both raw and compensated elapsed nanoseconds.
6. Verify placement again, update limits/convergence, and retain the round even
   if its resource observation is flagged. There is no clean-round replacement
   sampling in confirmation.

Iteration estimation itself can affect state. A group containing very different
workloads can create very unequal sample lengths and carryover. Prefer groups
that answer one comparison question. Random order balances order effects in
expectation; short finite schedules need not balance them exactly. The realized
schedule is retained, but there is no user-facing seed/replay guarantee for
identical future timing conditions.

### Timing boundaries and overhead

`Bencher::iter` times a repeated loop and black-boxes returned values; returned
values are dropped inside the timed loop. Input opacity and semantic work remain
the author's responsibility. Inspect optimized machine code when compiler
elimination, specialization, dispatch or vectorization could change the question.

`iter_deferred_drop` preallocates output storage, pushes outputs during timing,
and destroys them afterward. The pushes and changed memory behavior still cost
time. `with_input(...).run(...)` creates each input outside its individual timer
and drops the returned output after that timer; destruction performed inside the
callback remains included. It incurs per-operation timing overhead instead of
one timer pair for the entire batch. The optional thread CPU-time diagnostic for
this path spans a different boundary, including output teardown; it is not an
interchangeable latency measurement.

`bench_parallel` and `bench_contended` create workers before timing and measure
coordinator wall time from release at a start barrier to an end barrier. They
include scheduling/synchronization effects and do not provide instruction-level
simultaneous starts: a worker may begin before the coordinator reads its clock.
Use sufficiently substantial work. Thread creation and later joins are outside
this interval but inside experiment wall time. For an existing Rayon/tokio pool,
use a normal bench callback and wait for all measured work to finish. Nested
zenbench workers plus an external pool change the concurrency experiment.

With `precise-timing`, supported x86_64 uses serialized TSC reads and AArch64 uses
the architectural counter; otherwise the implementation uses `Instant`.
The calibrated counter is converted to nanoseconds. The x86 invariant-counter
check and calibration do not measure instantaneous execution-core frequency.
Calibration seeks agreement between successive counter/Instant ratios and can
fall back to its last estimate. Timer backend and resolution are recorded;
the execution record names the engine's selected backend, while the parallel/
contended coordinator timers described above still use `Instant`. Consequently,
that label is not proof that every callback used the same counter path. Raw
counter values are not interchangeable with retired instructions or
frequency-dependent CPU cycles. TSC_AUX is not retained as a migration trace.

Separate built-in integer, sequential-read and pointer-chase calibration workloads
run at startup unless disabled with `ZENBENCH_NO_CALIBRATE`. They are diagnostic
context, not corrections applied to benchmark times. A 1 MiB read or 4 MiB chase
can exercise cache rather than DRAM on a particular processor; their labels do
not certify a particular hardware bottleneck or normalize unlike machines.

The engine estimates loop overhead using the minimum of 200 empty-loop batches
of 10,000 iterations. It subtracts the resulting per-iteration estimate and clamps
each compensated batch to at least 1 ns. This is a model, not a measurement of
each callback shape's exact overhead. Sub-nanosecond numbers are batch averages;
they are not evidence that an individual operation was resolved below one timer
tick. Raw and compensated samples allow inspection of subtraction and clamping.

Cache spoiling is off by default. Its default 2 MiB buffer is touched at
cache-line strides; that is not a guarantee of complete eviction on modern LLCs.
Optional stack jitter changes a padded call-stack depth before callbacks. It
neither randomizes code/heap layout nor implements STABILIZER. Optional
`linear_sampling` cycles the batch multiplier from 0.2 to 2.0 over ten rounds and
adds a descriptive OLS slope; confirmation still uses process mean latencies.

Allocation profiling requires installing `AllocProfiler` as the allocator.
Counter updates, including a shared relaxed atomic activity flag on successful
allocations, are inside allocation paths and can perturb timings. Thread-local
counts do not account for an opaque pool's allocations or peak live heap.
Confirm native timing separately from allocation, instruction or tracing runs.
Heaptrack and iai-callgrind are external diagnostic workflows, not integrated
confirmation backends.

### Limits and adaptive stopping

Iterate defaults to a 10-second measurement budget and a 120-second group wall
budget. The wall budget includes group admission/warmup; limits are checked
between rounds and cannot interrupt a blocked callback. The measurement budget
applies after the configured minimum rounds, initially five. Expensive callbacks
can therefore exceed a nominal budget.

Adaptive convergence is considered every ten completed rounds, starting at
`max(min_rounds, 30)`. With the default 30-round cap, this cannot save rounds
before that cap. Larger exploratory plans can stop when a streaming normal
approximation considers all baseline comparisons sufficiently resolved and
stable. Its default relative precision is 2%; a clearly directed effect larger
than 10% of baseline additionally uses a half-width/effect criterion below 10%.
A single-arm group uses relative standard error. This stopping heuristic is not
the final bootstrap interval and is not a sequential confidence procedure.

Confirm disables adaptive convergence. A budget ending a child before its fixed
round count makes the attempt incomplete, rather than reducing the planned
sample size. The supervisor enforces its deadline while waiting for children,
including a blocked callback. It does not include Cargo compilation or kill
arbitrarily detached descendants. Use foreground protocol execution for acceptance;
[the workflow guide](docs/benchmark-workflows.md) describes the older daemon wrapper's
wait/exit-code limitations.

## 5. Exploratory summaries

Per-arm descriptive statistics include arithmetic mean, median, minimum, maximum,
sample variance, MAD and a bootstrap interval for the mean. The minimum is the
smallest observed round average under this experiment's conditions. It is not a
known intrinsic latency or a bound valid for another cache, frequency or input
regime. The displayed MAD is the median absolute deviation multiplied by 1.4826,
a normal-consistency scaling; it describes spread, not a confidence interval.

Paired analysis divides each arm's elapsed time by that round's iteration count
and forms candidate-minus-baseline differences. Tukey filtering operates on
these differences, removing the corresponding pair together. The IQR has a 1 ns
floor and fences at 1.5 IQR. Per-arm descriptive rows retain their full sample
populations; the paired diagnostic can therefore summarize a different subset.
Raw records preserve the original data.

The paired bootstrap resamples the cleaned differences, normally 10,000 times,
and reports the 2.5th/50th/97.5th percentiles of their resampled means. Percent
change divides mean difference by the cleaned baseline mean. The `significant`
flag requires the interval outside the configured noise band, normally ±1% of
baseline, and rejects timer-resolution-limited differences. With a zero noise
threshold it instead requires the interval to exclude zero. Wilcoxon signed-rank,
Cohen's d and Spearman drift are diagnostics, not additional acceptance votes.

These intervals do not account for arbitrary round correlation, adaptive stopping,
selection among many edited candidates, or all comparisons across a campaign.
Bootstrap resampling does not repair those design problems. An exploratory
“significant” flag is not a Confirm verdict. Conversely, a wide interval does not
show equivalence. Preserve the distinction in automation and reports.

## 6. Fixed-process confirmation: exact estimand and decision

Let `t[p,r,a]` be compensated elapsed nanoseconds for arm `a`, round `r`, process
`p`, and `k[p,r]` the actual iteration count. Define:

```text
x[p,r,a] = t[p,r,a] / k[p,r]
m[p,a]   = mean over retained rounds r of x[p,r,a]
```

Each round receives equal weight in this process mean; it is not total elapsed
divided by total iterations. Every retained round contributes. There is no
outlier trimming or selection of the fastest process for confirmation.

### Arms in the same executable

For `P` processes with both arms, form `z[p] = ln(m[p,candidate]) − ln(m[p,base])`.
Use the arithmetic mean `mu` of `z`, sample standard deviation `s`, standard error
`s / sqrt(P)` and Student-t degrees of freedom `P−1`. The paired unit is the
process. Within-process interleaving helps each process comparison, but 150 rounds
from five processes do not supply 150 process degrees of freedom.

### Separate baseline and candidate populations

For baseline log means `b[1..n]` and candidate log means `c[1..m]`, use a Welch
interval:

```text
mu = mean(c) − mean(b)
vb = sample_variance(b) / n
vc = sample_variance(c) / m
SE = sqrt(vb + vc)
df = (vb + vc)^2 / (vb^2/(n−1) + vc^2/(m−1))
```

If the degrees-of-freedom denominator is zero, the implementation uses `n+m−2`.
Both populations need at least three valid processes. Overlapping child run IDs
are rejected: reusing the same evidence does not create fresh independent data.
Historical baselines are not randomized contemporaneous A/B blocks.

### Multiplicity, resolution and transformation

For family confidence `C` and `F` comparisons, each interval uses
`alpha = (1−C)/max(F,1)` and the two-sided t quantile at `1−alpha/2`.
Within-suite `F` is the number of reported arm comparisons across the first
child's groups. Default groups use all pairs through three arms and baseline-only
comparisons above three arms, unless explicitly configured. Cross-baseline
comparison uses the matched benchmark cases. This is Bonferroni control for that
family under the model; it is not campaign-wide control over previous edits,
hardware cells or future repeated looks.

The log half-width is the larger of `t_critical × SE` and a timer floor. For each
arm, the floor is the maximum across children of twice recorded timer resolution
divided by the minimum round iteration count and the arm's descriptive mean.
The two arm floors are added. This heuristic prevents a zero-width decision
from identical quantized samples; it is not a complete calibration-error model.

Transform the estimate and bounds to percentage latency change:

```text
estimate = 100 × expm1(mu)
lower    = 100 × expm1(mu − half_width)
upper    = 100 × expm1(mu + half_width)
```

Positive means slower. This geometric ratio differs from a ratio of pooled
arithmetic display means. A percentage latency reduction is also not the same
percentage throughput increase. Nonpositive/nonfinite process means and
unrepresentable intervals cannot support a decision.

### Margin and validity

For a positive practical margin `M`, classification is ordered as follows:

| Interval condition | Verdict |
|---|---|
| `lower > M` | Regression |
| `upper < −M` | Improvement |
| `lower >= −M` and `upper <= M` | Equivalent within the margin |
| `upper <= M` | Non-inferior within the margin |
| Otherwise | Inconclusive |

Before inference, validity checks require complete fixed-round process packets,
matching group identities/settings, workload identity, compatible testbed and CPU
placement, known timer backend, and fresh clean resource observations. Each
population's children must share its executable hash. A baseline and candidate
may have different executable hashes—that is often the intervention. Their
process counts can differ if each plan is valid. Record the candidate decision
policy when comparing rather than assuming an old baseline's policy is reused.

The benchmark macros and evidence comparison return 1 for a confirmed regression
and 2 for invalid/inconclusive confirmation; runtime errors also return nonzero.
A complete single-arm population can initialize a baseline without making a
within-suite relative claim. Updating a baseline requires non-inferiority for
**every** matched case; empty or changed inventories cannot silently promote.
Low-level descriptive save APIs do not themselves certify this contract.

### Statistical assumptions and validation

The process intervals assume independent, approximately normal process log ratios
(or independent log-latency populations for Welch). Fresh processes do not prove
these assumptions. Persistent background work, shared thermal state, layout
bias, or heavy-tailed process means can invalidate nominal coverage. The harness
does not automatically test and repair those distributions. More repetitions
alone do not remove systematic bias.

Independent [SciPy interval fixtures](tests/fixtures/process_intervals.json)
check numerical calculations. A [deterministic simulation](examples/coverage_oracle.py)
uses 2,000 trials, five process log values, standard deviation 0.03 and one
comparison. It records 97 null interval misses and 97 misses of an injected
`ln(1.1)` effect; that effect excludes zero in 1,999 trials. The same generator
with 0.9 serial correlation gives 1,193 null misses; taking repeated looks at
sample sizes 3 through 20 gives 460. [Committed counts](tests/fixtures/process_coverage.json)
are checked by the Rust implementation. These are model controls, not measured
coverage or statistical power on arbitrary application workloads.

## 7. Useful measurements on a busy development machine

A workstation running many coding agents is a legitimate development environment.
It is usually a poor place to demand that every small edit satisfy an idle-host
publication protocol. Requiring global quietness for all feedback makes the
workflow unusable; calling all noisy feedback confirmation makes it misleading.
Keep the intent explicit and bound the time spent obtaining an answer.

The current Iterate path takes background observations and continues through
ordinary load rather than sleeping before every round. It retains flags and
can provide useful directional evidence. It still has cooperative locking and
the named-rival admission wait described above. It does **not** have a bounded,
load-aware multi-agent reservation scheduler. `--no-busy-gate` is an explicit
exploratory escape when those observations/waits are unsuitable; it removes the
observations needed for confirmation. It is not the default recommendation.
CPU-class uncertainty and ordinary background load are separate problems: keep
verified class placement even for noisy iteration whenever available.

Use a small targeted group, short in-process rounds, equivalent work and nearby
A/B observations while editing. Inspect direction, magnitude, drift and spread;
treat unstable or small changes as unresolved. Run A/A controls with the same
paths and state transitions to find asymmetries. A control that looks quiet does
not certify that the candidate responds identically to interference. Repeat
correctness checks and examine generated code or profiler evidence before
spending more measurement time. A quiet confirmation slot is most valuable after
candidate selection, rather than after every intermediate edit.

### What interference does to a paired result

Nearby arms can share slowly changing conditions. If interference affects both
similarly, a paired ratio can remove part of that common variation. A compiler
job, however, can affect memory-bandwidth-heavy code differently from a compute
kernel. An SMT sibling can contend for different execution resources. Unequal
cache footprints can change carryover; AVX use can change the next arm's power
state. Short spikes can hit only one arm. Pairing and randomization reduce some
biases but cannot promise cancellation on a busy host.

Useful next steps for this workload, **not implemented admission guarantees**:

| Intent | Pragmatic behavior to aim for | Honest result |
|---|---|---|
| Edit feedback | Small bounded queue; serialize timing arms; continue with observations under ordinary load | Exploratory effect and noise diagnostics |
| Local acceptance | Reserve a suitable CPU/cache domain where possible; keep interference diagnostics; fixed fresh plan | Current strict Confirm only if its checks pass |
| Publication | Deliberately scheduled cells, controls, provenance, declared exclusions and independent repetitions | Evidence with explicit scope and limitations |

A future admission policy should consider selected CPUs, SMT siblings, shared
LLC/NUMA and memory/power contention instead of treating whole-host load as a
complete certificate. Merely lowering a global threshold wastes idle partitions;
merely raising it misses local interference. Such a policy needs controlled
experiments with foreign work on selected cores, siblings, other cache domains
and memory bandwidth. It must preserve a negative control that detects actual
interference. The current <=5% Confirm threshold remains unchanged.

Likewise, resource-friendly process priority is part of the experimental
configuration, not a portable promise of equal scheduling. Keep both arms'
priority/QoS identical and record it. Platform-dependent interactions between
priority, core selection and scheduling require measurement; do not silently
raise priority to defeat other agents. CPU reservations need cooperation from
other workloads, not just affinity on the benchmark.

An inconclusive result is useful: it says this experiment could not settle the
chosen margin. Keep it and either stop pursuing the edit, diagnose the noise, or
predeclare a new experiment with more suitable resources/budget. Do not discard
unsuccessful attempts and retain only the first favorable rerun. CI completion
and performance acceptance are separate statuses.

## 8. External executables and environment A/B: issue 29

The [external-process implementation design](docs/external-process-design.md)
turns this review into a delivery scope and acceptance gates. It is a proposal,
not an available command.

[Issue 29](https://github.com/imazen/zenbench/issues/29) requests a serial paired
runner for prebuilt executables with per-arm environments, internal metrics,
correctness controls and resumable case grids. The same executable with different
environment settings is a special case of that design. It matters when dispatch,
pools, allocators or library configuration are initialized once per process.
Environment changes can also change stack/data layout. Using identical executable
bytes removes a build difference, but does not prove the only effect is the
intended runtime switch. Controls should exercise the same launch shape and
record that limitation.

**This runner is not implemented.** Current fresh-process confirmation reruns one
zenbench executable/configuration. `self-compare` builds two revisions and runs
them sequentially. Saved packets can be compared with Welch inference, but there
is no automatic randomized external A/B schedule, per-arm environment manifest,
metric extractor or paired-block inference for these invocations.

The issue's concrete use cases support the following design requirements:

- An arm consists of immutable executable identity, argv, cwd, explicit environment
  set/unset operations, timeout, placement and declared metric units/boundary.
  Compare the same binary under two environments without mutating the parent
  process environment. Record shared configuration and the intentional difference;
  avoid dumping secrets from the inherited environment into published artifacts.
- Run arms serially in randomized nearby blocks. Record order, timestamps,
  resource observations, settling policy and all failures. Two simultaneous
  timing processes would create another contention experiment.
- Retain every metric returned by an invocation, but preserve invocation and block
  boundaries. Three internal passes from one child are not three independent
  process repetitions. Choose the within-invocation summary and outer block
  estimator before running; do not flatten vectors into a bootstrap population.
- Check exit status, metric cardinality, units and finite positive values. Record
  externally measured wall time separately from self-reported internal time.
  A regex extracts a value; it does not prove that C and Rust binaries timed the
  same work or excluded the same warmup.
- Include A/A as a diagnostic control, preferably near the A/B observations, with
  uncertainty and order information. `|AB−1| > |AA−1|` can be a named exploratory
  heuristic; it is not a calibrated significance test or a sufficient acceptance
  rule. A/A is itself noisy and may miss candidate-specific interference. Do not
  subtract its observed bias automatically or use a lucky A/A to certify a run.
- Correctness failure must remain visible as invalid performance evidence for
  that contract. Retain measured values, artifacts and the failure; exclude the
  cell from a “correct and faster” conclusion. A missing sample must not silently
  reduce the planned repetitions.
- Resume only against a matching experiment fingerprint: binaries, inputs, arm
  environments, metric/check contracts, schedule and decision policy. Keep every
  attempt and distinguish a later time block. Resumption across days introduces
  another source of variation that an interval must account for.

For busy systems, interleaving is worth preserving at the **process** level as
well as inside a process. The external runner should offer bounded exploratory
feedback without pretending global load is harmless. Confirmation needs its own
validated block-level analysis and contamination policy. Reusing the existing
presentation/storage components is appropriate; reusing an in-process bootstrap
without respecting process clusters is not sufficient.

The source review also shows that issue 29 describes a desired common protocol,
not identical existing implementations. The inspected rav1d `ab_bench.py` alternates
AB/BA deterministically and flattens returned passes into arm arrays. The inspected
SVT `perf_ab.sh` randomizes per-round order and retains paired ratios, but its
shell loop can omit missing metrics. These scripts provide concrete migration
cases and failure controls, not a statistical oracle to copy verbatim. Their
source identities are recorded under References. Optional size-axis fitting
should remain separate from acceptance: a fitted intercept is not proof of
fixed overhead without a suitable model, residual checks and input design.

## 9. Evidence, provenance and small version-control diffs

Raw data lives under `.zenbench/data/` or `ZENBENCH_DATA_DIR`, outside replaceable
Cargo build output. Compressed content-addressed objects retain complete round
and process boundaries. Small references locate objects and verify their hashes.
Incomplete attempts, child packets and lifecycle manifests survive later failure.
Publication writes the object before exposing a reference; corrupt or oversized
packets fail loading instead of silently losing data. The encoded/decoded packet
limit is 256 MiB; split larger studies into explicit suites/cells.

Select stable CSV snapshots for Git rather than adding every run. A snapshot's
size follows the benchmark inventory rather than round count. It includes context,
evidence identity and commands for reading/comparing full packets. Gzip can be
decoded independently; references and snapshots carry command argument arrays.
A compact summary is not a replacement for the raw evidence or a valid input to
process inference. Keep full objects in durable artifact storage with a backup
and access plan. Zenbench does not upload, schedule a fleet, or garbage-collect
unsuccessful runs.

```sh
zenbench read candidate.json.gz --format llm
zenbench read candidate.json.gz --format json
zenbench compare baseline.json.gz candidate.json.gz
zenbench snapshot candidate.json.gz --output benchmarks/baseline.csv
zenbench reanalyze candidate.json.gz --output analysis.json
```

Reanalysis writes a separate versioned decision artifact linked to the immutable
evidence hash. It cannot change exploratory evidence into a fixed experiment.
Named baseline updates preserve prior references. [Storage details](docs/result-storage.md)
cover portability, retained attempts and historical format support.

Executable SHA-256 identifies bytes run. Runtime Git HEAD identifies the current
checkout, which can differ from the source used to build a retained executable.
Automatic clean/dirty capture and a complete build manifest are not implemented.
For defensible evidence, retain compiler/toolchain, target, profile, flags, features,
lockfile/dependency identities, source revision and dirty patch/source snapshot,
input hashes, environment interventions, pool sizes and allocator/profiler choices.
Bind that manifest to the executable hash. Record power/NUMA/priority configuration
when it matters; the current packet does not certify all of it.

For a many-crate/many-machine study, define cells and scope first, compare like
with like, and export selected stable summaries per scope. Do not average unlike
hardware into a single speed claim by accident. Specify any cross-cell weighting
and multiplicity policy separately. Reuse saved evidence for views and analysis;
do not rerun an expensive experiment just to change its output format.

## 10. Measured wall time and storage cost

These measurements are from 2026-10-11 on a Linux Core Ultra 7 265K, inside Docker
with a hard CPU-0 cpuset and a 2 GiB memory limit, nice 19, release/default features,
and no custom RUSTFLAGS. The recorded toolchain was Rust 1.99.0/LLVM 23.1.1; the
host used intel_pstate, powersave and balance_performance. Frequency was not
measured or fixed. Driver wall time covers the executable invocation and its
output, excluding compilation and container startup. All numbers below come from
retained records; no multi-host calendar-time extrapolation is implied.

### A. Removing the per-round monitor stall

This fixture has two identical folds over 1,024 u64 values, 200 fixed rounds,
and the original 500 ms warmup per arm.

| Revision/monitor | Invocations | Wall seconds, median | Observed range |
|---|---:|---:|---:|
| Before, monitor enabled | 1 | 41.228751 | Single observation |
| Before, monitor disabled | 1 | 1.565484 | Single observation |
| After, monitor enabled | 3 | 1.967261 | 1.766872–1.967650 |
| After, monitor disabled | 3 | 1.566121 | 1.565478–1.566181 |

The before-enabled observation divided by the after-enabled median is 20.957440.
This descriptive ratio is not a confidence interval from a randomized revision
comparison. The equal-round ablation and source change identify a blocking
counter refresh as the avoidable cost. All 600 after-enabled rounds retained fresh
clean resource observations. Disabling monitoring was a diagnostic ablation;
it is not an acceptable Confirm configuration.

Sources: [before timings](benchmarks/defaults_cost_2026-10-11.summary.json),
[before ablation](benchmarks/defaults_gate_ablation_2026-10-11.summary.json),
[after timings](benchmarks/monitor_fixed_2026-10-11.summary.json),
[before provenance](benchmarks/defaults_audit_2026-10-11.meta.json),
[after provenance](benchmarks/monitor_fixed_2026-10-11.meta.json),
[before archive](benchmarks/defaults_audit_2026-10-11.pointer.md),
[after archive](benchmarks/monitor_fixed_2026-10-11.pointer.md).
Source revisions were `98100750257a6fb3e5dc2c440cd143ae4281d7f9` and
`a6179274d20420354c6f20571ddb9107feb5d023`.

### B. New mode cost on a separate control

The final protocol fixture compares one fold with deliberately repeated work.
Default warmup is 100 ms per arm. Each mode below was invoked once.

| Mode | Processes × rounds | Wall seconds | Outcome |
|---|---:|---:|---|
| Iterate | 1 × 30 | 0.765767 | Execution succeeded; exploratory |
| Confirm | 5 × 30 | 3.827814 | Known regression detected, exit 1 |
| Evidence | 5 × 30 | 3.875655 | Known regression detected, exit 1 |

Confirm added 3.062047 seconds and cost 4.998666 times Iterate in this observation.
Evidence added 3.109888 seconds and cost 5.061141 times Iterate. Those ratios are
not default promises for long operations, many arms, external programs or busy
machines. The old default's 41.218343-second median belongs to the identical-arm
fixture and different warmup/round defaults; comparing it directly to 0.765767
would conflate monitor, defaults and workload changes.

One preceding Evidence attempt was refused after a child's observations flagged
all 30 rounds; it remains archived. The successful control run does not erase
that attempt. A one-second deadline control terminated after 1.017605 seconds
with exit 2. No whole-study or compilation-speed claim follows from these controls.

Source: [times and storage](benchmarks/modes_2026-10-11.summary.json),
[provenance](benchmarks/modes_2026-10-11.meta.json),
[all retained attempts and binary](benchmarks/modes_2026-10-11.pointer.md),
revision `0acfd44aaa1d88c91a3bc37ab985c16fd58608ba`.

### C. Storage measurements at the mode revision

| Packet | Pretty JSON bytes | Gzip bytes | Selected CSV bytes | Reanalysis bytes |
|---|---:|---:|---:|---:|
| Iterate | 24,426 | 3,003 | 826 | 245 |
| Confirm | 146,109 | 9,729 | 874 | 532 |
| Evidence | 146,069 | 9,668 | 895 | 532 |

These historical sizes precede the later embedded read/decode/compare commands;
current sizes also depend on paths and metadata. They demonstrate the retained
fixture's storage choices, not a universal compression ratio or a current byte
budget. Independent replay checked the full round trip and reconstructed process
intervals. A small CSV without its raw object cannot reproduce the inference.

### Resource accounting and planning calendar time

Recorded wrapper lines for the cited runs:

```text
original audit: run-heavy: done rc=0 2972s | peak-RSS 0.03GiB | min-avail 26737MiB | peak-load 0.75
old ablation:  run-heavy: done rc=0 18s | peak-RSS 0.03GiB | min-avail 26833MiB | peak-load 0.43
monitor fix:   run-heavy: done rc=0 10s | peak-RSS 0.03GiB | min-avail 26797MiB | peak-load 0.36
mode control:  run-heavy: done rc=0 11s | peak-RSS 0.10GiB | min-avail 26797MiB | peak-load 1.22
```

RSS here covers the Docker client (and Python replay for the mode control), not
a measured payload peak. The separate 2 GiB container limit is a cap, not usage.
The original audit wrapper covered a larger campaign than the selected table rows.

Experiment wall time includes admission, process launch, warmup, estimation,
sampling, monitor lifecycle, analysis and persistence. Summing timed batches alone
misses most of these on small workloads. The supervisor records aggregate elapsed
time rather than presenting one child's duration as the multi-process cost.
An individual engine's `total_time` starts after startup calibration and excludes
some final output/persistence work; use an external invocation clock for total
user-visible cost, as the retained audit drivers do.

To budget a real campaign, measure representative cells at their actual settings,
record compilation separately, and retain queue/wait and failed-attempt time.
Give exploration a bounded budget and schedule confirmation after candidate
selection. Additional processes are useful when between-process variation matters;
more rounds in one process cannot replace them. More repetitions also cannot fix
biased placement or unequal work. There is no measured “dozen crates on a dozen
computers” completion time in this record, and multiplying this fold control
would not establish one.

## 11. Threats to validity and what remains external

| Threat | Current mitigation | Remaining responsibility/limitation |
|---|---|---|
| Incorrect or unequal work | Named workloads, retained inventory, correctness guidance | Arbitrary closures have no automatic output oracle |
| CPU/cache asymmetry | Topology admission, process boundary, scoped thread masks | Unsupported targets refuse; capacity is not complete domain identity |
| Frequency and thermal history | Warmup, interleaving, available temperature observations | No proof of maximum clock or steady state; power limits and AVX history persist |
| Other agents, IRQs, SMT and shared memory | Background observations, cooperative serialization, retained flags | No exclusive reservation or complete per-core interference detector |
| Code/stack/heap layout | Fresh processes and optional stack jitter | One build is not a randomized layout population |
| Optimizer elimination and specialization | Black-box output, assembly/differential workflow | Inputs, dispatch boundary, flags and semantic work need review |
| Timer and harness perturbation | Batching, calibration, raw/compensated data, resolution floor | Overhead subtraction and calibration are approximate |
| Temporal correlation and multiple looks | Fixed confirmation plan; process-level unit | Independence/log-normality not guaranteed; no sequential inference |
| Candidate/corpus selection | Separate exploration and fresh confirmation; workload IDs | No automatic holdout design or study-wide multiplicity control |
| Historical baseline drift | Testbed/config checks, independent-population interval | No contemporaneous randomized two-binary runner yet |
| Missing/selected evidence | Complete attempts, validity checks, immutable objects | External artifact availability and disclosure remain necessary |
| Cross-machine generalization | Explicit scope and per-cell records | No automatic fleet matrix or population-weighted inference |

For a paper or release report, publish the question and scope, correctness
contract, exact builds/inputs, placement/power context, timing boundary, complete
sampling plan, estimand, interval/margin, exclusions and unsuccessful attempts,
raw-data locator, decoding/reanalysis commands, and measured experiment cost.
Explain what a reader can reproduce and what was not measured. A narrow interval
and a green exit code cannot supply missing experimental design.

## 12. Research context and references

Mytkowicz, Diwan, Hauswirth and Sweeney, *Producing Wrong Data Without Doing Anything
Obviously Wrong!* (ASPLOS 2009), demonstrate that apparently irrelevant setup
choices can bias conclusions. The implication here is to treat layout/environment
as experimental factors, rather than assume paired timing cancels them.
[Author publication page](https://sape.inf.usi.ch/publications/asplos09.html),
[DOI](https://doi.org/10.1145/1508244.1508275).

Curtsinger and Berger, *STABILIZER: Statistically Sound Performance Evaluation*
(ASPLOS 2013), repeatedly randomize code, stack and heap layouts to address layout
bias. Zenbench does not implement that mechanism. Neither 30 rounds nor fresh
execution alone establishes its distributional guarantees.
[Author manuscript](https://people.cs.umass.edu/~emery/pubs/stabilizer-asplos13.pdf).

Chen and Revels, *Robust Benchmarking in Noisy Environments* (HPEC 2016), discuss
timer resolution, batching and non-ideal timing distributions. Their model
motivates examining low observations, but does not justify treating every
benchmark's minimum as universal true performance. Zenbench retains minima as
diagnostics and uses a different process-mean estimand for confirmation.
[Paper](https://arxiv.org/abs/1608.04295),
[full text](https://arxiv.org/html/1608.04295v1).

Kalibera and Jones, *Rigorous Benchmarking in Reasonable Time* (ISMM 2013), separate
variation at iteration, execution and build levels and allocate repetition where
uncertainty arises. Zenbench's fixed process protocol adopts the distinction
between levels; it does not implement their full experimental-budget optimizer
or reproduce their estimator.
[Author repository and corrected manuscript](https://kar.kent.ac.uk/33611/).

Primary operating-system documentation relevant to containment:
[Linux cpuset controller](https://docs.kernel.org/admin-guide/cgroup-v2.html#cpuset),
[Windows Job Objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects),
[Apple affinity API](https://developer.apple.com/library/archive/releasenotes/Performance/RN-AffinityAPI/).

Issue 29 source review, observed 2026-10-12 (Git blob identities; resolve the paths
again before relying on a later revision):

- [rav1d-safe ab_bench.py](https://github.com/imazen/rav1d-safe/blob/7e48ed12501f208b5d4b155878c26a761f11b88e/scripts/perf/decoder_bench/ab_bench.py),
  blob `33383d7ac457de6f23ca2fc6af35ef08d7f18745`.
- [zenav1-svt perf_ab.sh](https://github.com/imazen/zenav1-svt/blob/bed839749c786ba35d751e18e2c382a65c2f8bc8/rust/tools/perf_ab.sh),
  blob `04f9242865ba9b0e99dfccb3a4bc73ab086a863a`.

## Appendix: implementation and validation map

| Subject | Source/retained check |
|---|---|
| Plan defaults, validation, precedence | [plan.rs](src/plan.rs), [mode guide](docs/benchmark-modes.md) |
| Supervisor, aggregation, macro exits | [lib.rs](src/lib.rs) |
| Admission, warmup, batching, schedules, stopping | [engine.rs](src/engine.rs) |
| Timing boundaries and threading APIs | [bench.rs](src/bench.rs), [timing.rs](src/timing.rs) |
| Descriptive summaries, trimming and bootstrap | [stats.rs](src/stats.rs) |
| Process estimand, intervals, compatibility and verdicts | [inference.rs](src/inference.rs), [baseline.rs](src/baseline.rs) |
| Resource counters, ownership and freshness | [platform.rs](src/platform.rs), [monitor.rs](src/monitor.rs), [gate.rs](src/gate.rs) |
| CPU controls and independent native observations | [placement documentation](docs/process-affinity.md) |
| Storage, integrity, export and reanalysis | [storage.rs](src/storage.rs), [storage guide](docs/result-storage.md) |
| Numeric estimator oracle | [SciPy generator](examples/inference_oracle.py), [interval fixtures](tests/fixtures/process_intervals.json) |
| Coverage and excluded scenarios | [simulation generator](examples/coverage_oracle.py), [counts](tests/fixtures/process_coverage.json) |
| Historical wall-time experiment | [audit driver](examples/audit_defaults.py), [defaults audit](docs/defaults-audit.md) |

This document describes the audited implementation. Proposals in sections 7 and
8 need their own implementation, independent controls and measured evaluation;
they are not guarantees provided by the current CLI.

## Earlier reading list

These links preserve the earlier methodology's reading list. They are background
references, not claims that Zenbench implements those tools or their guarantees.

- [Producing Wrong Data Without Doing Anything Obviously Wrong](https://dl.acm.org/doi/10.1145/1508284.1508275)
- [What Metric to Use When Benchmarking?](https://tratt.net/laurie/blog/2022/what_metric_to_use_when_benchmarking.html)
- [Minimum Times Tend to Mislead When Benchmarking](https://tratt.net/laurie/blog/2019/minimum_times_tend_to_mislead_when_benchmarking.html)
- [nanobench documentation](https://nanobench.ankerl.com/reference.html)
- [Criterion.rs Analysis Process](https://bheisler.github.io/criterion.rs/book/analysis.html)
- [Divan announcement and design philosophy](https://nikolaivazquez.com/blog/divan/)
- [tango-bench: paired benchmarking via dylib loading](https://github.com/bazhenov/tango)
- [Google Benchmark User Guide](https://google.github.io/benchmark/user_guide.html)
- [Apache Otava: Change Point Detection for Performance Regressions](https://otava.apache.org/docs/math/)
- [CodSpeed: Why glibc is faster on some GitHub Actions Runners](https://codspeed.io/blog/unrelated-benchmark-regression)
- [iai-callgrind: Hardware-agnostic Rust benchmarking via Cachegrind](https://github.com/iai-callgrind/iai-callgrind)
- [Bencher: Continuous Benchmarking](https://bencher.dev/docs/explanation/continuous-benchmarking/)
- [Aakinshin: Performance Stability of GitHub Actions](https://aakinshin.net/posts/github-actions-perf-stability/)
- [rustc-perf: Instruction counts as primary metric](https://kobzol.github.io/rust/rustc/2023/09/23/rustc-runtime-benchmarks.html)
