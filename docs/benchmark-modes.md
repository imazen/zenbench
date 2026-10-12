# Iteration, confirmation and evidence

Choose a task in the [workflow guide](benchmark-workflows.md), or run
`zenbench guide` without opening a browser. Benchmark flags are available with
`cargo bench --bench NAME -- --help`.

The default is **Iterate**: one process, 30 maximum rounds per group and 100 ms
warmup per arm. Explicit group settings still apply. Results are exploratory;
within-process intervals do not authorize CI acceptance or baseline promotion.
CPU admission remains strict about asymmetric or unknown topology in every mode.
An explicit `unrestricted` CPU policy permits exploratory measurements only.

```sh
cargo bench --bench kernels -- --group=filter
cargo bench --bench kernels -- --mode=confirm --workload-id=images-v3 --dry-run
cargo bench --bench kernels -- --mode=confirm --workload-id=images-v3
cargo bench --bench kernels -- --mode=evidence --workload-id=images-v3 --scope=linux/performance/filter
```

Confirm launches five fresh processes by default, with a fixed round count in
each. It retains every process and every round. Evidence runs the same protocol
for one explicitly named matrix cell; the caller owns the hardware/workload
matrix, correctness controls and coverage. It does not automatically run every
CPU class, upload results or schedule a fleet.

| Setting | CLI | Environment | Default |
|---|---|---|---|
| Intent | `--mode=iterate\|confirm\|evidence` | `ZENBENCH_MODE` | iterate |
| Fresh processes | `--processes=N` | `ZENBENCH_PROCESSES` | 5 for confirmation |
| Round override | `--rounds=N` | `ZENBENCH_ROUNDS` | group's setting, initially 30 |
| Practical margin (%) | `--margin=N` | `ZENBENCH_MARGIN` | 1 |
| Family confidence | `--confidence=N` | `ZENBENCH_CONFIDENCE` | 0.95 |
| Input/workload identity | `--workload-id=ID` | `ZENBENCH_WORKLOAD_ID` | required for confirmation |
| Evidence matrix cell | `--scope=ID` | `ZENBENCH_SCOPE` | required for Evidence |
| Supervisor deadline (seconds) | `--timeout=N` | `ZENBENCH_TIMEOUT` | 300 |

CLI overrides environment, which overrides `Suite::plan()` settings. For example:

```rust
let mut suite = zenbench::Suite::new();
suite.mode(zenbench::BenchmarkMode::Confirm);
suite.plan().workload_id = Some("corpus-v3/config-v2".into());
suite.plan().processes = 7;
suite.plan().margin_pct = 0.5;
```

