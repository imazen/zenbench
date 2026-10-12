# Optimizing archmage/magetypes programs

Run `zenbench guide --topic simd` for the offline checklist. This workflow draws
on the retained zenav1-svt probes linked below. Their measurements are historical
experiments, not predicted gains for your kernel. Use the dependency versions in
your lockfile; token and macro behavior evolves.

## Decide which cost you are measuring

Keep these questions in separate benchmark groups:

| Question | Baseline and candidate must both include |
|---|---|
| Does a kernel transformation help? | The same already-held token and entry boundary |
| Does dispatch hoisting help the real caller? | Their actual public entry paths, with the same operation and batch size |
| What does a public operation cost? | Dispatch, allocations and setup that callers pay on every operation |
| Does the application improve? | The full production path on representative inputs |

Acquiring a token before timing one arm while summoning it inside the other
confounds dispatch and kernel changes. That comparison can answer a caller-level
question, but cannot isolate vector arithmetic or abstraction overhead. The
[zone-2 probe][zone2] explicitly records this limitation; the
[transpose probe][transpose] holds the same V3 token on both sides.

## Compile-time features, runtime summoning, and held tokens

These are three distinct mechanisms:

1. **Global compile-time feature guarantee.** When the compiled target enables
   all required features, the normal token `summon()` fast path can return a proof
   without runtime detection. `compiled_with()` describes that compile-time
   configuration. This changes the binary's minimum CPU requirements. Archmage's
   `testable_dispatch` configuration suppresses this fast path for testing.
2. **Runtime dispatch in a portable build.** With runtime detection enabled,
   `summon()` checks support and caches detection. A warmed call can still load
   that cache and branch. First detection and repeated cached checks are different
   costs. Normal benchmark warmup exercises cached behavior; measuring cold
   detection needs fresh processes with a controlled pre-call path, not just a
   group-level cold-start flag. Without archmage's `std` feature, runtime detection is unavailable;
   do not assume every tier supported by the host will be selected.
3. **Already-specialized context.** Dispatch once at a useful operation boundary,
   then carry the token through helpers. Current tier macros can rewrite nested
   `incant!` calls to direct same/lower-tier calls; an attempted upgrade can still
   require runtime summoning. This is macro/context specialization, not a promise
   that any arbitrary `summon()` call inside any function disappears.

These distinctions are visible in the current [generated token implementation][tokens]
and [nested-dispatch rewriter][rewrite]. A zero-sized token alone says nothing
about the resulting call graph, loads, spills or dispatch checks.

Measure the portable deployment configuration with its real runtime dispatch.
Do not add `-C target-cpu=native` to make a benchmark look cheaper: it can remove
the dispatch path you meant to measure. A deliberately fixed-target deployment
is a separate configuration with its own applicability. Inspect Cargo config,
`RUSTFLAGS`, features, LTO and PGO settings for both builds, and record them.

At the code level, use an `#[arcane]` entry where a baseline caller enters
feature-enabled code and `#[rite]` for appropriate inner helpers. A
`#[magetypes]` kernel already generates tier entries; do not add duplicate wrappers
around it. Keep the useful loop inside the specialized region. Plain generic
magetypes helpers need to inline into a caller with the required features to
avoid out-of-line backend operations. Inspect the caller before changing inline
annotations. The guidance for a plain generic helper is not a reason to attach
`#[inline(always)]` to an `#[arcane]`/`#[rite]` function. See the
[magetypes API patterns][magetypes] for your pinned version.

Holding a token does not require inlining an entire transform family. The
[large Hadamard composition][composition] carries one token through sub-transforms;
its recorded disassembly still contains helper calls. Preserve code-size and
instruction-cache checks when considering larger specialization or inlining.

## Differential correctness before differential performance

Freeze the control source or executable. A probe linked to the changing
production library is not a frozen baseline. The [composition probe][compose-probe]
was rerun with copied baseline source for exactly this reason.

Check candidate output against an independent oracle and the old implementation
over the operation's declared domain. Include short lengths and vector tails,
padded strides, input offsets, output sentinels, signed extrema, overflow/wrapping,
rounding and narrowing. For codecs, also compare complete encoded/decoded outputs
at the required fidelity. A faster wrong result fails the experiment.

