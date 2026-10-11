# Regression testing with zenbench

Use Iterate for local edits and Confirm for a frozen candidate. A successful
exploratory run cannot update a CI baseline. Start with registered native `Suite`
groups; immediate-mode criterion compatibility results cannot confirm.

```sh
# Establish a retained baseline population on the selected testbed.
cargo bench --bench kernels -- --mode=confirm --workload-id=inputs-v3 --save-baseline=main

# Compare the candidate against every required case.
cargo bench --bench kernels -- --mode=confirm --workload-id=inputs-v3 --baseline=main --max-regression=1

# Replace the named reference only after non-inferiority is established.
cargo bench --bench kernels -- --mode=confirm --workload-id=inputs-v3 --baseline=main --update-on-pass
```

Choose verified CPU placement first on asymmetric systems. Baseline and candidate
must have compatible testbeds, placement, workload identities and sampling
settings. Historical measurements cannot establish a contemporaneous randomized
A/B comparison. `self-compare` retains both result packets under the durable data root and remains
an exploratory convenience; it does not
supply a validated randomized-block confirmation protocol.

Exit 0 means every compared case establishes improvement, equivalence or
non-inferiority within the margin. Exit 1 means confirmed regression. Exit 2 means
inconclusive/invalid evidence or a configuration/storage error; other runtime
failures are also nonzero. A wide interval cannot pass simply because it fails
to detect a regression. Missing, additional and empty benchmark selections cannot
promote. Initial baseline creation requires complete confirmation evidence but
does not claim improvement over another build.

The [mode contract](docs/benchmark-modes.md) documents process counts, fixed
sampling, confidence/margin settings, resource admission, statistical assumptions
and practical limits. It is the authoritative reference for CI decisions. The
old descriptive `regressions`/`improvements` fields are not the acceptance policy;
library consumers use `BaselineComparison::confirmed`, `validity_issues` and
`promotable`.

## Retention and Git

Named baselines are small references under `.zenbench/baselines/`. Prior versions
remain under `history/`; complete gzip packets live in `.zenbench/data/objects/`
or `ZENBENCH_DATA_DIR`. The baseline reference records the data root, so moving
it to another machine requires restoring the referenced objects and adjusting
that location. Cache/archive both references and objects in CI. No automatic
upload or garbage collection is performed.

For version control, explicitly export a selected stable CSV file:

```sh
zenbench snapshot .zenbench/baselines/main.json --output benchmarks/baseline.csv
zenbench compare baseline.json.gz candidate.json.gz
zenbench reanalyze candidate.json.gz --output analysis.json
```

Snapshots contain one row per benchmark and a full-evidence hash. Keep raw
packets in an artifact store; do not commit each process or each exploratory run.
Offline `compare` uses the same 0/1/2 acceptance statuses; legacy/exploratory data
can be displayed but cannot return a confirmed pass.
See [storage and portability](docs/result-storage.md).

```sh
zenbench baseline list
zenbench baseline show main
zenbench baseline delete old
```

Deleting/pruning a named reference does not garbage-collect historical evidence.
