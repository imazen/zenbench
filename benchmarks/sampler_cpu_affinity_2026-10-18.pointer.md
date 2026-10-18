# Sampler-affinity CPU-time diagnostics

**Neither run is valid optimization evidence.** Both completed but were
resource-invalid. These measurements do not establish why earlier CPU-load
flags occurred, or that moving the sampler improves performance.

On Ryzen 5 3500, compare two frozen same-binary farbfeld harnesses:

| diagnostic | executable SHA-256 | sampler CPU | reported gate waits |
|---|---|---|---|
| published 0.1.11 plus post-timer CPU-time trace | `8eb66a34384d2a443bfc732e2910f5fead6087073476098a176b23090497414e` | 2 | 3 |
| 0673d448 plus the same trace | `88ef1815bc64e7e477dcd491b14f2a47b3957a4197d0866ac765ff6fba4209d9` | 3 | 3 |

Both used process boundary CPUs 2–3, group pin CPU 2, five fresh processes,
30 rounds, 1% margin, 95% confidence and all ten workload groups. Each retained
4,500 per-arm wall/CPU-time trace rows. Full byte/spec preflight passed in every
process. The source pair was unchanged: zenbitmaps baseline `aa017f70` and
candidate `713901cd`; it differs from later fixed-pixel-append experiments.
Live `/proc` thread-affinity receipts confirm the sampler placements.

This is not an isolated affinity experiment. The second benchmark dependency
also contains intervening trace, coordination and external CLI lifecycle
repairs. The benchmark callback, timing and statistics source are identical,
but a timing difference could not uniquely be attributed to observer affinity.
Gate-wait counts can repeat an observation; they are not counts of independent
foreign-load events.

Artifacts checked on **dev**:

`/mnt/v/output/zenbench/cputime-matched-2026-10-18-r3500-v1/`

- `ROOT_SHA256SUMS.json`: SHA-256
  `70cdd230ba57a76b35a52eb6c46d4b591dddb7532b7306a7c4d14d332eba1ad9`.
- Includes both executables, both complete source archives, locked dependencies,
  full output, replicate records, 9,000 trace rows, live placement receipts,
  predeclared diagnostic protocol, source-comparison caveat and correctness CAS.
- `ROOT_SUMMARY.json` independently recounts rows, replicates and gate waits.
- No R2 or tower mirror has been verified.

Resource wrappers, both with 6 GiB caps and two jobs:

- Original: `done rc=2 110s | peak-RSS 0.58GiB | min-avail 14021MiB | peak-load 0.79`.
- Observer change: `done rc=2 106s | peak-RSS 0.58GiB | min-avail 13872MiB | peak-load 0.64`.

The source review separately approved observer placement and its required
policy-independent tests; this rejected measurement is not evidence of a
counter arithmetic or threshold defect. A later raw-counter diagnostic has
additional I/O overhead and must also be kept separate from acceptance timing.
