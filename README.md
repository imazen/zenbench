# zenbench

Interleaved microbenchmarking with resource gating, paired statistics, and fire-and-forget mode.

## Why another benchmark harness?

Existing Rust benchmark harnesses (criterion, divan, tango) run benchmarks sequentially.
This means benchmark A runs on a hot CPU after warmup, while benchmark B runs on an even
hotter CPU with potentially degraded turbo boost. System load changes between runs corrupt results.

Zenbench fixes this by **interleaving**: in each measurement round, all benchmarks in a
comparison group run in randomized order. Since round N of benchmark A and round N of
benchmark B execute under near-identical system conditions, paired statistical tests have
far more power to detect real differences.

## How measurement works

Three nested layers: **rounds**, **samples**, and **calls**.

A **call** is one invocation of your function — the thing you're measuring.

A **sample** is a timed batch of calls. Zenbench starts a timer, runs your
function N times, stops the timer, and divides by N. This gives a
per-call time with enough total duration to be above timer noise. N is
auto-selected during warmup to target ~10ms per sample (1 call for slow
functions, millions for sub-nanosecond operations).

A **round** is one measurement of every benchmark in a comparison group.
Each round, zenbench shuffles the order and takes one sample from each
benchmark. Since all benchmarks in a round execute back-to-back under
near-identical system conditions, the per-round measurements form
natural pairs.

The full flow:

1. **Warmup** — Run each benchmark to estimate calls per sample.
2. **Gate check** — Before each round, verify the system is quiet
   (low CPU, enough RAM, cool temperature). If not, wait.
3. **Measure** — For each round (default: up to 200), shuffle
   benchmark order, take one sample from each.
4. **Analyze** — Compute paired statistics on the per-call times.
   Because round N of benchmark A and round N of benchmark B ran
   under the same conditions, paired tests (Wilcoxon signed-rank,
   bootstrap CI) detect differences that unpaired tests would miss.

The iteration count varies ±20% per round (anti-aliasing jitter) to
prevent synchronization with periodic system events like timer interrupts
or scheduler quanta.

## Key features

- **Interleaved execution** — randomized round-robin eliminates thermal, turbo, and load bias
- **Resource gating** — waits for CPU load, RAM, temperature, and process contention to clear
- **Cross-process coordination** — file lock prevents concurrent benchmark processes from corrupting each other
- **Paired statistics** — Welford streaming, bootstrap CI, Cohen's d, Wilcoxon signed-rank test, drift detection
- **Robust metrics** — median, MAD (scaled), and mean/variance for both parametric and non-parametric analysis
- **Anti-aliasing jitter** — varies iteration count ±20% per round to prevent timer synchronization artifacts
- **Cache firewall** — optional L2 cache spoiling between samples for cold-cache measurement
- **Self-compare** — build and benchmark old vs new code via git worktrees
- **Fire-and-forget** — spawn detached benchmark processes, query progress, auto-kill stale runs
- **MCP server** — JSON-RPC 2.0 interface for AI/editor integration
- **CI-aware** — auto-detects GitHub Actions, GitLab CI, etc. and adjusts gate thresholds
- **Cross-platform** — Linux, macOS, Windows (x64 and ARM)
- **`#![forbid(unsafe_code)]`** — no unsafe anywhere

## Cache firewall

Every benchmark harness runs your code in a tight loop that warms up the
branch predictor, instruction cache, TLB, and data cache. The number you
get is a best-case: how fast your function runs when the CPU has been
doing nothing else.

Zenbench's **cache firewall** (off by default) reads a 2 MiB buffer between
benchmarks in each round to evict L2-resident data. Enable it when you want
cold-cache numbers or when benchmarks touch different memory regions and
you don't want cross-contamination.

```rust,ignore
// Cold-cache measurement
group.config().cache_firewall(true);
```

