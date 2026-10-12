# Defaults, runtime cost, and retained evidence

Audit date: 2026-10-11. Implementation examined: `98100750` (harness behavior
unchanged from `37033b7b`). This document distinguishes the current implementation,
measured diagnostic results, and proposed behavior. The proposed modes are not
implemented presets at the audited revision. The subsequent implementation is
documented in [benchmark modes](benchmark-modes.md) and
[result storage](result-storage.md); the historical findings below retain their
original revision scope.

## Findings that affect the design

1. **Repetition is not yet confirmation.** `aggregate_results` replaces benchmark
   summaries but retains the first run's `ComparisonResult::analyses`. Its total
   time, run ID, round count, calibration and gate accounting also come from that
   first run. The audit's synthetic counterexample has effects of -20% and +20%:
   aggregate baseline and candidate means are both 100, but the retained analysis
   says -20%, significant. `Best` can select different processes for different
   arms; none of these aggregate policies preserves the original paired
   population. They cannot implement a confirm preset by themselves.
2. **Monitoring changes the schedule substantially.** Every enabled per-round
   resource check calls `SystemMonitor::snapshot`, which can sleep until a
   200 ms CPU-counter interval has elapsed. This delay is not charged to
   `gate_wait_time`. The group measurement budget excludes it, while the wall
   limit includes it. `max_time=10s` therefore does not bound elapsed time to 10s.
   This is relevant to CPU demand and warmup as well as calendar time.
3. **Default admission is advisory.** Thread-tagged groups disable the gate.
   The default and CI gate configurations are not strict. Rival-process waiting
   gives up after its limit and continues. Failure to obtain the exclusive lock
   also permits execution. A strict preset needs an explicit result-validity
   contract, rather than merely changing a configuration constructor.
   CPU load is averaged over the machine's reported CPUs, not the selected set;
   process visibility can also be restricted by a container namespace. Admission
   must distinguish whole-host and selected-CPU observations and their coverage.
4. **Intervals are conditional on one process and the current sampling rules.**
   Auto-rounds repeatedly checks approximate normal intervals; final analyses
   use a bootstrap after paired-difference outlier removal. This audit does not
   establish coverage under adaptive stopping, autocorrelation, or trimming.
   A narrow interval is not an equivalence test: equivalence requires the entire
   interval to lie inside a declared practical margin. A non-significant result
   is not proof that implementations are equivalent.
   The current `significant` flag also includes a default 1% practical margin;
   it must not be interpreted as a standalone test of nominal 95% coverage.
5. **Baseline and CLI diffs answer different questions.** The baseline helper
   uses summary means, variances and counts with a fixed `t > 2` rule; fewer than
   two observations bypass statistical gating. It does not reject unreliable
   inputs. The CLI `compare` prints percentage differences and applies colors
   beyond 1%; it does not perform that statistical test. `--update-on-pass`
   promotes a result when no regression is detected, which is weaker than
   establishing non-inferiority. Missing benchmarks do not prevent that promotion,
   so a filtered/empty result can replace a fuller baseline. There is no common
   versioned analysis policy.
6. **Placement evidence is useful but is not full experiment identity.** CPU
   compatibility compares class, effective set, enforcement, topology kind and
   coordinator masks. It does not compare the complete hardware/build/workload
   identity. Testbed metadata contains CPU model, OS, architecture and core
   counts. Source-tree dirtiness, compiler flags, feature set, input identity,
   worker count and binary digest need separate recording. Core confinement
   does not establish quietness, exclusive caches, fixed clocks or NUMA locality.
7. **Current storage loses information needed for reanalysis.** Ordinary
   auto-save retains an LLM text report under `target/zenbench/results`; it omits
   round samples. Explicit JSON saving preserves rounds for ordinary groups.
   Multi-process exchange JSON is deleted after loading, including on failure;
   the aggregate clears rounds. Named baselines overwrite
   `.zenbench/baselines/<name>.json`. `SuiteResult::save` is a direct write, not an
   atomic publication. Some automatic save errors are ignored or only printed.
   `cargo clean` can remove the default auto-saved reports.
   Run IDs contain epoch seconds and PID, so they are not globally unique across
   hosts/containers, or even across multiple fast engine calls in one process.
8. **Configuration needs an explicit resolved contract.** Bench macros scan
   selected arguments rather than rejecting every unknown option; a proposed
   `--mode` flag is not implemented and must not be assumed to work. Suite CPU
   settings currently override CLI/environment choices. That can prevent a fleet
   profile from controlling a crate's hard-coded settings, even though placement
   mismatches still fail admission. Record effective settings and their origins,
   and distinguish caller-overridable defaults from hard workload constraints.

