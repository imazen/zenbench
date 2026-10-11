# Results without thousands of Git files

Ordinary runs write compressed evidence under `.zenbench/data/`, outside Cargo's
`target/`. Set `ZENBENCH_DATA_DIR` to use an external durable directory. Each data
root contains its own `.gitignore` excluding raw evidence. Nothing is automatically
added to version control. Existing saved files are not migrated or deleted.

For Git, export a **selected snapshot to a stable filename**, updating that file
when accepting a baseline. Do not export every iteration:

```sh
zenbench snapshot .zenbench/data/runs/<id>.json --output benchmarks/baseline.csv
```

Read or compare retained evidence without running benchmarks:

```sh
zenbench read .zenbench/data/runs/<id>.json --format llm
zenbench read result.json.gz --format json
zenbench compare baseline.json.gz candidate.json.gz
```

`read` defaults to LLM-readable text and also accepts `json`, `csv`, and `md`.
References and exported CSV headers include read, decode and compare command
argument arrays, preserving spaces and shell metacharacters in paths. Replace
`<BASELINE>` with the baseline packet/reference; run relative paths from the
project directory. These arrays are instructions for the reader, never code
executed by the loader. Gzip headers also identify the format and decoding commands;
standard gzip tools can decode the JSON independently.

Snapshots are readable summaries, not comparison inputs: use their embedded
command to locate the full evidence. `compare` exits 0 for confirmed non-inferiority,
1 for regression, and 2 for invalid or inconclusive evidence. Reading exploratory
data does not turn it into confirmation. Historical references without instructions
continue to load.

The CSV has one sorted row per benchmark, an evidence-hash header and a compact
context header with testbed/CPU, workload/scope, executable hash and decision policy. Export retains that packet before writing the CSV.
Rows contain descriptive mean/MAD/count/scope/reliability plus available
process-level intervals/verdicts; exporting does not
certify a performance claim. Timing values are not rounded away. Snapshot size
depends on benchmark count, not round count or number of child processes.

Store the raw objects in your artifact store and keep the chosen CSV in Git.
An experiment spanning many machines should select one snapshot per scope or
combine selected tables in the owning fleet report, not commit each process's
JSON. This crate does not upload data or create a second fleet scheduler.

## On-disk evidence

- `objects/<sha256>.json.gz`: immutable complete packet, including replicate
  boundaries and all retained rounds. Identical packets share one object.
- `runs/<id>.json`: small versioned reference. Loading verifies the object hash.
- `attempts/<id>.json.gz`: checkpoint of an active/interrupted attempt, marked
  incomplete. A successful publication retires its own checkpoint.
- `processes/<experiment>/experiment.json`: plan, executable hash and per-attempt
  lifecycle status. Child stderr remains beside the packets, including failures.
- `processes/<experiment>/<index>.json.gz`: subprocess output, retained even if a
  subsequent child fails. Aggregates also retain the original replicate packets.

Writes to ordinary result/snapshot paths use a sibling temporary file and rename;
write failures are errors. Evidence references appear only after the object is
written and synced. Interrupted object creation cannot become a successful
reference. Unknown or corrupt references fail loading; there is no silent fallback.
Reading and publication cap both encoded and decoded packets at 256 MiB. Larger experiments must
be split into separate suites rather than silently truncate their measurements.

`SuiteResult::save("result.json")` writes compact JSON without indentation;
`save("result.json.gz")` writes gzip. `SuiteResult::load` and CLI comparison accept
historical JSON, gzip JSON, and evidence references. To compact an existing file:

```sh
zenbench archive old-result.json --output old-result.json.gz
```

Re-render/diff retained data instead of rerunning it. There is no automatic garbage
collector: failed attempts and promoted evidence require deliberate retention
management. Compression uses the Rust backend of flate2; no C library is needed.

## Measured size check

The retained two-arm, 200-round audit packet was 69,511 bytes as pretty JSON.
At the storage-only revision, the lossless archive was 5,915 bytes and its
selected Git snapshot was 318 bytes.
Python's independent gzip/JSON decoder verified every historical field and all
200 raw rounds. These are this fixture's measured sizes, not a universal ratio.
See [the size record](../benchmarks/compact_storage_2026-10-11.json).

JSON float round-trip parsing is enabled: the independent check caught one-bit
changes in three historical floating-point fields with the prior parser settings.
The regression preserves those exact floating-point values through load/archive.

## Analysis and baseline references

`zenbench reanalyze INPUT --output analysis.json` recomputes the recorded fixed
process policy from raw rounds. The separate artifact includes its policy version,
evidence hash, validity issues and decisions. It does not change measurements or
turn exploratory measurements into confirmation.

Named baselines are references with an absolute data-root locator. Earlier versions
remain under `.zenbench/baselines/history/`; deleting a name leaves history/raw
objects intact. When moving evidence between machines, copy the objects and
update the locator, or use a portable `.json.gz` packet directly. The low-level
save API stores descriptive populations; CLI promotion applies the mode contract.

The final mode implementation's two-arm Confirm fixture retained five processes
and 150 rounds: **146,109 bytes** as pretty JSON, **9,729 bytes** as gzip, and an
**874-byte** selected Git snapshot, including context and process interval. The
separate reanalysis was 532 bytes. Independent Python/SciPy replay checked the
complete round trip and reconstructed interval. See the
[measured sizes/times](../benchmarks/modes_2026-10-11.summary.json) and
[raw evidence pointer](../benchmarks/modes_2026-10-11.pointer.md). This is one fixture;
size depends on the selected benchmark inventory and retained metadata.
