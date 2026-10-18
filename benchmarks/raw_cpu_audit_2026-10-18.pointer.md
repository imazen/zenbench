# Raw CPU counter diagnostic

**Diagnostic only; this is not optimization acceptance evidence.** The frozen
benchmark includes post-timer CPU-time tracing and additional `/proc` I/O.
It does not explain resource flags in earlier measurements on other hosts.

Measured on native Neoverse-N1 with process boundary CPUs 2–3, group pin CPU 2
and the observer assigned CPU 3: five fresh processes, 30 rounds, ten groups.
The retained output contains 644 raw audit records and 4,500 per-arm timing
samples. All recorded rounds passed the resource gate; gate waits were zero.
The source pair is zenbitmaps `aa017f70` versus `713901cd`, not the later
fixed-pixel-append candidate.

- Executable SHA-256: `ed90dc51af793f0e99c0251df9fa1d7549d5b4a318e60ba1d388c83c43e7d111`.
- Source archive SHA-256: `9b26e935420b52ffaaeb4fe3c8b8fb6b1ee73808bbfd7a47abb33bba38f9b33c`.
- Source includes frozen `0673d448` plus private CPU-time and raw-counter
  instrumentation. This instrumentation was not added to the library.

Artifacts checked on **dev**:

`/mnt/v/output/zenbench/rawcpu-audit-2026-10-18-n1-v1/`

`ROOT_SHA256SUMS.json` inventories 57 retained files, SHA-256
`120a1e1b0d63e5217ba0cb4c6391c848968b7b1aa557aa1c360b09f90d5520ad`.
These include source, executable, full output, replicate records, protocol and
correctness CAS. No R2 or tower mirror has been verified.

The retained protocol and logs contain the invocation and environment. The
resource wrapper reports:

`done rc=1 140s | peak-RSS 0.58GiB | min-avail 14473MiB | peak-load 1.08`

Exit 1 includes inverse candidate-to-baseline comparisons and is not itself an
acceptance verdict. No performance or counter-causation claim is made here.