Source owners: [engine](../src/engine.rs), [gate](../src/gate.rs),
[monitor](../src/platform.rs), [aggregation and postprocessing](../src/lib.rs),
[baseline comparison](../src/baseline.rs), [CLI comparison](../src/bin/zenbench.rs),
[result schema](../src/results.rs), [paths](../src/paths.rs).

The [independent Windows recount](../benchmarks/windows_evidence_reaudit_2026-10-11.json)
verified the compressed archive hashes and revisited every successful process.
The original totals hold: 40 successful processes, 20 refusals, 24,000 retained
rounds, 240 positive controls detected, and no practical-significance null
verdicts. Fourteen raw null intervals exclude zero. All 24,000 rounds have
`gate_clean=null`; this evidence does not exercise the default resource monitor.
The process/worker mask readbacks also agree with the selected sets. This is a
reanalysis of retained Windows evidence, not a new Windows timing run. The
[native verification document](cpu-placement-verification.md) already records
the ARM/group/macOS and 9950X3D measurement limits; they still apply.

## Measured runtime and storage

Optimized default-feature binary on a Core Ultra 7 265K, Linux, Docker cpuset
`[0]`, nice 19, separate 2 GiB container memory cap. The usual case has one group
and two identical fold arms over 1024 u64 words. Compilation and container launch
are outside the per-invocation wall measurements. The CPU reported `intel_pstate`,
`powersave`, and `balance_performance`; no governor setting was changed. This is
a harness-cost diagnostic on one host, not a timing prediction for other crates,
machines, or the unimplemented modes.

| Current-engine experiment | Observed wall time | Repetitions / rounds |
|---|---:|---|
| Empty suite, startup/calibration | 0.064 s median | 3 invocations |
| Default, two arms | 41.218 s median | 3 invocations, 200 rounds each |
| Three fresh processes | 123.592 s median | 2 invocations, 3 × 200 rounds each |
| Five fresh processes | 205.913 s | 1 invocation, 5 × 200 rounds |
| Twelve sequential single-process invocations | 494.611 s total | 12 × 200 rounds |
| Twelve sequential three-process invocations | 1483.371 s total | 36 × 200 rounds |
| Six arms instead of two | 43.192 s median | 2 invocations, 200 rounds each |
| 100 ms warmup per arm instead of 500 ms | 40.401 s median | 3 invocations, 200 rounds each |
| Gate disabled, adaptive rounds | 1.165 s median | 3 invocations, 30 rounds each |

The twelve-invocation comparison measures 8m 14.611s versus 24m 43.371s: an
additional 16m 28.759s, a 2.999× total. These are twelve invocations of the
diagnostic, not measurements of twelve distinct crates. The single-group tests
at 16 and 65,536 words also exhausted 200 rounds and took medians of 41.199s and
41.239s. They do not cover arbitrary slow callbacks, real SIMD workloads, or
large multi-group suites.

Equal-round ablations separate gate scheduling from adaptive convergence:

| Rounds | Gate enabled | Gate disabled |
|---|---:|---:|
| 30, alternating control A | 6.987 s | 1.165 s |
| 30, alternating control B | 6.986 s | 1.166 s |
| 200 | 41.229 s | 1.565 s |

With 30 rounds and the gate enabled, the first/second execution positions averaged
772/475 ns in control A and 777/444 ns in control B, despite identical work.
Disabled controls averaged 372/371 ns and 373/371 ns. This identifies a schedule-
dependent measurement effect in this fixture. CPU frequency was not measured
through the rounds, so the data does not distinguish frequency changes from
cache or other state effects. Gate-disabled runs are diagnostic ablations, not
verified replacements for resource monitoring.

Three-process outputs report about 41 seconds in `total_time`, rather than their
measured 123.6-second duration, and retain an analysis with 200 samples beside a
summary with three process means. The [synthetic counterexample](../benchmarks/defaults_aggregation_counterexample_2026-10-11.json)
demonstrates why that retained analysis cannot support the aggregate conclusion.

| Retained measurement population | Pretty JSON bytes | Gzip bytes |
|---|---:|---:|
| Twelve single-process results | 834,043 | 79,377 |
| All 36 child results from the three-process batch | 2,501,870 | 238,519 |
| The twelve aggregate summaries, separately | 69,463 | 20,429 |

