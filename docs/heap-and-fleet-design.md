# IQA heap measurement and fleet integration

Status: proposed implementation contract, based on source review on 2026-10-12.
No peak-heap backend, deployment bundle command, or benchmark fleet job is added
by this document. The existing timing, pair and cooperative-admission commands
remain available. See [the methodology](../METHODOLOGY.md) for timing inference.

## What exists

Zenbench's [allocator profiler](../src/alloc.rs) records thread-local allocation
traffic. It does not track concurrent live bytes or opaque Rayon workers. Its
bytes-per-iteration result is not peak heap. Instrumentation contributes to
timed execution when installed; a universal overhead number is not established.

At zenmetrics revision `2f2c113527f08847a517f91d6e17e7f8e34dbe76`, the
[CPU measurement script](https://github.com/imazen/zenmetrics/blob/2f2c113527f08847a517f91d6e17e7f8e34dbe76/scripts/cpu_metrics_1t8t.sh)
runs each selected large-image metric under `/usr/bin/time -v`. Its
[driver](https://github.com/imazen/zenmetrics/blob/2f2c113527f08847a517f91d6e17e7f8e34dbe76/benchmarks/heaptrack/drivers/cpu_profile/src/bin/new_metrics_wall.rs)
constructs inputs and performs two calls in that process. The
[analyzer](https://github.com/imazen/zenmetrics/blob/2f2c113527f08847a517f91d6e17e7f8e34dbe76/scripts/cpu_metrics_1t8t_analyze.py)
reports the process maximum RSS. This does not separate cold and warm heap peaks
or isolate the metric's live allocations from input preparation.

## Name each memory measurement

| Quantity | Meaning | Important limits |
|---|---|---|
| Requested live heap peak | Maximum sum of live requested allocation sizes in one observed event stream | Excludes allocator metadata, fragmentation, stacks, direct mappings and device memory unless explicitly intercepted |
| Allocation traffic | Total successful requested bytes and allocation operations | Repeated allocate/free cycles increase traffic without increasing the live peak |
| Process resident high-water mark | OS-recorded peak resident memory for the child | Includes more than heap; has platform-specific semantics and usually a process-lifetime scope |
| Private committed / job memory | Named Windows process or Job accounting counter | Must retain its actual counter name; not interchangeable with RSS or requested heap |
| Cgroup memory peak | Linux cgroup accounting peak, including descendants | Can include file cache and kernel memory; the supervisor must be outside the measured group |
| GPU memory | Device allocations/reservations reported by a declared backend | Separate from host heap, including on unified-memory systems; avoid double-counting overlapping accounting domains |

Record bytes as integers with backend, version, coverage, scope and provenance.
Unavailable means null plus a reason, never zero. Sampled RSS is a sampled lower
bound on the peak; prefer an OS high-water counter for that quantity. Do not sum
independent thread/process peaks: they may occur at different times. Do not
subtract two process-lifetime high-water marks to manufacture a phase peak.

The Linux [cgroup documentation](https://www.kernel.org/doc/html/latest/admin-guide/cgroup-v2.html)
and Windows [process memory documentation](https://learn.microsoft.com/en-us/windows/win32/psapi/process-memory-usage-information)
define different accounting quantities. Preserve those distinctions in fleet tables.

## Measurement protocol

Use separate runs for native latency and memory profiling. The memory run uses
one metric, one input pair, one mode and one thread configuration per fresh child.
It performs a declared number of operations, rather than timing calibration or
adaptive batching. Never divide a peak by an iteration count. Retain repeated
process peaks individually and report their range/maximum; the observed maximum
is not an upper bound on every possible input or thread interleaving.

Provide two named views:

1. **End-to-end:** start the child, prepare inputs and pools, construct any
   reference representation, score, and release resources. Retain the whole-child
   heap and OS peaks. This describes the deployment envelope of the declared task.
2. **Prepared-input operation:** identify input and pool preparation separately,
   then measure reference preparation, the score call, and retained state with
   explicit phase boundaries. Report absolute live bytes at entry, phase peak,
   and live bytes at exit. If reporting `phase_peak - entry_live`, label it as
   additional live memory above entry, not ownership-attributed metric scratch.
   Freeing pre-existing memory can mask subsequent allocations in this difference.

Cold construction and warm reference reuse need distinct cases. The warm case
must retain the reference/pool state it claims to reuse, record that persistent
cost, and begin a new phase watermark after warmup. A whole-process RSS peak
cannot be reset into a warm-only measurement by subtracting an earlier reading.
Decode/color conversion and output/diffmap retention are explicitly included or
excluded. Join work before ending a phase; a returned future is not completion.

For IQA, declare width, height, stride, pixel representation, color/transfer
contract, content/input hashes, metric configuration, reference reuse, output
mode, actual thread count and effective SIMD tier. Measure single-threaded and
production parallel cases. Cover representative sizes and contents without
extrapolating bytes from one resolution; include odd dimensions and strided input.
Validate the metric result against its declared correctness contract in both
latency and memory builds.

## Heap backends and acceptance gates

Start with a Linux external [heaptrack](https://github.com/KDE/heaptrack) adapter
for whole-child requested-heap evidence and call stacks. Retain the trace outside
Git, its hash, command and profiler identity. Preserve optimized SIMD paths and
debug symbols. Verify interception for the actual allocator/build; direct mappings,
custom arenas and device allocations require separately declared coverage.
Do not claim phase-specific heap from a whole-process summary. Phase integration
needs its own tested protocol.

A portable Rust-allocation backend would observe all threads at the global
allocator boundary from process startup. It needs a coherent process-wide live
ledger, phase high-water marks and explicit realloc/failure semantics. Never
reset the live count while older allocations remain; cross-thread frees must
update the same ledger. Snapshot subtraction of thread-local counters cannot
recover simultaneous peak liveness. An arbitrary set of relaxed counters cannot
establish a coherent live/peak pair without an ordering argument.

Even a correctly ordered allocation-boundary ledger measures the instrumented
execution and requested sizes. It cannot see a wrapped allocator's temporary
old/new storage overlap inside realloc, nor promise an unperturbed physical peak.
Serialization and tracing can change scheduling and therefore peak overlap.
Audit a published backend before choosing it; this proposal grants no new unsafe
exception. For example, current
[alloc_tracker process spans](https://docs.rs/alloc_tracker/0.7.12/alloc_tracker/)
explicitly withhold operation peak memory, so that API alone does not satisfy
the process-wide peak contract.

Required controls: a transient allocate/free spike; simultaneous worker allocations;
non-overlapping worker peaks; pre-existing Rayon workers; cross-thread frees;
live allocations crossing phase boundaries; failed allocation/reallocation;
realloc growth/shrinkage; zero allocation; retained reference state; and background
harness activity. Use barriers and exact requested-size oracles, plus an independent
heap profiler. Separately test native/C allocator coverage and explicit unsupported
cases. Profiling duration/overhead and real IQA results have not been measured here.

## Fleet boundary

Zenmetrics already owns
[job kinds and capability routing](https://github.com/imazen/zenmetrics/blob/2f2c113527f08847a517f91d6e17e7f8e34dbe76/crates/zenfleet-core/src/job.rs),
[content-addressed blobs](https://github.com/imazen/zenmetrics/blob/2f2c113527f08847a517f91d6e17e7f8e34dbe76/crates/zenfleet-core/src/content.rs),
and the [worker/lease/ledger workflow](https://github.com/imazen/zenmetrics/blob/2f2c113527f08847a517f91d6e17e7f8e34dbe76/docs/RUNNING_JOBS.md).
Keep deployment, host inventory, scheduling, credentials and uploading there.
Zenbench should provide a versioned local measurement contract:

- Machine-readable capabilities: executable/build identity, result/plan schemas,
  OS/architecture, topology and placement strength, installed profiler backends,
  supported counter semantics, available admission/memory controls. Unsupported
  capability means not-run with a reason; never silently downgrade.
- A portable logical plan with content-addressed inputs and binaries. Keep its
  experiment fingerprint separate from machine-local resolved paths. Preserve
  the resolved invocation as provenance rather than making deployment directories
  change the intended experiment identity.
- A typed memory packet alongside timing evidence. The existing pair extractor
  normalizes positive durations to ns; bytes and valid zero-allocation results
  must not be disguised as durations to reuse that interface.
- Fixed attempt/replicate IDs, progress/heartbeat events, bounded deadlines,
  cancellation/descendant cleanup and resumable immutable artifacts. Distinguish
  unsupported, busy/not-run, incorrect output, timeout, resource limit, and
  incomplete evidence. Preserve unsuccessful attempts.
- A compact selected table and a hash-addressed evidence manifest, with decode
  and comparison commands. Large heap traces belong in the existing object store.

Zenmetrics needs a benchmark job kind/executor adapter, building on its existing
program/data bundle machinery. Its job identity must include experiment and
planned replicate, requested machine/testbed or placement domain, and measurement
mode/backend. A benchmark's repeated execution is intentional: deduplicating by
program/input alone must not collapse a dozen computers or repeated processes
into one result. Retries get distinct attempt IDs and retain all attempts; a
predeclared rule selects completed evidence, never the fastest retry.

Build once per target/toolchain/configuration and deploy hash-verified binaries,
runtime dependencies and optional symbols. Native Windows ARM64 and emulated x64
are different testbeds. Reserve the benchmark resource domain through the existing
worker scheduler before invoking zenbench; queue only the admitted local run.
Keep worker lease renewal alive while admission waits. Do not let nested wrappers
deadlock their own reservation. Cooperative `nice` alone cannot control unrelated
workers that bypass the same admission directory.

Aggregate within matching workload, hardware/CPU-cache class, build, thread and
instrumentation contracts. Present cross-machine results as distinct strata.
Use the existing ledger for fleet status and one selected campaign table in Git.

## Implementation order

1. Typed memory evidence and a one-case fresh-child runner; whole-child heaptrack
   and correctly named OS peaks first, with failure and ownership controls.
2. Validated all-thread Rust heap tracking and explicit cold/prepared/warm phases.
3. Capability probe, portable plan/bundle binding, progress and attempt contract.
4. Zenmetrics benchmark executor integration and a CPU/OS/thread backend matrix.

The default remains fast uninstrumented iteration. Memory measurement is explicit;
changing a memory profile must not silently change the native latency baseline.
