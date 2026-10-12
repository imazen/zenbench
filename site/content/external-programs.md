+++
title = "Compare external programs"
description = "Measure prebuilt executables or one executable under different startup environments."
weight = 4
+++

Use `zenbench pair` when the arms are separate programs, or a setting is read once
at startup—dispatch, pool size, allocator configuration or environment variables.
The programs need not use the Rust Suite API. You supply an internal timing metric
and a correctness contract; Zenbench runs fresh children serially in randomized
nearby blocks. Use **Iterate** while editing, then [Confirm a frozen external comparison](@/external-confirmation.md)
when an acceptance decision is needed.

## 1. Define what the program reports

Build and retain your executable first. It must stay in the foreground, join its
workers, produce equivalent work across arms and exit successfully. Decide whether
timing includes setup, allocation, warmup, I/O and teardown. A metric extractor
cannot establish that both programs timed the same boundary.

The example assumes a program that accepts `--input PATH --output PATH`, writes
its result file and prints exactly three positive finite internal durations:

```text
elapsed_ms=...
elapsed_ms=...
elapsed_ms=...
```

The ellipses represent numbers produced by your program, not literal protocol
values. Each line must start with the prefix, on a new line. The program excludes
its own warmup from those measurements. Adapt the arguments, metric count and
correctness check to your program rather than adding a shell pipeline.

## 2. Compare the same binary with different environments

Save this as `plan.json` beside your `inputs` directory. Replace both program paths
with the absolute path to your retained binary, the input path with your file,
and the environment switch with one your program implements:

```json
{
  "schema": 1,
  "baseline": {
    "id": "before",
    "program": "/absolute/path/to/program",
    "args": ["--input", "${input}", "--output", "${output}/result.bin"],
    "env": {"IMPLEMENTATION": "old", "OPTION_TO_REMOVE": null},
    "revision": "same-retained-build",
    "timeout_seconds": 30
  },
  "candidates": [{
    "id": "after",
    "program": "/absolute/path/to/program",
    "args": ["--input", "${input}", "--output", "${output}/result.bin"],
    "env": {"IMPLEMENTATION": "new", "OPTION_TO_REMOVE": null},
    "revision": "same-retained-build",
    "timeout_seconds": 30
  }],
  "cases": [{"id": "small", "inputs": {"input": "inputs/small.bin"}}],
  "metric": {
    "extract": {"kind": "prefix", "prefix": "elapsed_ms="},
    "stream": "stdout",
    "unit": "ms",
    "expected": 3,
    "divide_by": 1
  },
  "check": {"kind": "files", "paths": ["result.bin"]},
  "warmup": "Program excludes its warmup from each elapsed_ms row",
  "blocks": 4,
  "aa": true,
  "deadline_seconds": 300
}
```

`null` removes an environment key. Both arms start from the same inherited
snapshot, with their explicit overrides applied. The plan can set shared `env`,
`record_env` for selected inherited values, or `clean_env: true` to start empty.
Explicit settings are retained in evidence, so do not put secrets in the plan.
An identical binary under different environments can also change layout and
startup state; A/A controls do not prove that only the intended switch changed.

## 3. Compare two prebuilt binaries

Use the same plan and change `baseline.program` and `candidates[0].program` to
the two immutable executable paths. Give them build annotations in `revision`;
keep environment, inputs, work counts and timing boundaries matched unless one
of those is the deliberate intervention. `revision` is an annotation, while
executable and declared input SHA-256 hashes identify the measured bytes.
Runtime libraries and undeclared assets are not automatically identified.

Relative program, cwd and input paths resolve beside the plan. A bare program
name searches the arm's PATH. `${input}`, case `fields` and `${output}` expand
literally inside one argument; a path with spaces stays one argument. No implicit
shell runs, and unresolved placeholders fail. Each invocation gets a fresh output
directory. Declare runtime assets as inputs when they affect the experiment.

## 4. Validate, then run one relevant case

Inspect [CPU placement](@/cpu-placement.md), then use a new output directory:

```sh
zenbench pair plan.json --mode=iterate --case small --dry-run
zenbench pair plan.json --mode=iterate --case small --output target/pair-edit-01 --wait 60
zenbench data read target/pair-edit-01/packet.json.gz
zenbench data read target/pair-edit-01/packet.json.gz --format csv
```

Dry-run prints identities, placement, resolved arguments, schedule and invocation
count. With four blocks, one baseline, one candidate and A/A, one case launches
**12 fresh children**. This is a count, not a runtime prediction. The default
queue wait is zero; `--wait 60` allows bounded FIFO admission. Ordinary background
load is observed and retained; it does not establish clean measurement conditions.

Every invocation contributes one arithmetic mean of its internal timing vector.
Each complete block contributes one candidate/baseline log ratio. The report
shows geometric mean latency change and between-block log-ratio dispersion.
Three internal timings in one child remain one process observation; they do not
become three independent repetitions. Positive latency change means slower.

