# External process acceptance record: 2026-10-12

The [compact evidence manifest](../benchmarks/pair_acceptance_2026-10-12.json)
records source revisions, executable/input hashes, packet hashes, commands and
measured wall times. The runner was built from `ddfc0034`; the strengthened
independent codec replay audit is `ba4e279d`. These runs validate protocols and
decision plumbing. **They are not codec performance comparisons.**

## Real codec adapters

The decoder uses rav1d-safe `profile_ivf` revision `444cb912`, a 17×13 AV1
10-bit 4:2:0 IVF, and thread counts 1 and 4. Before, after and A/A use the same
binary; the candidate has a different explicit environment label. Four blocks
produce 24 child invocations, each containing three internal timing values. The
checker requires matching positive frame/iteration counts and no decode/flush
errors. This fixture does not establish decoded pixel equivalence.

The encoder pairs zenav1-svt Rust `perf_encode` revision `224c6bbb` with the
pristine C `perf_c_encode` driver from revision `9292ec8e`. The input is a frozen
64×64 gradient, preset 10, QP 40. Four blocks produce 12 invocations including A/A.
The built-in file contract retains SHA-256 hashes of `result.obu`; the independent
replay audit verifies identical nonempty bytes in every block. This is one codec
cell, not a full size/preset/quality grid.

Both runs use Iterate and explicitly unrestricted placement. Their noisy timing
values remain exploratory. The independent Python audit rechecks artifact
hashes, correctness, block membership and mean/log-ratio computations against the
packets. It does not infer correctness from the runner's verdict alone.

Reproduce with prebuilt executables and a new output directory:

```sh
just pair-codec-acceptance PROFILE ENCODE C_ENCODE INPUT NEW_OUTPUT PROFILE_REV ENCODE_REV C_REV
```

The source is [`pair_codec_acceptance.py`](../examples/pair_codec_acceptance.py).
Use the host's resource wrapper for builds and runs. Build these programs with
runtime dispatch rather than `target-cpu=native`; the recorded Rust builds used
thin LTO, and the C driver used `-O2`.

## Native confirmation and busy-system refusal

The [control driver](../examples/pair_control.py) emits synthetic internal values
of 100 for baseline/A/A and 90 for the candidate, identical correctness markers,
and waits 0.6 seconds per invocation. These values are deliberately synthetic:
the control tests resource observation and acceptance, not hardware speed.

On a homogeneous eight-core Neoverse-N1 Linux worker, the five-block Confirm run
established A/A equivalence and accepted the declared improvement with exit 0.
All required resource observations passed. A matched four-block Iterate run
completed with exit 0 and `accepted: false`, as expected for exploratory output.
On the shared Core Ultra 7 265K workstation, all three retained diagnostic attempts
were invalid with exit 2 because of foreign resource activity. No attempt was
silently discarded or retried until accepted; the quiet-worker experiment is a
separate testbed and population.

The shared-host check is a negative control with observed interference. It is
not a controlled sweep of selected-core, SMT-sibling, separate-cache and
memory-bandwidth interference. Zenbench therefore retains conservative whole-host
admission; these observations do not justify reservation-aware relaxation.
Resource sampling also has start/exit attribution gaps and finite observation
windows, described in the [runner contract](external-process.md#resource-and-priority-observations).

## Measured wall time and storage

| Run | Invocations | Elapsed seconds | Gzip packet bytes | Selected CSV bytes |
|---|---:|---:|---:|---:|
| Decoder protocol | 24 | 11.052939175 | 7,932 | 852 |
| Encoder byte identity | 12 | 5.537227016 | 4,420 | 527 |
| ARM synthetic Iterate | 12 | 10.703420015 | 3,316 | 416 |
| ARM synthetic Confirm | 15 | 19.460821696 | 5,588 | 477 |

The last two rows measure this specific 0.6-second child workload, including
monitoring and launch overhead. They do not predict a multiplier for real codecs
or in-process benchmarks. Confirm uses more blocks and during-child monitoring;
Iterate retains its shorter exploratory path.

Resource-wrapper reports for these executions:

```text
codec:      rc=0 17s | peak-RSS 0.06GiB | min-avail 26381MiB | peak-load 0.50
ARM Confirm: rc=0 20s | peak-RSS 0.01GiB | min-avail 15010MiB | peak-load 0.32
ARM Iterate: rc=0 10s | peak-RSS 0.01GiB | min-avail 15013MiB | peak-load 0.06
```

These are wrapper observations, not exact allocator peaks. Raw packets, stdout,
stderr and produced artifacts remain outside Git at the manifest's local paths.
Those paths are evidence pointers for the original checkout, not public downloads.
Git stores this compact manifest instead of thousands of invocation files. Keep
raw evidence in your artifact store and commit selected CSV/manifest records when
reviewing a study. The packet prints its decoding command, and the manifest keeps
both commands:

```sh
zenbench data read PACKET --format json
zenbench data pair-diff BEFORE AFTER
```

## Statistical limits and CI

The [inference document](pair-inference.md) contains the estimator, independent
SciPy numerical oracle, null/known-effect simulations and adverse model results.
In particular, the candidate-specific confounder produced false acceptance:
passing A/A cannot certify an arbitrary comparison. Conclusions remain conditional
on independent, approximately normal block log ratios and the declared timing
error contract. No universal validity under correlated interference is claimed.

[CI run 38214834561](https://github.com/imazen/zenbench/actions/runs/38214834561)
passed for the implementation, including Linux x64/ARM64, Windows x64/ARM64,
macOS Intel/ARM64, i686 and armv7 cross tests, MSRV, lint and published API
compatibility. Native fixtures test deterministic decisions without requiring a
shared CI runner to produce a particular hardware speedup.
