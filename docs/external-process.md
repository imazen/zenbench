# External comparisons with `zenbench pair`

`zenbench pair` compares prebuilt executables, including the same executable
with different startup environment variables. Iterate supports quick exploration;
Confirm evaluates a frozen experiment with explicit acceptance rules.
Run `zenbench pair --help` or `zenbench guide --topic pair` for a complete JSON
plan and commands without needing this document installed.

```sh
zenbench pair plan.json --dry-run
zenbench pair plan.json --case small --output target/pair-edit
zenbench data read target/pair-edit/packet.json.gz --format json
zenbench data read target/pair-edit/packet.json.gz --format csv
zenbench data pair-diff target/pair-before/packet.json.gz target/pair-edit/packet.json.gz
```

Select one relevant case during editing. The default is four blocks, one fresh
process per arm in each block, plus an automatic clone of the baseline as an
A/A control. With one candidate that is 12 invocations. `aa: false` is an explicit
cost/diagnostic tradeoff and stays visible in the report. These are counts, not
predicted wall times. `--dry-run` validates the plan and prints resolved launches,
identities, schedule, placement and the invocation count before running anything.

## What a plan declares

The schema and an executable example are embedded in the
[offline CLI guide](../src/pair-help.txt). Unknown schema versions and fields
are errors. A plan is limited to 10,000 invocations, one million retained metric
values and 8 MiB of expanded launch descriptions; select fewer cases for larger
studies. Arms have an executable, argv array, cwd, environment set/remove
operations, revision annotation and deadline. An environment value of `null`
removes that key. Both arms derive from the same startup environment snapshot.
`clean_env: true` instead starts with an empty environment. `record_env` names
inherited values the owner chooses to retain; unrelated inherited secrets are
not dumped into evidence. Explicit settings are retained, so keep secrets out
of the plan itself.

Case `inputs` are resolved paths whose bytes are hashed. Case `fields` are literal
parameters. `${input_name}`, `${field_name}` and `${output}` substitute within
one argument or environment value. They do not invoke a shell or split spaces.
Unknown placeholders are errors. Every invocation receives a distinct output
directory. Relative cwd/program/input paths resolve from the plan directory;
a bare program name searches that arm's PATH. Use absolute executable paths
for the least ambiguity. Runtime libraries and undeclared assets are outside
the identity guarantee; declare relevant assets as inputs.

The required `warmup` string describes the program's actual timing boundary.
A fixed `settle_ms` delay can precede each invocation. Zenbench does not discard a
separate warmup process and pretend that it warmed the next child's allocator
or thread pools.

## Metrics, correctness and inference

Extractors support prefix/value lines, TSV columns, JSON-line pointers, regular
expression capture groups and explicitly selected outer wall time. Metrics and
JSON correctness checks independently select `stream: "stdout"` (default) or
`stream: "stderr"`. Prefix/TSV
matching is line-based. A JSON prefix is stripped before parsing that line;
regular expressions run over the full selected stream. `expected` is exact
cardinality. Missing, extra, malformed, zero, negative and nonfinite values fail
the invocation. `unit` accepts ns/us/ms/s; `divide_by` is a positive, predeclared
work normalization. Normalized vectors are retained, with original stdout/stderr.
There is no fallback to another metric after extraction fails.

The explicit wall metric includes process launch, completion polling and cleanup
in the supervisor. Polling currently sleeps 10 ms. It is unsuitable for timing
short kernels; extract their internal metrics. Before/after system observations
and correctness checks are outside the child metric. All of them still cost
experiment wall time.

Correctness can compare declared output files by SHA-256, compare JSON fields
(for example output digest and work count), or run an external checker. File
checks resolve inside the fresh output directory. A checker receives literal
`${baseline}` and `${candidate}` output directory arguments and must return zero;
its logs are retained. `unchecked` requires a written reason and is reported.
Each candidate and A/A control is checked against its own block's baseline.

