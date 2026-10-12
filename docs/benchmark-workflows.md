# Did this code edit help?

Start with `zenbench guide` for offline workflow guidance or
`cargo bench --bench kernels -- --help` for benchmark options. Replace `kernels`
and `filter` below with your benchmark target and group. The installed CLI and
the benchmark executable have separate argument parsers: Cargo forwards benchmark
arguments after `--`; a retained executable takes them directly.

A bare `zenbench` invocation lists nine primary task entries and the guide paths.
`data` owns saved evidence; `fleet` owns deployment/observations; `jobs` contains
the older background interface. All previous flat command spellings remain
accepted for scripts. Benchmark options accept `--name=value` or `--name value`.
See [migration and command contracts](cli-migration.md) for the complete mapping.
Benchmark launches also print a short guide hint to stderr before configuration
validation, once per process with supervised children suppressed. Structured
result stdout stays separate; no first-use marker or user configuration is needed.

| Question | Workflow | What it establishes |
|---|---|---|
| Which local edit looks promising? | Iterate; compare arms in one group | Exploratory paired measurements |
| Can this frozen candidate replace a baseline? | Confirm, same cases/inputs/testbed | Non-inferiority within a declared margin, if valid |
| What changed since a source revision? | `self-compare --bench kernels --ref REF` | Builds and sequentially runs both versions; exploratory by default |
| Can I keep two builds and rerun them? | Retain executables, run each, compare packets | Binary hashes distinguish executed artifacts; no automatic A/B scheduling |
| Can I publish a multi-machine result? | Evidence per named cell, caller-managed matrix | Same confirmation protocol for each completed cell, not publication certification |
| Why did memory/instruction behavior change? | Separate profiler runs | Diagnostics; not native wall-clock confirmation |

For archmage/magetypes kernels, see the [SIMD optimization workflow](simd-optimization.md)
and `zenbench guide --topic simd`: token/dispatch boundaries, differential testing,
assembly inspection and lessons from retained zenav1-svt experiments.

## Explore, then freeze and confirm

Verify output correctness before timing. Match real input sizes, distributions,
thread counts, cache conditions and enabled features. When both implementations
can coexist, register them in the same comparison group so rounds interleave.

```sh
cargo bench --bench kernels -- --group=filter
cargo bench --bench kernels -- --mode=confirm --workload-id=inputs-v1 --dry-run
```

The default Iterate mode is intentionally cheap and exploratory. Its successful
exit does not confirm performance. Freeze the candidate, workload, margin and
sampling plan before running confirmation; do not keep sampling until the answer
becomes favorable. A workload ID must change when inputs/configuration change,
but remain the same across implementations of the same workload.

On the control build, then on the candidate respectively:

```sh
cargo bench --bench kernels -- --mode=confirm --workload-id=inputs-v1 --save-baseline=before
cargo bench --bench kernels -- --mode=confirm --workload-id=inputs-v1 --baseline=before
```

Add `--update-on-pass` only when you intend to replace that baseline. Confirm
defaults to five fresh processes with fixed rounds, a 1% margin and 95% family
confidence. Confirmation/comparison exits are 0 for non-inferiority, 1 for
regression, 2 for invalid/inconclusive evidence. Initial baseline creation makes
no relative claim. Invalid evidence needs its reported cause fixed; an
inconclusive interval is not equivalent performance. Retain unsuccessful attempts
and distinguish any newly planned experiment from the original one.

CPU admission comes first: `zenbench cpu-info`, then `zenbench exec --help`.
Use detected core/cache classes, not assumed CPU numbers. Linux needs a hard
cpuset; supported Windows x86/x64 and native ARM64 topology uses a Job boundary.
macOS advisory hints and Windows multiple-group topology cannot currently supply
strict class confirmation. `unrestricted` is an explicit exploratory option. Run different
classes separately. For Rayon-managed work, use `bench()` and configure the pool
size yourself; do not nest it in zenbench's parallel/contended pool APIs.
See [placement](process-affinity.md) and [mode/inference limits](benchmark-modes.md).

## Source revisions and retained executables

```sh
zenbench self-compare --bench kernels --ref HEAD~1 --cargo-args="-- --group=filter"
```

This builds the old revision in a separate Git worktree and the candidate in the
current working tree, including uncommitted changes. Both must have a compatible
zenbench benchmark target. It runs baseline first, candidate second and retains
both packets. It does not randomize order; its default exploratory comparison
returns 2, not a confirmation pass. `--cargo-args` is split on whitespace, not
parsed as a shell command; use direct executable runs for arguments needing spaces.