**The firewall is off by default** because most microbenchmarks measure
hot-path code. Pointer-chasing operations (Box, Arc, vtable dispatch) stay
in L1/L2 during a real hot loop — the firewall would penalize them
unrealistically. When the firewall is active, it's reported in the output
header so you can see its effect.

For architectural decisions ("is this fast enough per-request?"), consider
making each iteration large enough that the measurement loop's
microarchitectural warmth is negligible:

```rust,ignore
group.bench("encode_1mp", |b| {
    b.with_input(|| generate_test_image(1024, 1024))
        .run(|img| encoder.encode(&img))
});
```

## Quick start

Add to `Cargo.toml`:

```toml
[dev-dependencies]
zenbench = "0.1"

[[bench]]
name = "my_bench"
harness = false
```

Using the `main!` macro (recommended for `cargo bench`):

```rust,ignore
// benches/my_bench.rs
use zenbench::black_box;

zenbench::main!(|suite| {
    suite.compare("sorting", |group| {
        group.bench("std_sort", |b| {
            b.with_input(|| (0..1000).rev().collect::<Vec<i32>>())
                .run(|mut v| { v.sort(); black_box(v) })
        });
        group.bench("sort_unstable", |b| {
            b.with_input(|| (0..1000).rev().collect::<Vec<i32>>())
                .run(|mut v| { v.sort_unstable(); black_box(v) })
        });
    });
});
```

Or using `zenbench::run()` directly:

```rust,no_run
# fn main() {
zenbench::run(|suite| {
    suite.compare("sorting", |group| {
        group.bench("std_sort", |b| {
            b.with_input(|| (0..1000).rev().collect::<Vec<i32>>())
                .run(|mut v| { v.sort(); v })
        });
        group.bench("unstable_sort", |b| {
            b.with_input(|| (0..1000).rev().collect::<Vec<i32>>())
                .run(|mut v| { v.sort_unstable(); v })
        });
    });
});
# }
```

```console
$ cargo bench --bench my_bench
```

## Group configuration

```rust,ignore
suite.compare("my_group", |group| {
    // Set the baseline benchmark (default: first added)
    group.baseline("reference_impl");

    // Custom throughput unit (default: "ops")
    group.throughput(Throughput::Elements(100));
    group.throughput_unit("checks");  // → "5.0 Gchecks/s"

    group.config()
        .cache_firewall(true)             // enable L2 cache spoiling between benchmarks
        .cache_firewall_bytes(4 * 1024 * 1024) // 4 MiB for larger L2 caches
        .baseline_only(true)              // only compare against baseline (auto for >3 benchmarks)
        .sort_by_speed(true)              // sort table fastest-first (default: definition order)
        .expect_sub_ns(true)              // suppress "optimized away" warnings for sub-ns benchmarks
        .rounds(200)                      // target measurement rounds
        .min_rounds(5)                    // minimum rounds before max_time applies
        .max_time(Duration::from_secs(10)); // max measurement time (excludes gate waits)

    group.bench("reference_impl", |b| { /* ... */ });
    group.bench("new_impl", |b| { /* ... */ });
});
```

Comparison groups with more than 3 benchmarks automatically switch to
baseline-only comparisons to keep output readable. The full pairwise matrix
is always available in JSON output.

## Iteration scaling

Zenbench auto-scales iterations so each sample takes ~10ms. During warmup,
it measures your function and picks an iteration count that fills this
window. You can constrain this with `min_iterations` and `max_iterations`:

```rust,ignore
group.config()
    .min_iterations(1)        // default: 1
    .max_iterations(10_000_000); // default: 10M (high enough for sub-ns ops)
```

For sub-nanosecond operations, the auto-scaler naturally ramps up to
millions of iterations per sample, keeping measurements above timer
resolution. For slow operations (milliseconds+), it drops to 1 iteration
per sample.

If you need manual control over batching — for instance, measuring 100
checks as a single logical operation — use `Throughput::Elements(100)` and
do the loop yourself:

