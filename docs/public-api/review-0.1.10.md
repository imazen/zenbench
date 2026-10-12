# API and CLI review against 0.1.10

Reviewed 2026-10-12: published zenbench 0.1.10, source revision
`8c642a02df3397f073ad5c004ee943dfa2f4f407`, versus
`9d84e8b24a92f6464050b15c93fd6aeaca107524`.
`cargo read zenbench --path-only` resolved 0.1.10 during this review; its
`.cargo_vcs_info.json` identifies that revision. The
[release page](https://github.com/imazen/zenbench/releases/tag/v0.1.10)
identifies the same baseline. This is an assessment, not approval to remove APIs
or publish a breaking release.

## Compatibility findings

Two executed `cargo semver-checks` configurations found the same two failing
lint categories:

- `constructible_struct_adds_field`: `baseline::BaselineComparison` adds
  `confirmed`, `validity_issues`, and `promotable`. Published downstream struct
  literals and exhaustive destructuring can break. This type was not
  `#[non_exhaustive]`.
- `auto_trait_impl_removed`: `ResourceGate` no longer implements `UnwindSafe`
  or `RefUnwindSafe`. Its new background monitor owns a `JoinHandle`; downstream
  unwind-bound code can break. Restoring the traits requires reviewing the
  invariant, not blindly asserting unwind safety.

Both runs reported 200 passing checks, two failing checks and 58 skipped checks.
The first checked default features plus the new heap feature. The second also
checked `async,cpu-time,criterion-compat,charts`. These were native Linux x86_64
checks; optional WASM and target-specific API compatibility were not checked.
No blanket source-compatibility claim is justified. Prefer repairing these
breaks before deciding that a version break is necessary.

Reproduce against a fetched published source directory:

```sh
cargo semver-checks check-release --baseline-root "$PUBLISHED_SOURCE" \
  --default-features --current-features heap-profiling
cargo semver-checks check-release --baseline-root "$PUBLISHED_SOURCE" \
  --default-features --features async,cpu-time,criterion-compat,charts \
  --current-features heap-profiling
```

No existing benchmark registration method was removed in the compared public
snapshots. Existing result types such as `SuiteResult`, `ComparisonResult`, and
`RoundSample` were already non-exhaustive; their additional fields do not have
the same struct-literal problem. Historical result deserialization uses defaults
for new evidence fields, but historical data does not become confirmed evidence.

## Surface changes

The committed default-feature API snapshots report:

| Category | 0.1.10 | Reviewed main |
|---|---:|---:|
| Public types | 44 | 65 |
| Free functions | 37 | 44 |
| Inherent methods | 173 | 199 |
| Public fields | 249 | 337 |
| Visible top-level CLI commands, excluding generated help | 9 | 26 |

API figures are snapshot-generator counts, including its re-export presentation;
these are surface measurements, not measures of usefulness or correctness.
The CLI count excludes the hidden `cpu-child` implementation command.

The ordinary `Suite` → group → `Bencher::iter` path remains. Additions are:

- Suite mode/plan settings and explicit Iterate/Confirm/Evidence intent;
  execution records, process comparisons and verdicts.
- Suite process policy/set and suite/group thread affinity; platform topology,
  scoped affinity/QoS guards, and observational worker probes.
- Evidence compatibility checks, confirmation diagnostics, snapshots and
  separately saved analysis.
- Optional `HeapProfiler`, `HeapSnapshot`, `HeapPhase`, `HeapMeasurement`, and
  `HeapError`. Dedicated memory profiling remains off by default.

All nine old CLI command names remain. The 17 new names are:
`guide`, `read`, `archive`, `snapshot`, `reanalyze`, `cpu-info`, `exec`, `pair`,
`pair-diff`, `nice`, `capabilities`, `bundle`, `bundle-check`, `fleet-run`,
`fleet-read`, `fleet-diff`, and `fleet-export`.

Benchmark executables additionally accept mode/process/round/margin/confidence,
workload/scope/deadline, CPU policy/set and dry-run options, with corresponding
configuration/environment paths. The bench parser mostly requires `--name=value`;
the installed Clap CLI accepts ordinary separated values. This inconsistency is
visible to users and automation.

## Behavioral migration

- Group defaults change from 200 maximum rounds and 500 ms warmup per arm to
  30 and 100 ms. Iterate remains adaptive/exploratory; Confirm defaults to five
  fresh processes with fixed rounds. These budgets are not wall-time ratios.
- Unknown/asymmetric CPU placement now refuses ordinary execution unless a
  suitable boundary is verified or exploration explicitly opts out. This is
  intentional, but it needs migration guidance, especially on macOS.
- Baseline promotion and saved-result comparison require complete confirming
  evidence. Exit 0/1/2 means accepted/regressed/invalid-or-inconclusive for that
  decision path; an exploratory execution's zero means only that it ran.
- Ordinary automatic storage moves from temporary LLM text to ignored durable
  gzip evidence with content hashes and small selected CSV exports for Git.
- Old and new releases use different lock locations. Running both versions on
  one host does not provide shared mutual exclusion merely by using defaults.

## Design judgment

The strongest future-facing choices are retaining raw evidence and process
boundaries, separating measurement from analysis, versioning CLI data protocols,
keeping fleet implementation types private, and making memory instrumentation
explicit. Most new public types are non-exhaustive. Fleet scheduling stays with
the caller; zenbench runs an observation and returns evidence. These choices
support the existing workflows without inventing a general plugin framework.

The largest problems are consistency and exposed scope:

1. Repair the two source-compatibility failures before release. Add a release
   compatibility gate; existing platform tests do not substitute for it.
2. Group the CLI by task, with fewer primary entry points. Fleet's seven commands
   need not precede the everyday workflow in top-level help. `results` and `read`,
   plus three distinct comparison commands, need clearer routing. Preserve
   aliases if introducing namespaces; do not silently invalidate scripts.
3. Make one documented run/decision contract authoritative. Legacy best/mean/
   median repetition, background `run`, `self-compare`, suite confirmation and
   `pair` have different guarantees. Background `run` does not preserve the
   confirmation exit contract. Default `self-compare` can execute successfully
   then return inconclusive because its results are exploratory.
4. Consider representing Evidence as Confirm plus required scope: its measurement
   protocol is currently identical. A separate mode needs a concrete distinction
   beyond naming. This is a proposed simplification, not an implemented change.
5. Preserve process boundaries and scoped thread affinity as distinct mechanisms,
   but lead users to the process boundary for Rayon. Do not suggest that
   `Suite::affinity` controls pre-existing external pools.
6. Review the five public worker-probe types and raw QoS details before stabilizing
   them. Probes have an actual external-worker use case, but every exposed field
   becomes a maintenance commitment. Avoid adding extension systems or further
   knobs without a consumer.

The immediate functional gap is that `pair` explicitly does **not** implement
external Confirm/Evidence or persistent workers. It supports two binaries and
per-arm environments with A/A and correctness controls, but its results remain
exploratory. That limitation should guide the next implementation priority;
it should not be obscured by matching command vocabulary.

Documentation also needs a consistency pass: `exec --help`,
`src/workflow-help.txt`, and `docs/installation.md` still say Windows ARM class
placement is unsupported at the reviewed revision, although its native backend
and tests have landed. Existing text is recorded here for correction through the
normal documentation review process.

The assessment is: capabilities are mostly justified by current uses; the API
and CLI are not yet minimal or coherent enough to freeze unchanged.

## Cleanup following this review

The compatibility repair restores the published `BaselineComparison` layout and
`ResourceGate` unwind traits. Confirmation now uses a separate non-exhaustive
`BaselineAssessment`; the same expanded-feature semver check passes all 202
applicable checks. CI repeats that published-version comparison.

CLI discovery now has nine primary entries, with data/fleet/jobs families and
all flat commands retained. Benchmark options accept both value spellings.
New guidance recommends Confirm with scope, retaining Evidence's API and recorded
identity. Probe/QoS APIs remain because actual external-worker diagnostics use
them; their observations still do not certify placement. Platform, heap and
pair documentation has been reconciled with the implementations.

See [migration and command contracts](../cli-migration.md). External pair
confirmation remains an explicitly unimplemented capability; this cleanup does
not claim to add a new statistical protocol or change legacy background exits.