Compression here is measured gzip, not a prediction for the proposed zstd layout.
The first ordinary 200-round result used 69,511 bytes as JSON, 6,617 with gzip,
and 1,408 in its auto-saved text report. The text report omits the round data.
Preserving all child measurements is inexpensive in this fixture; choosing
summary-only storage sacrifices reanalysis for a small saving. Binary storage
is separate and should deduplicate by executable hash.

Reproduction: `just defaults-audit-build`, `just defaults-audit-image`, then
`just defaults-audit-run <new-output-directory> full 0` and the `ablation` plan.
Run heavy commands through the resource wrapper. The audit-only child archive
preserves the JSON that the production runner deletes; its write cost is included
in these elapsed measurements. Full-plan and ablation container image IDs are
recorded separately in the [manifest](../benchmarks/defaults_audit_2026-10-11.meta.json).

The [cost summary](../benchmarks/defaults_cost_2026-10-11.summary.json),
[ablation summary](../benchmarks/defaults_gate_ablation_2026-10-11.summary.json),
and [archived raw evidence](../benchmarks/defaults_audit_2026-10-11.pointer.md)
retain the measurements and executable. All timing results here concern the
audited implementation, not a repaired monitor or process-aware estimator.

Resource wrapper: `run-heavy: done rc=0 2972s | peak-RSS 0.03GiB |
min-avail 26737MiB | peak-load 0.75`. This RSS covers the Docker client, not
benchmark payload RSS. The payload had its own 2 GiB hard cap. The ablation
wrapper completed in 18s with peak-RSS 0.03GiB, min-avail 26833MiB, peak-load 0.43.

## Revised modes

The previous proposal of 2 seconds / 30 seconds / 5 minutes per comparison was
an unvalidated spending policy, not a runtime estimate or an accuracy guarantee.
A group shares sampling across its arms, so charging independently for every
pair also misrepresents cost. Budget by selected group and process replication,
with a total command deadline. Report build, setup, warmup, sampling, monitoring,
queueing and analysis separately. A callback cannot generally be preempted
safely; a supervising process is needed for a hard deadline, with partial
artifacts preserved.

| Mode | Intended decision | Scope and repetition | Permitted conclusion |
|---|---|---|---|
| Iterate | Choose what to investigate next | Selected groups and class, one fresh process, bounded work | Exploratory estimate; no confirmed regression or baseline promotion |
| Confirm | Accept one frozen candidate | Independent baseline/candidate repetitions under a recorded plan | Improvement, regression, equivalence/non-inferiority, or inconclusive within declared scope |
| Evidence | Support a release or external claim | Confirm protocol across an explicit workload/hardware matrix; controls and complete provenance | Claims restricted to the completed matrix and achieved uncertainty |

For an optimization-focused tool, the proposed local default is **Iterate** with
strict placement admission and unmistakable exploratory status. CI and baseline
promotion explicitly require **Confirm**. Evidence is a workflow selecting scope
and controls, not a promise obtained by multiplying a timeout. These are changes
to the earlier recommendation that Confirm should always be the local default.

CPU scope should be orthogonal to rigor. Reuse a declared per-host CPU-class and
thread-count profile. When mixed hardware has no selected profile, require a
choice or an explicit all-classes experiment; do not guess the faster cache
class. An all-classes fleet matrix is valuable, but need not rerun for each local
edit. Whole-machine mixed-class throughput is a different, explicitly labeled
experiment. Unknown/unsupported enforcement remains unverified and cannot be
promoted into a verified placement claim.

Classification and containment also need distinct states. A verified singleton
CPU domain can support a comparison on that CPU even when its P/E ranking is
unknown; its label should say so. The current resolver requires membership in a
known class. Consider allowing an explicitly selected singleton once the platform
can prove its CPU/group identity and confinement. This would not establish a
performance-core claim, make a multi-CPU unknown set homogeneous, or supply the
multi-group topology backend. Native Windows ARM64 single-group discovery was
subsequently implemented; see [placement verification](cpu-placement-verification.md).

Keep boundary checks and correctness validation in every mode. Validate workload
outputs outside timing; the harness cannot infer arbitrary closure semantics.
Warm the actual workload and workers. A fixed warmup duration or a diagnostic
CPU exercise cannot certify maximum frequency. Observe demand stability without
assuming it; retain uncertainty about opaque pools and sampling gaps.