```rust,ignore
group.throughput(Throughput::Elements(100));
group.throughput_unit("checks");

group.bench("check_100x", |b| {
    b.iter(|| {
        for _ in 0..100 {
            black_box(stopper.check());
        }
    })
});
```

The harness runs the outer `iter` loop for timing. Your inner loop of 100
is measured as a batch, and throughput is reported per-element.

## Self-compare

Compare your current code against a previous version:

```console
$ zenbench self-compare --bench sorting --ref v0.1.0
```

This creates a git worktree at the specified ref, builds and runs the benchmark there,
then builds and runs it on current code, and prints a side-by-side comparison. If `--ref`
is omitted, it uses the most recent version tag (`v*`).

## Resource gating

Before each measurement round, zenbench checks:
- CPU utilization (default: wait if >30%)
- Available RAM (default: wait if <512MB)
- CPU temperature (default: wait if >90°C)
- Heavy processes (default: wait if >3 processes using >10% CPU)

```rust
use zenbench::GateConfig;

zenbench::run_gated(
    GateConfig::default()
        .max_cpu_load(0.10)
        .min_available_ram_mb(2048)
        .max_cpu_temp_c(Some(80.0)),
    |suite| {
        // ...
    },
);
```

## Statistics

Zenbench provides multiple layers of statistical analysis:

- **Paired differences** — per-round diffs eliminate system-state confounds
- **IQR outlier filtering** — Tukey's 1.5×IQR fences remove measurement spikes
- **Bootstrap 95% CI** — 10,000 resamples for confidence interval on mean difference
- **Cohen's d** — standardized effect size for practical significance
- **Wilcoxon signed-rank test** — non-parametric p-value, valid for non-normal distributions
- **Spearman drift detection** — flags thermal throttling or systematic load changes
- **Multiple-comparison warning** — Bonferroni correction when groups have many benchmarks

## CLI

```console
$ zenbench list                                  # List all benchmark runs
$ zenbench status <run-id>                       # Check a specific run
$ zenbench kill <run-id>                         # Kill a running benchmark
$ zenbench kill stale                            # Kill runs from old git commits
$ zenbench results latest                        # Show most recent results
$ zenbench results latest --json                 # Machine-readable output
$ zenbench compare a.json b.json                 # Compare two result files
$ zenbench self-compare --bench my_bench         # Compare vs last version tag
$ zenbench self-compare --bench my_bench --ref HEAD~5
$ zenbench clean --max-age-hours 48
```

## MCP server

Zenbench includes an MCP (Model Context Protocol) server for AI assistant and editor integration:

```console
$ zenbench-mcp                    # Start MCP server (stdio)
$ zenbench-mcp --project /path    # Specify project root
```

Available tools: `list_runs`, `run_status`, `kill_run`, `spawn_bench`, `get_results`,
`compare_results`, `clean_runs`.

Configure in your MCP client (e.g., Claude Code `settings.json`):

```json
{
  "mcpServers": {
    "zenbench": {
      "command": "zenbench-mcp",
      "args": ["--project", "/path/to/your/project"]
    }
  }
}
```

## Design principles

Built on lessons from criterion, divan, tango, nanobench, and the Mytkowicz "Producing
Wrong Data" paper:

1. **Interleave, don't sequence.** Same-round pairing eliminates system-state confounds.
2. **Gate, don't hope.** Check system state before measuring, not after.
3. **Pair, don't pool.** Paired statistical tests have more power than independent tests.
4. **Detect drift.** Spearman correlation flags thermal throttling or load changes.
5. **Cache firewall (opt-in).** When enabled, prevents one benchmark from warming caches for the next.
6. **Jitter iterations.** Anti-aliasing prevents synchronization with periodic system events.
7. **Coordinate.** File lock prevents concurrent benchmark processes from fighting.

## License

MIT OR Apache-2.0
