+++
title = "CI Regression Testing"
weight = 2
+++

Use explicit `--mode=confirm --workload-id=inputs-v3` with registered native Suite
groups. Iterate is the local exploratory default and cannot promote CI baselines.

```sh
cargo bench -- --mode=confirm --workload-id=inputs-v3 --save-baseline=main
cargo bench -- --mode=confirm --workload-id=inputs-v3 --baseline=main --update-on-pass
```

Confirmation uses fixed fresh processes, verified CPU admission and retained raw
evidence. Exit 1 means confirmed regression. Exit 2 means inconclusive or invalid
evidence. Promotion requires non-inferiority for every required case; noisy,
missing or incompatible measurements cannot replace the baseline.

See the [complete regression guide](https://github.com/imazen/zenbench/blob/main/REGRESSION-TESTING.md),
[mode contract](https://github.com/imazen/zenbench/blob/main/docs/benchmark-modes.md)
and [compact storage guide](https://github.com/imazen/zenbench/blob/main/docs/result-storage.md).

Named references and complete raw objects must both survive CI jobs. Export one
selected CSV to a stable Git path; do not commit each process's result file.
Immediate-mode criterion compatibility results do not support confirmation.
