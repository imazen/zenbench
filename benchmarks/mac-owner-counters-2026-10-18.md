# Mac owner CPU counters

A protected internal IQA trace with no external observer recorded these raw
observations for one unchanged owner PID. The counters are CPU milliseconds;
wall intervals use monotonic refresh timing. These are resource observations,
not qualified metric timings.

| CPU before | CPU after | Wall ms | Reported percent | Counter percent | Global fraction | Historical foreign fraction |
|---:|---:|---:|---:|---:|---:|---:|
| 1490 | 2353 | 202.086 | 10.355022 | 427.045911 | 0.485797819 | 0.477168633 |
| 2353 | 3242 | 208.526 | 10.665134 | 426.325734 | 0.497232656 | 0.488345044 |

The percentages temporarily undercount while cumulative CPU advances. The
private Mac correction computes process rates from the cumulative counters
across monotonic sample intervals, binding history to PID and process start
time. All processes use that rate when history is valid, retaining foreign
busy-process detection. Missing owner history subtracts zero; unknown foreign
history retains the raw percentage and global CPU load. No gate changes or
public API additions are involved.

The mechanical controls replay the first row and assert that real foreign busy
work still exceeds the unchanged gate and remains a heavy process. PID reuse,
counter reset, missing observations and stale zero-delta counters have separate
controls. Existing platform controls remain unchanged. Native corrected IQA
worker qualification needs a new binary hash and frozen bundle; historical
failed controls remain preserved.

Raw evidence: `~/tmp/iqa-dvifmish-mac/internal-protected-repeat.noindex/3-000095-7e84fe06110e505f/`
on mac, files `internal.jsonl`, `stdout.log`, `identity.json`. The diagnostic
worker SHA-256 is `9ea9b1ec685cc0d9f21017adbf23e0fa7ad048b7c065d9865213472d96819c89`.