## Choose a metric extractor

Replace the example's `metric.extract` with the object for your actual output:

| Program output | `extract` object | Detail |
|---|---|---|
| Lines starting `elapsed_ms=` | `{"kind":"prefix","prefix":"elapsed_ms="}` | Parses the rest of each matching line |
| TSV rows starting `time` and a tab | `{"kind":"tsv","prefix":"time\t","column":1}` | Columns are zero-based; the full row is split |
| `METRIC {"elapsed_ms":NUMBER}` lines | `{"kind":"json","prefix":"METRIC ","pointer":"/elapsed_ms"}` | Strips prefix, parses each line, follows a JSON pointer |
| A numeric field in a log | `{"kind":"regex","pattern":"elapsed_ms=([0-9]+(?:\\.[0-9]+)?)","capture":1}` | Capture 1 is the first group; regex scans the full stream |
| Entire invocation wall time | `{"kind":"wall"}` | Requires `unit: "ns"` and `expected: 1` |

`stream` selects stdout (default) or stderr. `expected` is the exact number of
matching values. Units are `ns`, `us`, `ms` or `s`; `divide_by` is a positive
predeclared work count for per-operation normalization. Missing, extra, malformed,
zero, negative or nonfinite values fail. Failed extraction never falls back to
wall time.

Wall time includes launch, supervisor completion polling and cleanup. It is
unsuitable for short kernels; use internal metrics for those. The required
`warmup` text describes the child's actual boundary; it does not warm a future
child. Optional `settle_ms` waits before launches rather than executing warmup.

## Choose a correctness contract

| Contract | Plan `check` | Use it when |
|---|---|---|
| Exact output files | `{"kind":"files","paths":["result.bin"]}` | Output bytes must match; paths are relative to the child output directory |
| JSON result fields | `{"kind":"json","prefix":"CHECK ","pointers":["/digest","/work_count"]}` | A digest and performed-work count must match |
| External oracle | `{"kind":"command","program":"./check-output","args":["${baseline}","${candidate}"],"timeout_seconds":30}` | Your contract needs a semantic or tolerance-aware check |
| Explicitly unchecked | `{"kind":"unchecked","reason":"Describe the missing correctness guarantee"}` | Exploratory execution has a documented limitation |

JSON checks can independently choose `stream: "stderr"`. A checker program is a
path resolved beside the plan, receives baseline/candidate **output directories**,
and must return zero. Its logs are retained. Each candidate and A/A is checked
against its block's baseline outside the child's internal timing metric. Declare
approximation tolerances before measuring; file equality is appropriate only
when your output contract requires it.

## Review, stop or resume

A/A clones the baseline launch by default. Examine its effect, order and variability
alongside A/B; do not subtract it automatically or declare a candidate accepted
because its estimate exceeds one observed A/A difference.

| Exit | Meaning | What to do |
|---|---|---|
| 0 | Planned execution/reporting completed | Review correctness, controls and variation; performance remains exploratory |
| 2 | Failed, incomplete or not-run | Inspect retained errors/logs and keep the attempt |

Correctness, extraction or supervision failure is invalid evidence for the planned
contract. An incomplete block contributes no pair. Stop optimizing that candidate
until the cause is understood; avoid collecting until a favorable result appears.

After an interrupted **Iterate** experiment, use checked resume with a **new** output directory:

```sh
zenbench pair plan.json --mode=iterate --case small --resume target/pair-edit-01 --output target/pair-edit-02 --wait 60
zenbench data pair-diff target/pair-edit-01/packet.json.gz target/pair-edit-02/packet.json.gz
```

Keep the original case selection. Resume checks plan, executable/input identities,
recorded environment, placement and artifact hashes. Completed cases are reused;
an incomplete case starts a new full attempt. Old failed attempts and journal
tails remain. `pair-diff` compares saved packets without pooling populations.

Copy only the selected rows in `selected.csv` to a deliberate Git path, with a
pointer to the retained packet. Keep the attempt directory, raw vectors, output
files, stdout/stderr and checkpoints in durable artifact storage.
[Evidence storage](@/results.md) explains the split.

## External confirmation

Freeze the candidate and use [external Confirm](@/external-confirmation.md) with
at least five fixed blocks, A/A, checked correctness, a declared timing error
bound and a confirmation policy. The command is `zenbench pair confirm-plan.json
--mode=confirm`; its 0/1/2 exit contract differs from Iterate's execution result.
Confirm cannot resume or extend the plan after looking at results.

Native [Suite confirmation](@/ci-regression.md) remains a separate workflow.
Separate historical suite populations have a different design from randomized
external A/B blocks; keep that distinction when choosing acceptance evidence.

[Complete external-process contract](https://github.com/imazen/zenbench/blob/main/docs/external-process.md)
and [offline plan guide](https://github.com/imazen/zenbench/blob/main/src/pair-help.txt)
cover limits and retained identities.