Keeping immutable executables avoids rebuilding the old candidate during each
edit. Compile using your normal benchmark profile (`cargo bench --bench kernels
--no-run` prints the executable path), then copy the executable to a fresh,
versioned artifact directory outside the replaceable Cargo target tree. Preserve
runtime assets/shared libraries too. On Windows retain the `.exe` and required
runtime files. Use identical working directories and inputs for both versions.

For two already retained zenbench executables, a POSIX-shell example is:

```sh
ZENBENCH_RESULT_PATH="$PWD/before.json.gz" ./builds/before/kernels --mode=confirm --workload-id=inputs-v1
ZENBENCH_RESULT_PATH="$PWD/after.json.gz" ./builds/after/kernels --mode=confirm --workload-id=inputs-v1
zenbench data compare before.json.gz after.json.gz
```

Choose fresh output paths for each attempt. On asymmetric machines launch each
through the same verified `zenbench exec` configuration. In PowerShell, set
`$env:ZENBENCH_RESULT_PATH` before each executable invocation. Retaining executables
does not make arbitrary command-line programs into zenbench benchmarks: these
commands require benchmark binaries using the zenbench protocol.

`zenbench data compare` reads suite packets; it never executes binaries.
For separate programs or one program with different environment variables,
use [`zenbench pair`](external-process.md). It randomizes fresh-process A/B
blocks, retains correctness and A/A controls, and supports checked resume.
Iterate is exploratory. `--mode confirm` evaluates a predeclared fixed-block
contract with A/A equivalence, timing resolution, corrected intervals and strict
resource observations. Use [the external confirmation guide](external-process.md#confirm-a-frozen-experiment)
for the plan fields and acceptance exits. Historical suite populations remain a
separate design from contemporaneous randomized external blocks.

## What identity is actually recorded?

| Identity | Automatically retained today? |
|---|---|
| Executable bytes | SHA-256 in `execution.binary_sha256` for engine runs |
| Current checkout HEAD | Optional `git_hash`, queried at runtime in the working directory |
| Clean/dirty status, changed files, source/patch digest | No |
| Compiler version, target, features, flags, Cargo.lock hash | No complete build manifest |
| Workload identity | Caller-supplied ID, not automatic hashing of arbitrary inputs |
| CPU/testbed, timing backend, plan, rounds and process boundaries | Retained by the current engine/protocol |

A retained old binary run from today's checkout may record today's Git HEAD.
Neither a clean tree nor HEAD alone proves what built that binary. At build time,
keep a sidecar with source revision, clean/dirty state and patch/source snapshot,
compiler/toolchain, target, profile, features, flags, dependency/lockfile hashes,
allocator configuration, and input hashes. Capture it before editing again and
bind it to the executable SHA-256. Keep private source/path details out of public
artifacts. Automatic build-time provenance is not implemented yet.

## Allocation and instruction diagnostics

Enabling `alloc-profiling` makes `AllocProfiler` available; it does not install
the global allocator. Installing it instruments allocation, deallocation and
reallocation paths with thread-local counters. Those updates are included in
timed samples; allocations also store to a shared relaxed atomic activity flag.
That shared write matters when reasoning about concurrent allocation costs.
Their overhead depends on workload/build; no universal measured
cost is established. Keep allocator instrumentation the same across comparison
arms and repeat native timing without it before attributing a speed change.

The counters report counts and allocated/deallocated bytes on the measured
thread. They do not capture external Rayon workers' allocations, stack traces,
or peak live heap. Zero recorded allocations does not prove a threaded workload
is allocation-free.

[Heaptrack](https://github.com/KDE/heaptrack) records allocation events with stack
traces. [iai-callgrind](https://docs.rs/iai-callgrind/latest/iai_callgrind/) provides
Valgrind-based library/binary profiling, including instruction counts. Both are
useful complementary tools; neither is integrated into zenbench. Use separate,
bounded profiling workloads so setup/harness work is understood, retain outputs
with binary/input identity, and keep profiled timing separate from native
confirmation. Instruction counts alone do not establish elapsed-time improvement
on a particular CPU, especially across SIMD implementations.

## Retain evidence without filling Git

Raw packets, child attempts and rounds stay in ignored `.zenbench/data` (or
`ZENBENCH_DATA_DIR`). Read without rerunning, then deliberately export one chosen
stable summary:

```sh
zenbench data read after.json.gz
zenbench data read after.json.gz --format json
zenbench data snapshot after.json.gz --output benchmarks/baseline.csv
```

Commit the selected CSV and build-manifest pointers. Store full gzip evidence
and retained executables in an artifact store with hashes. Back up referenced
objects: references and CSV alone cannot recreate or compare measurements.
Use a distinct named Evidence cell per hardware/workload configuration; do not
pool unlike CPUs/classes as one baseline. See [storage/portability](result-storage.md)
and [CI promotion](../REGRESSION-TESTING.md).
