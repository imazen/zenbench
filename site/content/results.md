+++
title = "Save and review evidence"
description = "Keep complete attempts outside Git and export a deliberate small summary."
weight = 7
+++

Use the retained data to review an experiment without rerunning it. Native suite
evidence lives in ignored `.zenbench/data/`, or `ZENBENCH_DATA_DIR` when configured.
It includes immutable hash-addressed gzip objects, process attempts and small
references. Selected summaries are for review; raw packets are for reconstruction
and inference.

## Give a suite attempt a clear path

For a retained zenbench benchmark executable:

```sh
ZENBENCH_RESULT_PATH="$PWD/candidate-01.json.gz" ./builds/candidate/my_bench --group=sort
zenbench data read candidate-01.json.gz
zenbench data read candidate-01.json.gz --format json
```

Use a fresh output path for every attempt. `data read` accepts JSON, gzip JSON or
an evidence reference and formats it as `llm`, `json`, `csv`, `md` or suite `html`.
It does not run the benchmark. The same reader understands retained external pair
packets for the text formats.

In PowerShell, set `$env:ZENBENCH_RESULT_PATH` before the executable invocation;
see [retaining builds](@/iterate.md#keep-two-builds-and-rerun-without-compiling).

## Export charts and reports

[Charts for slides and reports](@/charts.md) covers SVG, fixed light/dark themes,
vertical grouped bars and self-contained HTML. Export from retained native suite
evidence with `zenbench data read candidate-01.json.gz --save-charts figures`;
no new measurement is needed.

## Choose the correct comparison

| Evidence | Command | Interpretation |
|---|---|---|
| Native suite packets | `zenbench data compare before.json.gz after.json.gz` | Process inference; exit 0/1/2 is acceptance/regression/invalid-or-inconclusive |
| External pair packets | `zenbench data pair-diff before/packet.json.gz after/packet.json.gz` | Descriptive summaries and identity agreement; no pooled population |
| Fleet attempt directories | `zenbench fleet diff attempts/before attempts/after` | Compatible individual observations; descriptive only |

`data compare` cannot turn Iterate data into confirming evidence. Inspect exploratory
packets with `data read`. Match workload, inventory, settings, placement and testbed;
an identity mismatch is a reason to investigate the experiment rather than edit
the stored evidence.

## Export one stable summary for Git

For a selected **native suite** packet:

```sh
mkdir -p benchmarks
zenbench data snapshot candidate-01.json.gz --output benchmarks/baseline.csv
```

Commit this deliberate snapshot and a pointer to the raw evidence and build
manifest. Avoid exporting every exploratory attempt into Git. Snapshots retain
evidence identity and read/compare command guidance. CSV is not an input to
confirmation; it does not contain all process/round samples.

External `pair` attempts already have `selected.csv`. Their Confirm decision is
retained in the pair packet; `pair-diff` remains descriptive. Iterate does not
become confirming data when read later. Copy the chosen rows with
a packet pointer rather than using the suite snapshot command. Fleet uses
`zenbench fleet export study.jsonl ATTEMPT...`; [the fleet guide](@/fleet.md)
explains its receipt checks.

## Archive and reanalyze without measuring again

Losslessly compact an older suite JSON packet into a new gzip file:

```sh
zenbench data archive candidate.json --output candidate-archive.json.gz
```

Replay a complete suite packet's frozen process analysis into a separate artifact:

```sh
zenbench data reanalyze candidate-confirm.json.gz --output analysis-01.json
```

Reanalysis references the immutable packet hash. It cannot promote exploratory
evidence or repair missing samples. Keep the original packet and policy alongside
the new analysis.

## Keep the objects that references point to

Copy full gzip objects, child attempts, runtime assets and retained binaries to
durable artifact storage. Verify hashes when moving them. A reference file or CSV
alone cannot recreate the data. Preserve named baseline references and their
objects across CI jobs; set `ZENBENCH_DATA_DIR` to a persisted location when needed.
See [portable storage](https://github.com/imazen/zenbench/blob/main/docs/result-storage.md)
for reference resolution and retained process lifecycle records.

Build-time identity remains your responsibility: runtime Git HEAD is not the
source used to build a retained executable. Bind source revision, dirty patch,
compiler, target, flags, dependencies and input hashes to the binary hash.

Keep invalid, cancelled and inconclusive attempts too. Distinguish a planned
replicate from a retry; do not select only the fastest successful receipt. You
have a reviewable result when a reviewer can identify the intended experiment,
find the selected summary and follow its pointers back to complete evidence.
