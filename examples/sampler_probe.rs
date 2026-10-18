//! Mechanical resource-sampler diagnostic. Not a benchmark: the owned CPU
//! schedule is a fixed synthetic alternation, and sample timings describe the
//! sampler itself, never product performance.
//!
//! Spawns one same-process worker thread (`sampler-probe-worker`) that
//! alternates between busy-spin and idle phases under control of the main
//! thread. The main thread takes `phases * samples_per_phase` observations of
//! `SystemMonitor::snapshot_excluding_processes(&[current_pid])` — the same
//! owner-subtraction path the native background resource monitor uses
//! (`for_benchmark` also excludes the current process). Every observation is
//! emitted as one JSON line on stdout; there are no skipped samples.
//!
//! With `ZENBENCH_GATE_TRACE` set, `snapshot_excluding_processes` additionally
//! emits the raw sampler arithmetic on stderr (global fraction, logical core
//! count, owned PID CPU percentages with identity and thread kind, top foreign
//! contributors, foreign fraction, refresh intervals), so an external oracle
//! can recompute `foreign = max(global - owned_pct / cores / 100, 0)` for every
//! observation.
//!
//! A foreign sibling process (e.g. a busy loop pinned to a different CPU) must
//! be launched OUTSIDE this process tree. Descendants of the excluded leader
//! are owned by construction, so a foreign child spawned here would be hidden.
//!
//! Usage: sampler_probe [--phases N] [--samples-per-phase K]

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use zenbench::platform::SystemMonitor;

struct Args {
    phases: usize,
    samples_per_phase: usize,
}

fn parse_args() -> Result<Args, String> {
    let mut phases = 6usize;
    let mut samples_per_phase = 4usize;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--phases" => {
                phases = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .filter(|&n| n > 0)
                    .ok_or("--phases requires a positive integer")?;
            }
            "--samples-per-phase" => {
                samples_per_phase = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .filter(|&n| n > 0)
                    .ok_or("--samples-per-phase requires a positive integer")?;
            }
            "--help" | "-h" => {
                return Err("usage: sampler_probe [--phases N] [--samples-per-phase K]".to_string());
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(Args {
        phases,
        samples_per_phase,
    })
}

fn main() {
    let args = match parse_args() {
        Ok(args) => args,
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(2);
        }
    };

    let busy = AtomicBool::new(false);
    let stop = AtomicBool::new(false);
    let monitor = SystemMonitor::new();
    let own_pid = std::process::id();
    let started = Instant::now();

    println!(
        "{}",
        serde_json::json!({
            "record": "meta",
            "argv": std::env::args().collect::<Vec<_>>(),
            "pid": own_pid,
            "excluded_pids": [own_pid],
            "phases": args.phases,
            "samples_per_phase": args.samples_per_phase,
            "total_samples": args.phases * args.samples_per_phase,
            "worker": "same-process thread sampler-probe-worker, owned via leader aggregation",
            "zenbench_version": env!("CARGO_PKG_VERSION"),
            "hostname": std::fs::read_to_string("/etc/hostname")
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|_| "unknown".to_string()),
            "note": "mechanical sampler diagnostic; synthetic phases do not measure product performance",
        })
    );

    let mut seq = 0u64;
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("sampler-probe-worker".into())
            .spawn_scoped(scope, || {
                while !stop.load(Ordering::Acquire) {
                    if busy.load(Ordering::Acquire) {
                        std::hint::spin_loop();
                    } else {
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                }
            })
            .expect("cannot spawn sampler-probe-worker");

        for phase in 0..args.phases {
            let phase_kind = if phase % 2 == 0 { "busy" } else { "idle" };
            busy.store(phase % 2 == 0, Ordering::Release);
            for _ in 0..args.samples_per_phase {
                let call_started = Instant::now();
                let state = monitor.snapshot_excluding_processes(&[own_pid]);
                let call_elapsed_ns = call_started.elapsed().as_nanos() as u64;
                println!(
                    "{}",
                    serde_json::json!({
                        "record": "sample",
                        "seq": seq,
                        "phase": phase,
                        "phase_kind": phase_kind,
                        "t_ms": started.elapsed().as_secs_f64() * 1000.0,
                        "call_elapsed_ns": call_elapsed_ns,
                        "foreign_cpu_frac": state.cpu_load,
                        "heavy_processes": state.heavy_process_count,
                        "available_ram_bytes": state.available_ram_bytes,
                        "cpu_temp_c": state.cpu_temp_c,
                    })
                );
                seq += 1;
            }
        }
        stop.store(true, Ordering::Release);
    });
    println!(
        "{}",
        serde_json::json!({
            "record": "done",
            "samples": seq,
            "elapsed_ms": started.elapsed().as_secs_f64() * 1000.0,
        })
    );
}