Replace blocking per-round CPU snapshots with a monitor whose sampling schedule
does not insert long sleeps between arms/rounds. Take a valid initial sample,
retain sample timestamps/age and overlapping resource observations, account for
owned workers, and leave monitoring enabled for threaded groups. Missing or stale
observations are unknown, not clean. Test this against foreign-load controls;
simply disabling the gate is an ablation, not the proposed fix.

For confirmation, freeze the candidate and workload after exploratory selection.
Use a pilot to allocate a fixed independent confirmation experiment, or implement
and validate a sequential method with explicit assumptions. Pair within-process
arms by actual round; pair separate binaries by randomized execution blocks.
Preserve process boundaries when estimating uncertainty. Do not pretend thousands
of rounds inside one process are thousands of independent program launches.
Do not use best-of-processes for a general speedup claim. Retain all attempts,
including noise failures, under a declared bounded retry policy.

Declared practical margins, confidence level, multiple-comparison policy and
analysis version belong in the experiment plan. An inconclusive CI comparison
must not silently update its baseline. Distinguish inconclusive inference from
invalid placement or missing artifacts, both in output and exit status. Quick
results may be stored and descriptively diffed; they remain exploratory in every
format, including machine-readable rows.
Reject unknown mode/options, empty selections and missing required benchmark
cases in confirmation. Prefer CLI over environment over project/suite defaults;
explicit workload constraints should produce a configuration error when they
conflict. Avoid silently changing rigor just because a CI environment was
detected. The CI recipe should select its required mode explicitly.

