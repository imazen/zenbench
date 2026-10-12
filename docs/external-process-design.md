# Issue 29: external-process A/B implementation design

**Status: exploratory and fixed-block confirmation implemented.**
See the [implemented contract and limits](external-process.md), including explicit
resolution, A/A equivalence, resource observations and acceptance rules. The
original design below records the reasoning and acceptance gates; the implemented
contract takes precedence where stricter (A/A required and no resume in Confirm).
This specifies what to build for [issue 29](https://github.com/imazen/zenbench/issues/29),
including one executable under different environments. The current
[methodology](../METHODOLOGY.md) describes the existing in-process harness.

## Product boundary

Build a CLI-first `zenbench pair` runner driven by a versioned JSON experiment
plan. An arm names an executable, argv array, cwd, environment set/unset operations,
metric extraction and output contract. Case records supply inputs and declared
parameters. Two arms can name the same executable; executable hash alone must
never be their identity. Resolved environment/arguments distinguish the intervention.

The command should take a plan plus mode, case filter, deadline and resume options.
The plan is the durable interface; shell convenience flags can produce the same
resolved plan. `--dry-run` validates and prints the exact launches, comparison
family, CPU placement, warmup/metric boundaries, schedule and storage location
without running the workload. It must not invent a runtime estimate.

Keep implementation types private initially. A stable Rust `ProcessArm` or custom
extractor trait can follow experience with both codec harnesses; the first runner
should not force users to compile a wrapper binary or change their program.

## First usable delivery

Ship the following together as **exploratory** external comparison. Do not call
a launch loop alone an implementation of issue 29.

| Component | Required behavior |
|---|---|
| Arms | Baseline plus candidates; same or different executables; per-arm args, env set/unset, cwd, timeout and revision labels |
| Cases | Explicit named records; safe literal placeholder substitution into individual argv/env fields, with unknown/missing fields rejected |
| CPU control | Reuse verified process placement; explicit sets from topology, not arithmetic guesses about P/E IDs; identical policy across comparison arms |
| Scheduling | Fresh child per arm per block, one timing arm at a time, randomized order saved before execution |
| Metrics | Prefix/key-value, TSV field, JSON-line field and regex adapters; explicit units, normalization and expected cardinality; wall-clock measurement only when explicitly selected |
| Correctness | Byte equality or declared digest/count fields; an external checker command for a domain contract; its status and artifacts retained |
| A/A | Automatic clone of the baseline arm, with a separate control contrast and diagnostics; explicit opt-out retained in the plan |
| Retention | Every invocation, raw metric vector, stdout/stderr, errors, resource observations and realized schedule; complete/incomplete state per block and case |
| Resume | Match the immutable resolved-plan fingerprint and retained artifact hashes; keep unsuccessful and interrupted attempts |
| Reporting | Per-cell A/A and A/B estimates, dispersion, completed block/invocation counts, correctness and placement/load status; small selected exports |

A/A adds another baseline invocation in each block. It is enabled by default for
the external runner because it tests the launch/comparison mechanism itself.
Its added invocation count must be visible in dry-run. This is a cost decision,
not a measured runtime multiplier. Users doing a very short exploratory check
can explicitly disable it; the report must say the control is absent.

Use four planned blocks as the initial Iterate default, reflecting the issue's
short feedback use case. It remains exploratory, including with more blocks.
Select one relevant case during editing instead of running the entire grid.
Every arm has a timeout and the command has a total deadline; these are actual
execution limits, not expected durations. Pilot the real decoder/encoder cases
before declaring these defaults efficient. No external-runner wall time has yet
been measured.

### Environment and invocation identity

Capture one common inherited environment at experiment startup and derive every
arm from it; never change the runner's global environment between launches.
Apply common overrides, then arm overrides and explicit removals. Record explicit
settings and allowlisted inherited values without publishing unrelated secrets.
Unrecorded inherited state limits reproducibility; support a clean, explicitly
specified environment for experiments requiring complete control.

Hash executable bytes, relevant runtime assets and declared inputs. Include the
resolved arm/case configuration and metric/checker versions in the experiment
fingerprint. Revision labels are annotations, not substitutes for binary hashes
or build manifests. Changes to diagnostic environment switches must be visible
as interventions. Changing an environment string can also change process layout;
identical executable hashes do not remove that limitation.

Use argv arrays directly, without an implicit shell. An explicitly invoked shell
is part of the arm's declared command. Resolve input paths before execution and
provide a distinct retained output directory per invocation so stale artifacts
cannot satisfy the checker. Validate correctness outside the timed metric, and
preserve the exact work count used to normalize it.

### Internal timing is a contract

A child can emit several internal timing values. Retain the complete vector with
its process ID, invocation ID, block ID, order and metric units. The extractor
must reject missing, extra, malformed, nonfinite or nonpositive required values.
Unexpected exit status invalidates the invocation even if stdout contains a
plausible timing row. A failed internal extractor never silently switches to wall
time. Record outer wall time separately for experiment-cost accounting.

Choose one declared per-invocation summary, initially the arithmetic mean of the
internal latency values. Use equal invocation weight in the outer comparison;
three repetitions from one child are not three independent process repetitions.
If work counts differ, reject incompatible work or use a predeclared normalization
before summarizing. A median-of-internal-passes option would be a different
estimand and must be named/versioned accordingly.

Record whether warmup is inside the child, explicitly excluded by its metric, or
absent. Launching and discarding a separate warmup process does not warm the next
child's private allocator, pool or address space. Provide a fixed optional settling
delay, record it as wall-time cost, and keep it identical across arms. Do not use
adaptive delays that depend on which candidate looks faster.

## Busy-machine behavior

Default Iterate should **observe ordinary load and run**, retaining clear warnings.
Unknown CPU class still requires explicit unverified exploration or a valid class
boundary. Busy-system tolerance must not silently become permission to mix P/E
cores or unequal cache classes.

Serialize the runner's own timing arms and cooperate with the existing host-wide
benchmark lock. Use nonblocking admission by default for the external Iterate
runner: a held benchmark lock reports who holds it and returns a distinct “not
run” outcome. An explicit bounded wait option lets an agent queue. Never start
a rival measurement after a queue timeout. This bounds lost agent time without
throwing away load observations or requiring the whole host to become idle.

Do not change sibling agents' priority, suspend builds, or automatically request
administrator access. Record effective priority/QoS and use the same policy for
both arms. A `never-nice` flag is not a portable measurement guarantee; changing
priority is an explicit environment decision subject to host resource policy.

Retain load, observation age and whatever selected-domain diagnostics are
available before/after each invocation. Until a per-core/domain policy is tested,
these are observations, not proof that caches, SMT, bandwidth or package power
were isolated. The current whole-host strict policy remains the confirmation
policy; it is not silently relaxed in this runner.

If an entire A/B block cannot finish before the deadline, retain its partial data
and do not manufacture a pair. Exploratory reporting can summarize completed
blocks while clearly marking the planned experiment incomplete. It cannot promote
a baseline. Failures and missing blocks are reported, not retried away. A rerun
gets a distinct attempt identity.

## Confirmation follows with its own validated protocol

Add Confirm/Evidence after the exploratory runner and evidence schema pass the
controls below. Do not feed external timing vectors into the existing within-process
bootstrap and call that confirmation.

For each complete randomized block, compute one candidate/baseline log ratio from
the per-invocation summaries. A/A is a separate baseline-clone/baseline contrast.
Use fixed planned blocks, with at least five for the initial confirmation policy,
and a t interval across block log ratios. Bonferroni covers all declared candidate
contrasts and A/A controls in the selected experiment. Apply the existing practical
margin semantics after converting log bounds to percentage latency change.
The block is the inferential unit. Its independence and approximate log normality
are assumptions to state and validate, not consequences of launching children.

A timing-resolution floor must come from the metric contract. Self-reported
milliseconds do not inherit the supervisor's `Instant` resolution. Require a
positive declared metric resolution for confirmation, retain it with units, and
validate the propagated ratio floor independently. Unknown metric resolution
remains exploratory. Do not invent precision from printed decimal places.

Confirmation requires all planned blocks, valid outputs, compatible placement,
fresh clean observations and fixed identities/settings. Require the A/A interval
to fit inside its predeclared equivalence margin when the control is enabled;
an unresolved control prevents acceptance. Passing A/A is necessary under that
policy, not sufficient proof that A/B is unbiased. Never subtract observed A/A
bias or authorize confirmation using `|AB−1| > |AA−1|`. That inequality can be an
explicitly labeled diagnostic for migrating existing reports.

Retain all data when a control or candidate fails. No fastest-process selection,
post-hoc outlier deletion, optional stopping, automatic extension until significant,
or silent quiet-only resampling. An interrupted confirmation is incomplete; resume
may finish other untouched cases, but restarting that incomplete case creates a
new attempt/population. Do not combine separate days into one homogeneous block
population without an analysis that models that distinction.

Exit meanings should match the existing intent: exploratory success says only
that execution/reporting succeeded; confirmation 0 requires acceptance, 1 reports
regression, and 2 covers invalid/inconclusive/not-run outcomes with a machine-readable
reason. Evidence adds scope/provenance requirements, not a different estimator.

## Storage and implementation boundaries

Introduce a versioned external-process packet that retains cases, blocks,
invocations, metric vectors and correctness evidence. Do not synthesize a
`SuiteResult` whose fields pretend external invocations were in-process rounds.
Share the existing content-addressed gzip storage primitives, readable command
metadata and selected-summary export conventions. Extend read/compare/reanalysis
through explicit packet-kind dispatch and reject unknown schemas.

Checkpoint an append-only journal while running, then publish immutable complete
packets. Keep logs and produced artifacts outside Git. A stable selected CSV plus
manifest references is enough for review; raw events remain in the artifact store.
Resume verifies the plan, binaries, inputs and completed-case evidence. An altered
binary/configuration cannot inherit a prior cell's “complete” status. Preserve
partial journal tails as interrupted evidence rather than silently dropping them.

Expected internal modules are plan/validation, serial supervisor, metric adapters,
output checks, packet/storage and block-level analysis. Reuse the CPU and monitoring
components through crate-private interfaces. The parent owns cooperative admission;
registered child PIDs belong to its measurement and must not be mistaken for rivals.
Unregistered foreign work remains visible. Direct child timeout/kill/reap and output
capture must be tested on Linux, macOS and Windows. Descendant containment needs
platform verification before promising that arbitrary launchers cannot escape a
deadline; unsupported cleanup must be documented or rejected, not claimed complete.

## Acceptance gates and review order

1. **Runner plus exploratory evidence:** same binary/different environments; two
   different binaries; literal paths/arguments with spaces; A/A; known extra work;
   multiple internal samples; bad output/status/cardinality; timeout and interrupted
   block; stale artifact rejection; complete attempt retention and bounded admission.
   Tests use fixture programs with deterministic internal metrics as well as real
   work. Do not assert exact wall-time speedups in CI.
2. **Adapters and migration:** replay the retained decoder and encoder outputs,
   validate units/frame counts and byte-equivalence checks independently, and run
   the actual prebuilt programs with the new plan. Compare retained event membership
   and computations against an independent script. The old scripts' heuristics and
   missing-sample behavior are not assertions to copy. Keep migrations within their
   owning repositories as separately authorized changes.
3. **Confirmation:** independent numerical oracle for block intervals and timer
   floors; null/known-effect controls; false-acceptance checks under drift,
   correlated blocks, order/carryover effects and incomplete/control-failing data.
   Whole-process repetitions stay clustered. Confirm remains unavailable until
   its analysis and validity gates are implemented together.
4. **Busy-system evaluation:** isolated controlled experiments with foreign work
   on the selected CPU, SMT sibling, another cache domain and memory bandwidth.
   Measure misleading conclusions, inconclusive outcomes and wall time, including
   refusal/queue time. Keep real interfering work visible. Reservation-aware
   admission follows this evidence rather than a guessed global-load threshold.

Numeric size-axis fitting, persistent resident-worker protocols and build/checkout
orchestration can follow separately. They are not needed to compare today's
prebuilt executables. Persistent workers could save startup cost, but they would
measure a different state/repetition protocol and must not silently replace fresh
processes. Keep the first implementation focused on retiring the duplicated
measurement loops with honest evidence and usable iteration behavior.