Every invocation contributes one arithmetic mean of its internal latency vector.
A complete block contributes one candidate/baseline log ratio. The report shows
the geometric mean ratio as percent latency change, min/median/MAD summaries
and the sample standard deviation of block log ratios. Internal samples are never treated as independent
process repetitions. An incomplete or failed block contributes no pair, even if
two of its arms happened to finish. All attempted data remain in evidence.

## Confirm a frozen experiment

Add a confirmation contract to the plan, then run `zenbench pair plan.json
--mode confirm`. The default remains Iterate even when this object is present.

```json
"blocks": 5,
"confirmation": {
  "margin_pct": 1.0,
  "control_margin_pct": 1.0,
  "family_confidence": 0.95,
  "scope": "decoder-inputs-v1",
  "accept": "non_inferiority"
}
```

Also declare `metric.resolution`, a positive conservative bound on absolute error
in each reported timing, in the extracted unit **before** `divide_by`. Determine
it from the timer and reporting contract; printed decimal places alone are not a
resolution measurement. A quantization/error bound is not independent noise and
does not shrink with more internal repetitions or more blocks.

Confirm requires at least five fixed blocks, A/A, checked correctness and a scope
ID. `accept` chooses `improvement`, `equivalence`, or `non_inferiority` (default).
The two margins are percentages of baseline latency. Set them before execution.
The report contains baseline/arm mean, median, minimum and MAD of invocation means,
plus those descriptive summaries of paired ratios. These describe different
estimands: the inferential estimate is the geometric mean ratio.

The interval is Student-t on one log ratio per complete block, with Bonferroni
across **every selected case × (candidate contrasts + A/A)**. Missing cases do not
reduce the family. Exact extremal log ratios under the declared timing-error
bound widen each statistical endpoint. No control-bias subtraction, outlier
removal, fastest-process selection or optional stopping is performed. The legacy
`|AB−1| > |AA−1|` rule appears as `effect_exceeds_control_diagnostic`; it never
accepts a change. Statistical details and numerical/model controls are in
[pair inference validation](pair-inference.md).

Acceptance requires all planned blocks, verified uniform CPU placement, fresh
strict resource observations, and the A/A interval wholly inside its control
margin. An unresolved control makes the cell inconclusive. Exit 0 means the
selected acceptance rule passed for all candidate cells; 1 means a regression;
2 means invalid, inconclusive, incomplete or not-run. Invalid/inconclusive cells
have precedence over regressions in mixed experiments. Inspect per-cell reasons.
Confirm refuses `--resume`: start a new, separately retained fixed experiment.
Iterate keeps checked resume and exits 0 for successful execution only.

Confirmation remains conditional on independent, approximately normal block log
ratios and unchanged workload/environment assumptions. A/A does not detect a
candidate-specific confounder. Independent studies across machines/build layouts
are needed for claims beyond the selected experiment. More blocks are not a cure
for correlated observations.

## Busy machines, deadlines and resume

CPU-class validation is the same process-boundary check used by the benchmark
harness. Use `zenbench exec` for a verified inherited boundary; an explicit
`ZENBENCH_CPU_POLICY=unrestricted` permits unverified exploration. Keep different
CPU/cache classes in separate experiments. Ordinary load is observed, not treated
as proof of cleanliness or silently subtracted from the metric.

The runner holds [cooperative admission](cooperative-scheduling.md) for the whole
experiment. Its default queue wait is zero: a busy reservation reports not-run.
`--wait SECONDS` enables bounded FIFO waiting. Each arm has a timeout and the plan
has a total deadline. Foreground process groups (Unix) or Jobs (Windows) are
supervised; Unix daemonization is outside the supported command contract.
Supervision failures stop further launches. Logs are checked against a 64 MiB
limit while the child runs, with bounded-interval polling rather than a filesystem
quota. Cleanup has a separate bounded grace period and can extend the deadline.

Iterate resume accepts a retained packet or attempt directory. It verifies the resolved
plan fingerprint, current executable/input bytes, CPU placement and retained
artifact hashes. A fully completed case is reused. An incomplete case starts
again in a new attempt directory; it does not borrow successful arms from a
failed block. Interrupted journal tails stay in their original file. Only checked
block checkpoints can establish completed cases after an interruption. A crashed
running admission reservation still requires verifying child cleanup before a
new run can start.