This design follows the distinction between repetition levels in
[Kalibera and Jones](https://kar.kent.ac.uk/33611/). Adaptive stopping needs an
appropriate inference procedure; [Howard et al.](https://arxiv.org/abs/1810.08240)
give a foundation, not an automatic coverage guarantee for this harness.

## Storage and diff contract

Use an explicit project data root, proposed default `.zenbench/`, outside Cargo's
disposable build output. Allow a shared external data root for fleet runs. Keep
build outputs, process exchange and durable measurements in distinct directories.
The local index is rebuildable; the measurement objects are authoritative.

```
.zenbench/
  runs/<experiment-id>/manifest.json
  runs/<experiment-id>/attempts/<attempt-id>/metadata.json
  runs/<experiment-id>/attempts/<attempt-id>/rounds.jsonl.zst
  runs/<experiment-id>/attempts/<attempt-id>/stderr.txt
  analyses/<analysis-id>/summary.json
  analyses/<analysis-id>/report.txt
  refs/<baseline-name>/<compatibility-key>.json
```

The layout and compression are proposed, not existing commands. Each experiment
manifest identifies the plan and every child attempt. Each attempt records
uncompensated and compensated batch times, actual iteration counts, arm order,
resource evidence and lifecycle status. Write append-only chunks, hash them, and
atomically publish the completed manifest. A killed or failed run remains a
partial attempt. A save failure prevents a successful evidence verdict.
Use collision-resistant experiment/attempt IDs and create-new publication;
measurement hashes provide content identity, independently of human names.

Record source commit plus dirty-tree digest, binary digest, lockfile/toolchain/
build flags, harness version, benchmark/input IDs, dispatch tier, cache mode,
CPU/cache/NUMA topology, exact placement, SMT and worker counts, timer/calibration,
OS/kernel and relevant power settings. An unavailable field stays unknown.
Record the actual timer backend and conversion scale, not just the compiled
feature. Keep raw counter deltas when available. Runtime Git observations must
not be substituted for the identity of the code used to build the executable.
Avoid secrets and environment dumps; use an allowlist of relevant settings.

Analysis objects reference immutable measurement hashes, estimator version and
policy. Reanalysis and report rendering must not re-run benchmarks. A named
baseline is a versioned reference to selected evidence, not an overwritten copy
of its measurements. Promotion requires compatible, sufficiently informative
confirmation; it is never triggered merely by failure to detect a regression.

Diff joins benchmark/workload IDs and checks the experiment contract before
computing deltas. Allow the intended code/build change, while explicitly recording
it; require matching workload semantics and measurement scope. Distinguish:

- same-process paired arms;
- independent baseline/candidate process blocks;
- historical observational trends;
- cross-machine descriptive comparisons.

Use one analysis implementation for terminal, CLI diff, CI, JSON and other
exports. Report added/missing cases, incompatible scope, invalid attempts and
inconclusive effects separately. Never infer a paired experiment from matching
round numbers in unrelated processes. Do not normalize away machine differences
with a single calibration multiplier.

Keep compact plans, summaries and artifact pointers in Git. Store full compressed
rounds and content-addressed binaries in durable artifact storage. Upload each
completed object once; build a fleet summary index from manifests without copying
raw data into every report. Preserve failed attempts and promoted baselines;
retention/GC must be explicit and must protect objects referenced by evidence.

## Cost accounting

For a planned experiment, host occupancy consists of build/setup plus the sum of
actual process startup, group warmup, sampling, monitoring, analysis and storage
costs. Fresh-process repetition repeats startup and warmup as well as sampling.
Selecting another CPU class adds another experiment. A fleet's elapsed time is
governed by the slowest host's assigned work and queueing, not by the sum of all
hosts' CPU time. These are accounting identities, not predictions of unmeasured
hardware.

Use a pilot on each host to quote a range and a bounded plan before a large run.
Reuse the compiled artifact and verified static topology when their identities
match; recheck dynamic placement and machine health. Spend confirmation work on
frozen candidates, and run hosts concurrently with one benchmark job per host.
Build once per compatible target/toolchain/feature configuration, rather than
assuming one executable runs on every architecture or operating system.
More measurements cannot remove a persistent measurement bias or fix incompatible
workload definitions.

For planning, a simple random-effects model illustrates why time allocation
matters: with P independent processes and R independent rounds per process,
`Var(mean difference) = process_variance/P + round_variance/(P*R)`.
This requires the model's independence assumptions; it is not a coverage claim
for current measurements. Once process variation dominates, extra rounds mostly
waste time. Repeated executions also do not randomize a binary's relative code
layout. When layout sensitivity matters, independent build/layout experiments
are a separate axis. Historical baselines do not replace fresh control runs
when the intended claim requires contemporaneous comparisons.

For two implementations in one binary, P process launches can contain both
arms. Comparing two separately compiled versions requires baseline and candidate
executions in each block: 2P launches. Additional classes and workload groups
multiply the selected scope. These execution counts do not predict runtimes for
crates or hosts that have not been measured.

The reproducible cost diagnostic is [defaults_audit.rs](../examples/defaults_audit.rs)
with [audit_defaults.py](../examples/audit_defaults.py). It uses an existing
verified Linux cpuset and identical-work arms; it measures harness cost, not the
accuracy of a proposed mode. Its child archive is an audit-only preservation
hook, because the existing process runner deletes exchange JSON.

## Implementation order and acceptance criteria

1. Preserve immutable child results and complete provenance; prevent misleading
   aggregate analyses and accounting. Reanalysis must recover all observations.
2. Make monitoring non-blocking during sampling and correct its accounting.
   Compare equal-round on/off controls and foreign-load controls before choosing
   new defaults. Keep owner-aware monitoring active for multithreaded workloads.
3. Implement one process-aware inference and diff policy, with explicit invalid,
   exploratory, inconclusive and confirmed states. Validate false-positive rate
   and interval coverage against independent statistical oracles across null,
   known-effect, correlated-noise and stopping-rule scenarios. Existing native
   larger-effect controls alone do not establish that coverage.
4. Add mode presets and budget planning on top of those contracts. A dry-run plan
   should state the selected classes, inputs, binaries, process count, budgets
   and storage destination. No preset may turn missing evidence into success.
5. Integrate the manifest/attempt contract with the existing fleet runner. Keep
   orchestration outside individual benchmark crates and do not introduce a
   second distributed job system here.

## Implemented monitor correction (2026-10-11)

At `a6179274`, the engine collects resource observations on a background sampler,
primes the first valid CPU delta before warmup, and retains observation age for
each round. Threaded groups keep the owner-aware monitor enabled. Missing/stale
observations, rival admission timeout and failed exclusive locking flag results.
Host-average observations still do not prove selected-core exclusivity.

The same fixed-200-round fixture, on the same CPU and cpuset, took a median
1.967 seconds with monitoring (three invocations), compared with 41.229 seconds
in the original enabled control. New disabled controls took a median 1.566
seconds. All 600 enabled rounds retained fresh clean observations; these are
resource samples, not continuous quietness certification. This measurement
precedes the mode changes and retains the original 500 ms per-arm warmup.

[Summary](../benchmarks/monitor_fixed_2026-10-11.summary.json),
[manifest](../benchmarks/monitor_fixed_2026-10-11.meta.json), and
[raw evidence/executable](../benchmarks/monitor_fixed_2026-10-11.pointer.md).
Resource log: `run-heavy: done rc=0 10s | peak-RSS 0.03GiB |
min-avail 26797MiB | peak-load 0.36`. RSS covers the Docker client; the payload
had a separate 2 GiB hard cap.
