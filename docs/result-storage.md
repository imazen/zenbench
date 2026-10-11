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

The CSV has one sorted row per benchmark and one header containing the SHA-256
of its complete gzip packet. Export retains that packet before writing the CSV.
Rows contain descriptive mean/MAD/count/scope/reliability; exporting does not
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
- `processes/<experiment>/<index>.json.gz`: subprocess output, retained even if a
  subsequent child fails. Aggregates also retain the original replicate packets.

Writes to ordinary result/snapshot paths use a sibling temporary file and rename;
write failures are errors. Evidence references appear only after the object is
written and synced. Interrupted object creation cannot become a successful
reference. Unknown or corrupt references fail loading; there is no silent fallback.
Loading caps both encoded and decoded packets at 256 MiB. Larger experiments must
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
The new lossless archive is 5,915 bytes; its selected Git snapshot is 318 bytes.
Python's independent gzip/JSON decoder verified every historical field and all
200 raw rounds. These are this fixture's measured sizes, not a universal ratio.
See [the size record](../benchmarks/compact_storage_2026-10-11.json).

JSON float round-trip parsing is enabled: the independent check caught one-bit
changes in three historical floating-point fields with the prior parser settings.
The regression preserves those exact floating-point values through load/archive.