## Storage and review

Each ignored attempt directory contains a planned experiment, append-only event
journal, checked block checkpoints, invocation logs/artifacts, a gzip JSON packet
and `selected.csv`. Raw events are outside Git by default. Export selected CSV
rows and retain a packet pointer for review; do not check in every attempt.
The packet embeds decode/export/comparison commands. `pair-diff` presents both
sets of summaries and their identity agreement without pooling populations.

`pair` exit 0 means execution and reporting completed, including any explicitly
unchecked correctness contract. Exit 2 means failed/incomplete/not-run. It never
means a candidate has been accepted as faster. The external runner does not build
or check out source revisions. Revision strings are annotations; executable and
input hashes identify measured bytes.

## Prior art: Tango

This design review used Tango at commit
[`cd8cff90124751ced235361e358a03383137d1c5`](https://github.com/bazhenov/tango/tree/cd8cff90124751ced235361e358a03383137d1c5).
Its current [worker implementation](https://github.com/bazhenov/tango/blob/cd8cff90124751ced235361e358a03383137d1c5/tango-bench/src/worker.rs)
uses persistent benchmark workers and batched measurements. Its
[communication page](https://github.com/bazhenov/tango/blob/cd8cff90124751ced235361e358a03383137d1c5/doc/commpage.md)
coordinates workers through atomic progress counters, with results transferred
separately. The worker loop can measure both workers concurrently; the barrier
aligns progress rather than serializing each timed arm.

Tango provides useful ideas for a future persistent-worker protocol: calibrate
batch sizes, keep synchronization inexpensive, and distinguish diagnostic CPU
counters from latency samples. Zenbench's arbitrary-executable runner instead
launches fresh children serially. Opaque programs do not implement Tango's worker
protocol, and concurrent timing arms could contend on a busy machine. Persistent
workers would be a separately named protocol with different state and cost,
not a silent optimization of these fresh-process experiments.

## Resource and priority observations

Confirm samples the system before, during and after each invocation. Known live
worker leaders and descendants are subtracted from CPU admission; unrelated work
remains visible. Admission uses the existing strict policy: CPU ≤5%, no foreign
heavy processes, available RAM ≥512 MiB, and CPU temperature ≤85°C when available.
A sample taking over two seconds is invalid. Missing temperature is retained as
unknown, matching the existing policy, not invented as a cool reading.

At least one complete during-process observation is required after the initial
process-counter calibration window. A process shorter than two OS sampling
intervals may be unresolved: increase its predeclared internal
work budget while retaining the same estimand, or use Iterate.

New and reaped worker CPU cannot reliably be assigned. The first process-counter
window (`start_transition`) and every window completing at or after the
supervisor's owned exit observation (`exit_transition`) therefore waive only the
whole-host CPU-load magnitude (`admission_waiver: cpu_load_magnitude`). Foreign
heavy processes, RAM, temperature, sample freshness and a finite CPU reading stay
enforced at every window. The exit observation is taken after the supervisor's own
`try_wait` returns the leader's status, before cleanup. Windows completing
earlier, including those after the last successful liveness probe, remain subject
to full admission. Each sampled window keeps its raw counters plus
`sample_completed_after_start_ns`, `leader_last_alive_after_start_ns` (a lower
bound on exit) and `leader_exit_observed_after_start_ns` (an upper bound).
Missing owned lifecycle evidence adds a failing `lifecycle_unobserved`
observation. A fixed fresh post-exit sample follows.

Sampling cannot certify absence of interference between observations. Completion
times trail the OS counter refresh, so the window whose refresh spans reaping is
not admitted by CPU magnitude even when its counters still assigned the leader.
These blind intervals and OS counter limitations remain visible in raw evidence.

Unix nice values are queried for each child; unavailable values and unsupported
platform queries have explicit reasons. Children inherit priority and must not
change it during timing. QoS is not certified. Zenbench does not elevate itself,
renice unrelated work or assume that nice values identify a CPU class.