CPU policy and CPU set follow the same precedence. Group thread-affinity
constraints must fit inside the selected process boundary. See
[CPU placement](../README.md#cpu-placement-and-affinity). Workload IDs are supplied
by the benchmark owner: include input/configuration changes in the ID. The
harness cannot discover changes hidden inside arbitrary closures.

Unknown flags, malformed settings and conflicting legacy repetition flags fail.
`--dry-run` prints the resolved plan, group inventory/settings, CPU placement,
executable SHA-256 and data directory. Registration code has already run, but no
benchmark callback runs. It prints no invented runtime estimate. Warmup/sampling
budgets are not measured calendar time. Confirm's deadline covers supervised
children, including a blocked callback; it does not terminate detached descendant
processes or include Cargo compilation. Iterate's group wall limit is checked
between rounds and cannot interrupt a blocked callback.

## Decisions and promotion

Confirm/Evidence require complete fixed rounds, at least three fresh processes,
matching inventories/settings, known uniform placement, retained data and fresh
clean resource observations. Monitoring samples whole-host counters; it does
not prove continuous quietness on the selected cores. Unknown, stale or flagged
observations cannot pass confirmation. Immediate-mode criterion compatibility
results lack this evidence and cannot confirm; use registered `Suite` groups.

Each process contributes one mean latency per arm, reconstructed from its raw
rounds without trimming. Same-binary comparisons use a Student-t interval on
process log ratios. Saved baseline comparisons use Welch intervals on independent
process log latencies. Results express a geometric ratio, which differs from the
descriptive arithmetic means. Bonferroni correction covers the comparison family
in that suite; it does not cover exploratory selection across previous runs or
an entire multi-host study. Timer resolution supplies a minimum interval width.

These intervals assume independent, approximately normal process log values.
Fresh execution does not guarantee independence, remove code-layout bias or
eliminate persistent clock/cache interference. Freeze the workload/candidate
before confirmation. Baseline/candidate comparisons refuse overlapping child run IDs: reusing a saved
measurement is not a fresh confirmation population. Historical baselines are not randomized contemporaneous
A/B blocks. A release claim needing those blocks must arrange them separately.

Against a declared margin, the entire interval determines the decision:

- wholly above the positive margin: regression;
- wholly below the negative margin: improvement;
- wholly inside both margins: equivalent;
- upper bound below the positive margin: non-inferior;
- otherwise: inconclusive.

The bench macros return 1 for a confirmed regression and 2 for inconclusive or
invalid confirmation. Runtime failures also return nonzero. Iterate returning
0 means execution succeeded, not confirmed performance. A single-arm confirmation
can retain a baseline population without making a within-suite relative claim.
`--baseline=NAME --update-on-pass` requires non-inferiority for **every** matching
case; empty/missing/added cases, incompatible settings or unverified results cannot
replace the baseline. Initial `--save-baseline=NAME` requires complete confirmation
evidence. The low-level `baseline::save_baseline` API remains descriptive storage;
callers must apply `confirmation_issues` and `baseline::assess_baseline(...).promotable` for
acceptance. Legacy percentage/count fields are descriptive, not CI decisions.

## Validation and limits

The committed [SciPy interval fixtures](../tests/fixtures/process_intervals.json)
check paired and Welch calculations at two comparison-family sizes. A deterministic
[2,000-trial control](../tests/fixtures/process_coverage.json) gives 97 null misses
and 97 known-effect interval misses under independent normal log noise. The +10%
control excludes zero in 1,999 trials. The Rust estimator reproduces those counts.
Correlated process noise yields 1,193 null misses; checking after each of 3–20
processes yields 460. Those excluded scenarios demonstrate why repeated looks or
persistent process correlation cannot inherit the fixed-plan coverage claim.
This is model validation, not proof of coverage for every real workload.

The packet records executable hash, actual timer backend, sampling plan, workload
ID, topology/placement, testbed and observations. Runtime Git metadata is not
build provenance. Compiler flags, source dirtiness, complete power/NUMA state and
opaque pool membership are not automatically certified; retain a build manifest
and workload correctness controls alongside evidence intended for publication.

Raw evidence and selected Git exports are described in
[result storage](result-storage.md). `zenbench reanalyze INPUT --output analysis.json`
recomputes the frozen process policy without executing benchmarks and writes a
separate versioned analysis referencing the immutable packet hash.

## Measured local mode cost

The final two-arm fold control on a Linux Core Ultra 7 265K, Docker cpuset `[0]`,
took 0.766 seconds in Iterate, 3.828 seconds in five-process Confirm and 3.876
seconds in Evidence (one invocation each, compilation/container launch excluded).
Both confirming runs detected the deliberately repeated work. A previous Evidence
attempt was refused after one child's resource observations flagged all 30 rounds;
that attempt remains in the archive. These are harness controls on one host,
not speed predictions for arbitrary SIMD crates or publication-level coverage.

[Measurements](../benchmarks/modes_2026-10-11.summary.json),
[provenance](../benchmarks/modes_2026-10-11.meta.json),
[raw attempts and executable](../benchmarks/modes_2026-10-11.pointer.md).
`run-heavy: done rc=0 11s | peak-RSS 0.10GiB | min-avail 26797MiB | peak-load 1.22`.
RSS covers the Docker client and Python replay; payloads had a separate 2 GiB cap.