Match the oracle to the contract: the Hadamard work found that scalar and AVX2 C
paths differed in wrapping and coefficient ordering. A coefficient multiset
comparison was justified there only because the consumer used a permutation-
invariant reduction; it is not sufficient for an API promising positional output.
Its exact-position Rust comparison and output sentinels remained separate checks.

Exercise dispatch as well as directly callable tiers. Consume the token
permutation report and record missing/compile-time-fixed coverage; do not count
unsupported tiers as passing. Process-wide token disabling needs the testing
API's serialization or isolated test processes. Never run token-permutation
tests concurrently with measurements. See [archmage's testing implementation][testing].

## Inspect the actual code with cargo-show-asm

The package is **cargo-show-asm**; the command is **`cargo asm`**. Use its own
[documentation][asm] and `cargo asm --help`. Syntax below was checked with locally
installed 0.2.62; `PACKAGE`, `BENCH` and symbol names are placeholders.

```sh
cargo asm --version
cargo asm -p PACKAGE --bench BENCH --profile bench
cargo asm -p PACKAGE --bench BENCH --profile bench --asm 'kernel_v3'
cargo asm -p PACKAGE --bench BENCH --profile bench --rust --context 2 'kernel_v3'
cargo asm -p PACKAGE --bench BENCH --profile bench --llvm 'kernel_v3'
```

Match target/features/profile/flags to the measured build. Start with the symbol
listing, then choose the monomorphization actually called. The tool can return
a nonzero status while listing ambiguous matches; select the printed function
and index rather than treating that listing as an assembly capture. A generic library
function may exist only in a concrete executable or have been inlined into its
caller. A missing standalone symbol does not mean the work disappeared. Inspect
the containing caller; do not distort the measured build with `inline(never)`
just to make a symbol easy to find.

For an immutable retained executable, avoid recompiling a different artifact:

```sh
cargo asm --file ./builds/before/kernels --disasm 'kernel'
cargo asm --file ./builds/after/kernels --disasm 'kernel'
```

File disassembly requires cargo-show-asm built with its `disasm` feature and an
appropriate unstripped/symbol-bearing artifact. The [SVT tooling record][tooling]
used this path to inspect a saved binary. Preserve binary hashes with assembly
captures. Compare equivalent symbols from the same target/compiler/profile;
do not interpret address/register renaming alone as an optimization.

Look at the complete load → transform → shuffle → widen → reduce → store path:

- Did repeated runtime cache checks or dispatcher calls leave the hot loop?
- Did feature boundaries prevent useful inlining or register reuse?
- Did vector operations become scalar calls, stack spills or repeated reloads?
- Are bounds checks, panic paths, conversions or transposes dominating?
- Did specialization remove a branch but multiply code size?
- Does the short-input/tail path still avoid work that only pays off for large inputs?

Assembly explains a hypothesis; instruction count and `llvm-mca` analysis do not
establish elapsed time on the real CPU. Preserve arithmetic semantics while
changing the implementation, especially fused arithmetic, wrapping and narrowing.

## Isolate a cause, then check the application

Use paired **A/A** controls (the same implementation in both slots) alongside
**A/B** experiments. Retain any control bias; do not subtract it automatically or
discard inconvenient controls. The [transpose record][transpose] reports that
its frozen baseline's timing changed between binary layouts. The
[composition record][composition] retains a biased control rather than hiding it.

Change one mechanism at a time: dispatch placement, transpose, constant
specialization, buffer shape, or allocation strategy. Test tiny inputs and the
large hot path separately. The [SATD probe][satd] found a small-input dispatch
regression and checked where the vector loop began. The [DCT64 probe][dct64]
separately tried constant specialization, fixed buffers and returned arrays;
not every plausible source-level simplification helped.

For Linux differential profiles, a command template for two retained executables is:

```sh
perf record -o before.data -e cycles:u --call-graph dwarf -- ./builds/before/kernels
perf record -o after.data -e cycles:u --call-graph dwarf -- ./builds/after/kernels
perf diff before.data after.data
```

Use fresh output paths and pass the same workload arguments to both programs.
Set up the verified CPU boundary before collecting. Hardware-event access depends
on host permissions; a failed collection is not an empty valid profile. Preserve
symbols and both binaries. This is a diagnostic collection separate from native
confirmation; profiler overhead and call-stack collection affect the run.
Keep input, sample event, collection window and instrumentation identical. Compare call paths and absolute work/time as well as
sample shares. A function's percentage can rise because another function got
faster; inlining can move samples into its caller. The [SVT profile record][composition]
explicitly separates whole-process samples (including construction/teardown)
from its internal frame timing. Never convert a hotspot percentage into a
predicted application speedup.

Finally run the full operation against a retained control executable, check
output identity, and include A/A controls. The [retained-binary driver][pairs]
records executable/input hashes and realized shuffled order; its historical
quartile report is not zenbench's fixed-process confirmation policy. Do not copy
its old thread-affinity workaround as a substitute for today's hard CPU admission.

For zenbench, use Iterate to explore; freeze the winner and plan before Confirm.
Follow the [retained-build workflow](benchmark-workflows.md#source-revisions-and-retained-executables).
Zenbench's `compare` reads evidence packets; it does not orchestrate randomized
two-binary blocks. Keep per-CPU/core-class results separate, and measure the
actual SIMD tiers on their intended hardware. Recheck the end-to-end build with
matching PGO training/held-out inputs if PGO is part of deployment. An isolated
kernel gain is not a frame, codec or fleet gain.

## Keep the evidence compact and attributable

Keep the hypothesis, exact source/binary identities, build configuration, tested
domain/tier coverage, A/A and A/B packets, selected assembly/profile artifacts,
and the end-to-end decision. Commit a selected small snapshot and pointers;
archive full evidence outside Git as described in [result storage](result-storage.md).
Record optimization build time and text size separately from runtime: compile-time
token resolution is not a measurement of how long compilation takes.

The source links below pin the reviewed revisions rather than a moving baseline.
No new speedup or overhead measurement is claimed by this guide.

[zone2]: https://github.com/imazen/zenav1-svt/blob/224c6bbb9a3dc577c64f6e0531bda5c3f60c6b0b/rust/tools/perf_profile/dr_zone2_probe/README.md
[transpose]: https://github.com/imazen/zenav1-svt/blob/224c6bbb9a3dc577c64f6e0531bda5c3f60c6b0b/rust/tools/perf_profile/hadamard_transpose_probe/README.md
[compose-probe]: https://github.com/imazen/zenav1-svt/blob/224c6bbb9a3dc577c64f6e0531bda5c3f60c6b0b/rust/tools/perf_profile/hadamard_compose_probe/README.md
[composition]: https://github.com/imazen/zenav1-svt/blob/224c6bbb9a3dc577c64f6e0531bda5c3f60c6b0b/rust/docs/STILL-PERF-2026-09-06.md#held-token-large-hadamard-composition
[satd]: https://github.com/imazen/zenav1-svt/blob/224c6bbb9a3dc577c64f6e0531bda5c3f60c6b0b/rust/tools/perf_profile/satd_probe/README.md
[dct64]: https://github.com/imazen/zenav1-svt/blob/224c6bbb9a3dc577c64f6e0531bda5c3f60c6b0b/rust/tools/perf_profile/dct64_probe/README.md
[pairs]: https://github.com/imazen/zenav1-svt/blob/224c6bbb9a3dc577c64f6e0531bda5c3f60c6b0b/rust/tools/perf_profile/still_pairs.py
[tooling]: https://github.com/imazen/zenav1-svt/blob/224c6bbb9a3dc577c64f6e0531bda5c3f60c6b0b/rust/docs/STILL-PERF-2026-09-06-history-1.md#tooling-and-api-evidence
[tokens]: https://github.com/imazen/archmage/blob/dea3a46185aa06ff827d284a10027722fb23d17c/src/tokens/generated/x86.rs
[rewrite]: https://github.com/imazen/archmage/blob/dea3a46185aa06ff827d284a10027722fb23d17c/archmage-macros/src/rewrite.rs
[testing]: https://github.com/imazen/archmage/blob/dea3a46185aa06ff827d284a10027722fb23d17c/src/testing.rs
[magetypes]: https://github.com/imazen/archmage/blob/dea3a46185aa06ff827d284a10027722fb23d17c/magetypes/README.md
[asm]: https://github.com/pacak/cargo-show-asm
