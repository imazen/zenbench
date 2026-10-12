#![forbid(unsafe_code)]

use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use zenbench::daemon;

#[allow(dead_code)]
#[path = "../coordination.rs"]
mod coordination;
#[allow(dead_code)]
#[path = "../paths.rs"]
mod paths;
mod support {
    pub(super) mod fleet;
    pub(super) mod fleet_report;
    pub(super) mod fleet_run;
    pub(super) mod nice;
    pub(super) mod pair;
    pub(super) mod supervisor;
}

#[derive(Parser)]
#[command(
    name = "zenbench",
    version,
    about = "Interleaved microbenchmarking harness",
    after_help = "Start here: zenbench guide (SIMD optimization: zenbench guide --topic simd)
Measurement assumptions and busy systems: zenbench guide --topic methodology
Queue background work: zenbench nice -- cargo build --release
Compare external programs/environments: zenbench pair --help
Measure heap and deploy fleet cases: zenbench guide --topic fleet
Quick edit loop: cargo bench --bench NAME -- --group=GROUP
Before accepting a change: freeze inputs, then use --mode=confirm --workload-id=ID.
Inspect asymmetric CPUs: zenbench cpu-info; choose a verified boundary with exec.
Benchmark-binary options: cargo bench --bench NAME -- --help
Iterate exit 0 means execution succeeded, not a confirmed performance result."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run one planned observation from a verified bundle; retain every attempt.
    FleetRun(support::fleet_run::Options),
    /// Decode a fleet attempt receipt without rerunning work.
    FleetRead { directory: PathBuf },
    /// Compare verified fleet observations descriptively; refuse mismatched contracts.
    FleetDiff { left: PathBuf, right: PathBuf },
    /// Write small JSONL summaries for version control; retain raw evidence separately.
    FleetExport {
        output: PathBuf,
        #[arg(required = true)]
        directories: Vec<PathBuf>,
    },
    /// Machine-readable runner identity, CPU placement and supported protocols.
    Capabilities,
    /// Copy explicitly listed executables, inputs and assets into a verified portable bundle.
    Bundle { spec: PathBuf, output: PathBuf },
    /// Verify every content hash before using a deployed bundle.
    BundleCheck { directory: PathBuf },
    /// Compare prebuilt programs or one program under different environments.
    #[command(after_help = include_str!("../pair-help.txt"))]
    Pair {
        plan: PathBuf,
        #[arg(long, default_value = "iterate")]
        mode: String,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long)]
        case: Option<String>,
        #[arg(long)]
        resume: Option<PathBuf>,
        #[arg(long)]
        dry_run: bool,
        /// Admission wait seconds; default zero reports not-run immediately.
        #[arg(long, default_value_t = 0)]
        wait: u64,
    },
    /// Compare two external-process packets without rerunning or pooling samples.
    PairDiff { before: PathBuf, after: PathBuf },
    /// Queue a background command fairly with benchmarks, then run at low priority.
    #[command(
        after_help = "All participants must use the same ZENBENCH_COORDINATION_DIR for system-wide coordination.
The default directory coordinates one user. Tasks run serially in FIFO order.
Linux requires a systemd user scope for a hard RAM ceiling, with no swap.
Unix commands must stay in the foreground and join their workers; no daemonization.
Unwrapped tasks remain visible to benchmark resource checks.
Example: zenbench nice --jobs 8 -- cargo build --release"
    )]
    Nice {
        /// Maximum queue and resource admission wait in seconds.
        #[arg(long, default_value_t = 300)]
        wait: u64,
        /// Maximum command runtime in seconds.
        #[arg(long, default_value_t = 3600)]
        timeout: u64,
        #[arg(long)]
        jobs: Option<usize>,
        /// Linux hard ceiling; defaults to min(24 GiB, half physical RAM).
        #[arg(long)]
        memory_mib: Option<u64>,
        #[arg(last = true, required = true, num_args = 1..)]
        command: Vec<std::ffi::OsString>,
    },
    /// Show workflows, CPU setup, exit meanings, provenance and profiling limits.
    Guide {
        /// Workflows, SIMD optimization, or measurement methodology and busy systems.
        #[arg(long, default_value = "workflows", value_parser = ["workflows", "simd", "methodology", "pair", "fleet"])]
        topic: String,
    },
    /// Read saved JSON, gzip JSON, or an evidence reference without running benchmarks.
    Read {
        input: PathBuf,
        #[arg(long, default_value = "llm", value_parser = ["llm", "json", "csv", "md"])]
        format: String,
    },
    /// Launch in a verified process CPU boundary. Child processes inherit it.
    #[command(
        after_help = "First run zenbench cpu-info. IDs must belong to one detected core/cache class.
Linux needs a delegated cpuset controller or an existing matching hard cpuset.
Windows uses an inherited Job boundary on supported topology. macOS class binding
and Windows ARM/multiple-group classification are unsupported. Thread hints are
insufficient for existing Rayon pools. Unrestricted permits exploration only.
Example: zenbench exec --cpu-policy performance cargo -- bench --bench kernels -- --mode=iterate"
    )]
    Exec {
        #[arg(long, default_value = "performance")]
        cpu_policy: zenbench::platform::CpuPolicy,
        /// Explicit IDs/ranges, for cache classes without a unique P/E ranking.
        #[arg(long)]
        cpu_set: Option<String>,
        program: PathBuf,
        #[arg(last = true)]
        args: Vec<std::ffi::OsString>,
    },
    #[command(hide = true)]
    CpuChild {
        program: PathBuf,
        #[arg(last = true)]
        args: Vec<std::ffi::OsString>,
    },
    /// Show detected classes and the current process placement check as JSON.
    #[command(
        after_help = "Choose a class before launching libraries or external pools; see zenbench exec --help.
For unequal cache capacities, choose explicit IDs from this output; do not assume
which cache class is faster. Run classes separately and keep results separate.
--probe measures newly created workers; it does not certify existing pools or clocks."
    )]
    CpuInfo {
        /// Exercise new diagnostic workers; this does not certify CPU placement.
        #[arg(long)]
        probe: bool,
        /// Number of diagnostic workers (does not inspect external pools).
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u16).range(1..=128), requires = "probe")]
        workers: u16,
    },
    /// Run a benchmark as a background process.
    ///
    /// Spawns `cargo bench --bench <name>` in the background and tracks it.
    /// Use --wait to block until the benchmark completes.
    #[command(
        after_help = "For an exploratory background run: zenbench run kernels --cargo-args=\"-- --group=filter\"
Track it with list/status/results. Use direct cargo bench or a retained executable
for confirmation exit codes. This wrapper does not preserve the 0/1/2 decision
contract; --wait's timeout does not bound the initial Cargo child wait.
See zenbench guide for confirmation and evidence storage."
    )]
    Run {
        /// Benchmark target name (as in `cargo bench --bench <name>`).
        bench: String,

        /// Project root directory.
        #[arg(short, long, default_value = ".")]
        project: PathBuf,

        /// Wait for the benchmark to complete and print results.
        #[arg(long)]
        wait: bool,

        /// Timeout in seconds when using --wait (default: 600).
        #[arg(long, default_value = "600")]
        timeout: u64,

        /// Extra arguments to pass to `cargo bench` (after --).
        #[arg(long)]
        cargo_args: Option<String>,
    },

    /// List all benchmark runs (active and completed).
    List {
        /// Project root directory (default: current directory).
        #[arg(short, long, default_value = ".")]
        project: PathBuf,
    },

    /// Show status of a specific run.
    Status {
        /// Run ID to check.
        run_id: String,

        /// Project root directory.
        #[arg(short, long, default_value = ".")]
        project: PathBuf,
    },

    /// Kill a running benchmark.
    Kill {
        /// Run ID to kill, or "stale" to kill all stale runs.
        target: String,

        /// Project root directory.
        #[arg(short, long, default_value = ".")]
        project: PathBuf,
    },

    /// Show results of a completed run.
    ///
    /// Use "latest" to find the most recent run with results.
    /// You can also pass a path to a results JSON file directly.
    Results {
        /// Run ID, "latest", or path to a results JSON file.
        run_id: String,

        /// Project root directory.
        #[arg(short, long, default_value = ".")]
        project: PathBuf,

        /// Output as JSON.
        #[arg(long)]
        json: bool,

        /// Output as markdown tables with bar charts.
        #[arg(long)]
        markdown: bool,

        /// Output as CSV.
        #[arg(long)]
        csv: bool,

        /// Save standalone SVG charts to a directory.
        ///
        /// Produces dual-theme SVGs (light/dark via prefers-color-scheme)
        /// with ±MAD error bars, suitable for README embedding.
        #[arg(long, value_name = "DIR")]
        save_charts: Option<PathBuf>,

        /// Save publication-quality SVG charts via charts-rs.
        ///
        /// Requires the `charts` feature. Theme: light, dark, grafana, vintage, etc.
        #[cfg(feature = "charts")]
        #[arg(long, value_name = "DIR")]
        publish_charts: Option<PathBuf>,

        /// Theme for --publish-charts (default: light).
        #[cfg(feature = "charts")]
        #[arg(long, default_value = "light")]
        chart_theme: String,

        /// Use vertical bars instead of horizontal for --publish-charts.
        #[cfg(feature = "charts")]
        #[arg(long)]
        vertical: bool,
    },

    /// Losslessly compact a result packet into gzip JSON, including all rounds.
    Archive {
        input: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
    },

    /// Export one selected result as a small, stable CSV snapshot for Git.
    Snapshot {
        /// JSON, gzip JSON, or an evidence reference from a completed run.
        input: PathBuf,
        /// Destination CSV. Full raw data remains in evidence storage.
        #[arg(short, long)]
        output: PathBuf,
    },

    /// Recompute a versioned process analysis from retained evidence, without measuring.
    Reanalyze {
        input: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
    },

    /// Compare retained baseline and candidate evidence (does not execute code).
    #[command(
        after_help = "Inputs: JSON, gzip JSON, or evidence references; CSV snapshots are summaries only.
Exit 0: every case confirms non-inferiority; 1: confirmed regression; 2: invalid
or inconclusive (including exploratory results). Read exploratory data with read.
Freeze workload, case inventory, sampling plan and CPU placement before measuring.
Use zenbench guide for source-edit and retained-binary workflows."
    )]
    Compare {
        /// Baseline result file.
        baseline: PathBuf,

        /// Candidate result file.
        candidate: PathBuf,
    },

    /// Explore a source edit against a Git ref (builds and runs both versions).
    ///
    /// Builds and runs the specified benchmark at both the old and current
    /// ref and current working tree, including local edits. Requires the benchmark to use
    /// zenbench::main!() so results can be captured via ZENBENCH_RESULT_PATH.
    #[command(
        after_help = r#"Example: zenbench self-compare --bench kernels --ref HEAD~1 --cargo-args="-- --group=filter"
The candidate includes uncommitted changes. Runtime Git HEAD is not build identity.
Runs the baseline then candidate; no randomized two-binary A/B scheduling.
Default exploratory results return 2 from comparison, never a confirmed pass.
Retained executables can be run separately and their packets compared: zenbench guide."#
    )]
    SelfCompare {
        /// Benchmark target name (as in `cargo bench --bench <name>`).
        #[arg(long)]
        bench: String,

        /// Git ref to compare against (tag, branch, or commit hash).
        /// Defaults to the most recent version tag (v*).
        #[arg(long, name = "ref")]
        git_ref: Option<String>,

        /// Extra arguments to pass to `cargo bench`.
        #[arg(long)]
        cargo_args: Option<String>,
    },

    /// Clean up old run metadata.
    Clean {
        /// Project root directory.
        #[arg(short, long, default_value = ".")]
        project: PathBuf,

        /// Maximum age of run metadata in hours (default: 168 = 7 days).
        #[arg(long, default_value = "168")]
        max_age_hours: u64,
    },

    /// Manage saved baselines for CI regression testing.
    Baseline {
        #[command(subcommand)]
        action: BaselineAction,
    },
}

#[derive(Subcommand)]
enum BaselineAction {
    /// List all saved baselines.
    List,

    /// Show details of a baseline.
    Show {
        /// Baseline name.
        name: String,
    },

    /// Delete a baseline.
    Delete {
        /// Baseline name.
        name: String,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::FleetRun(options) => {
            let code = support::fleet_run::run(options).unwrap_or_else(|e| {
                eprintln!("fleet attempt failed: {e}");
                2
            });
            std::process::exit(code);
        }
        Commands::FleetRead { directory } => match support::fleet_run::read(&directory) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(e) => {
                eprintln!("cannot read attempt: {e}");
                std::process::exit(2);
            }
        },
        Commands::FleetDiff { left, right } => match support::fleet_report::diff(&left, &right) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(e) => {
                eprintln!("cannot compare attempts: {e}");
                std::process::exit(2);
            }
        },
        Commands::FleetExport {
            output,
            directories,
        } => {
            if let Err(e) = support::fleet_report::export(&output, &directories) {
                eprintln!("cannot export attempts: {e}");
                std::process::exit(2);
            }
        }
        Commands::BundleCheck { directory } => match support::fleet::verify(&directory) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(e) => {
                eprintln!("bundle verification failed: {e}");
                std::process::exit(2);
            }
        },
        Commands::Capabilities => match support::fleet::capabilities() {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(e) => {
                eprintln!("capability probe failed: {e}");
                std::process::exit(2);
            }
        },
        Commands::Bundle { spec, output } => match support::fleet::bundle(&spec, &output) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(e) => {
                eprintln!("bundle failed: {e}");
                std::process::exit(2);
            }
        },
        Commands::Pair {
            plan,
            mode,
            output,
            case,
            resume,
            dry_run,
            wait,
        } => {
            let result = support::pair::run(support::pair::Options {
                plan: &plan,
                mode: &mode,
                output: output.as_deref(),
                case: case.as_deref(),
                resume: resume.as_deref(),
                dry_run,
                wait,
            });
            let code = result.unwrap_or_else(|e| {
                eprintln!("{}", serde_json::json!({"kind":"zenbench_pair_failure","schema":1,"reason":e,"exit_code":2}));
                2
            });
            std::process::exit(code);
        }
        Commands::PairDiff { before, after } => match support::pair::diff(&before, &after) {
            Ok(text) => println!("{text}"),
            Err(e) => {
                eprintln!("cannot compare pair evidence: {e}");
                std::process::exit(2);
            }
        },
        Commands::Nice {
            wait,
            timeout,
            jobs,
            memory_mib,
            command,
        } => {
            let result = support::nice::run(
                Path::new(&command[0]),
                &command[1..],
                wait,
                timeout,
                jobs,
                memory_mib,
            );
            let code = result.unwrap_or_else(|e| {
                eprintln!("[zenbench nice] not completed: {e}");
                2
            });
            std::process::exit(code);
        }
        Commands::Guide { topic } => print!(
            "{}",
            match topic.as_str() {
                "fleet" => include_str!("../fleet-help.txt"),
                "pair" => include_str!("../pair-help.txt"),
                "simd" => include_str!("../simd-help.txt"),
                "methodology" => include_str!("../methodology-help.txt"),
                _ => include_str!("../workflow-help.txt"),
            }
        ),
        Commands::Exec {
            cpu_policy,
            cpu_set,
            program,
            args,
        } => cpu_exec(cpu_policy, cpu_set.as_deref(), &program, &args),
        Commands::CpuChild { program, args } => {
            let placement = zenbench::platform::check_cpu_placement().unwrap_or_else(|e| {
                eprintln!("{e}");
                std::process::exit(2);
            });
            if placement.status != zenbench::platform::PlacementStatus::EnforcedUniform {
                eprintln!("launcher requires an enforced uniform process boundary");
                std::process::exit(2);
            }
            eprintln!(
                "[zenbench] verified before program launch: {}",
                serde_json::to_string(&placement).unwrap()
            );
            let status = std::process::Command::new(&program)
                .args(&args)
                .status()
                .unwrap_or_else(|e| {
                    eprintln!("program launch failed: {e}");
                    std::process::exit(2);
                });
            std::process::exit(status.code().unwrap_or(1));
        }
        Commands::CpuInfo { probe, workers } => {
            let mut output = serde_json::json!({
                "topology": zenbench::platform::detect_cpu_topology(),
                "placement": zenbench::platform::check_cpu_placement(),
            });
            if probe {
                output["probe"] =
                    serde_json::to_value(cpu_probe_workers(usize::from(workers))).unwrap();
                output["probe_scope"] = serde_json::json!("new_diagnostic_workers_only");
            }
            println!("{}", serde_json::to_string_pretty(&output).unwrap());
        }
        Commands::Run {
            bench,
            project,
            wait,
            timeout,
            cargo_args,
        } => cmd_run(&project, &bench, wait, timeout, cargo_args.as_deref()),
        Commands::List { project } => cmd_list(&project),
        Commands::Status { run_id, project } => cmd_status(&project, &run_id),
        Commands::Kill { target, project } => cmd_kill(&project, &target),
        Commands::Results {
            run_id,
            project,
            json,
            markdown,
            csv,
            save_charts,
            #[cfg(feature = "charts")]
            publish_charts,
            #[cfg(feature = "charts")]
            chart_theme,
            #[cfg(feature = "charts")]
            vertical,
        } => {
            #[cfg(feature = "charts")]
            let pub_config = publish_charts.as_deref().map(|d| {
                let config = zenbench::charts::ChartConfig {
                    theme: chart_theme.clone(),
                    orientation: if vertical {
                        zenbench::charts::ChartOrientation::Vertical
                    } else {
                        zenbench::charts::ChartOrientation::Horizontal
                    },
                    ..Default::default()
                };
                (d, config)
            });
            #[cfg(not(feature = "charts"))]
            let pub_config: Option<(&Path, ())> = None;
            cmd_results(
                &project,
                &run_id,
                json,
                markdown,
                csv,
                save_charts.as_deref(),
                pub_config,
            )
        }
        Commands::Read { input, format } => {
            if support::pair::is_packet(&input) {
                match support::pair::read(&input, &format) {
                    Ok(text) => println!("{text}"),
                    Err(e) => {
                        eprintln!("cannot read pair evidence: {e}");
                        std::process::exit(2);
                    }
                }
                return;
            }
            let result = zenbench::SuiteResult::load(input).unwrap_or_else(|e| {
                eprintln!("cannot read evidence: {e}");
                std::process::exit(2);
            });
            let text = match format.as_str() {
                "json" => serde_json::to_string_pretty(&result).expect("loaded JSON result"),
                "csv" => result.to_csv(),
                "md" => result.to_markdown(),
                _ => result.to_llm(),
            };
            println!("{text}");
        }
        Commands::Archive { input, output } => {
            if output.extension().is_none_or(|e| e != "gz") {
                eprintln!("archive output must end in .gz");
                std::process::exit(2);
            }
            zenbench::SuiteResult::load(input)
                .and_then(|r| r.save(output))
                .unwrap_or_else(|e| {
                    eprintln!("cannot archive results: {e}");
                    std::process::exit(2);
                });
        }
        Commands::Snapshot { input, output } => {
            let result = zenbench::SuiteResult::load(&input).unwrap_or_else(|e| {
                eprintln!("cannot load snapshot input: {e}");
                std::process::exit(2);
            });
            if result.incomplete {
                eprintln!("cannot export an incomplete attempt as a snapshot");
                std::process::exit(2);
            }
            result.save_snapshot(&output).unwrap_or_else(|e| {
                eprintln!("cannot write snapshot: {e}");
                std::process::exit(2);
            });
        }
        Commands::Reanalyze { input, output } => {
            zenbench::SuiteResult::load(input)
                .and_then(|r| r.save_analysis(output))
                .unwrap_or_else(|e| {
                    eprintln!("cannot reanalyze evidence: {e}");
                    std::process::exit(2);
                });
        }
        Commands::Compare {
            baseline,
            candidate,
        } => cmd_compare(&baseline, &candidate),
        Commands::SelfCompare {
            bench,
            git_ref,
            cargo_args,
        } => cmd_self_compare(&bench, git_ref.as_deref(), cargo_args.as_deref()),
        Commands::Clean {
            project,
            max_age_hours,
        } => cmd_clean(&project, max_age_hours),
        Commands::Baseline { action } => match action {
            BaselineAction::List => {
                let names = zenbench::baseline::list_baselines();
                if names.is_empty() {
                    println!(
                        "No baselines saved. Choose verified CPU placement, then use: cargo bench --bench NAME -- --mode=confirm --workload-id=ID --save-baseline=NAME\nRun zenbench guide for the complete workflow."
                    );
                } else {
                    for name in &names {
                        let path = format!(".zenbench/baselines/{name}.json");
                        let meta = std::fs::metadata(&path);
                        let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
                        println!("  {name:<20} ({size} bytes)");
                    }
                }
            }
            BaselineAction::Show { name } => match zenbench::baseline::load_baseline(&name) {
                Ok(result) => {
                    println!("Baseline: {name}");
                    println!("  git: {}", result.git_hash.as_deref().unwrap_or("unknown"));
                    println!("  timestamp: {}", result.timestamp);
                    for comp in &result.comparisons {
                        println!("  group: {}", comp.group_name);
                        for bench in &comp.benchmarks {
                            println!(
                                "    {}: mean={:.1}ns min={:.1}ns",
                                bench.name, bench.summary.mean, bench.summary.min
                            );
                        }
                    }
                }
                Err(e) => {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
            },
            BaselineAction::Delete { name } => match zenbench::baseline::delete_baseline(&name) {
                Ok(()) => println!("Deleted baseline '{name}'"),
                Err(e) => {
                    eprintln!("Failed to delete baseline '{name}': {e}");
                    std::process::exit(1);
                }
            },
        },
    }
}

fn cpu_probe_workers(workers: usize) -> Result<zenbench::platform::CpuProbeReport, String> {
    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(workers);
        let mut starts = Vec::with_capacity(workers);
        // Only start measuring once all workers exist. If a spawn fails, dropping
        // senders releases earlier workers without leaving a broken barrier.
        for index in 0..workers {
            let (start, ready) = std::sync::mpsc::channel();
            let handle = std::thread::Builder::new()
                .name(format!("cpu-probe-{index}"))
                .spawn_scoped(scope, move || {
                    ready.recv().map_err(|e| e.to_string())?;
                    zenbench::platform::probe_cpu_thread(Default::default())
                })
                .map_err(|e| format!("CPU probe worker creation failed: {e}"))?;
            starts.push(start);
            handles.push(handle);
        }
        for start in starts {
            start.send(()).map_err(|e| e.to_string())?;
        }
        let observations = handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .map_err(|_| "CPU probe worker panicked".to_string())?
            })
            .collect::<Result<Vec<_>, _>>()?;
        zenbench::platform::CpuProbeReport::from_workers(observations)
    })
}

fn cpu_exec(
    policy: zenbench::platform::CpuPolicy,
    explicit: Option<&str>,
    program: &Path,
    args: &[std::ffi::OsString],
) {
    use zenbench::platform::{CpuPolicy, detect_cpu_topology};
    let topology = detect_cpu_topology();
    let selected = if let Some(spec) = explicit {
        // Reuse the harness's public policy check through a child; the parser
        // below validates input before it is passed to systemd as one argument.
        let mut cpus = std::collections::BTreeSet::new();
        for part in spec.split(',') {
            let (low, high) = part.split_once('-').unwrap_or((part, part));
            let low = low.parse::<usize>();
            let high = high.parse::<usize>();
            match (low, high) {
                (Ok(low), Ok(high)) if low <= high && high < 1024 => cpus.extend(low..=high),
                _ => {
                    eprintln!("invalid --cpu-set");
                    std::process::exit(2);
                }
            }
        }
        cpus.into_iter().collect::<Vec<_>>()
    } else {
        topology.select(policy).unwrap_or_else(|e| {
            eprintln!("{e}");
            std::process::exit(2);
        })
    };
    if policy == CpuPolicy::Unrestricted || selected.is_empty() {
        eprintln!("exec requires a nonempty uniform CPU set and a checked policy");
        std::process::exit(2);
    }
    let set = selected
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let executable = std::env::current_exe().unwrap();
    #[cfg(target_os = "linux")]
    let mut child = {
        let already_enforced = zenbench::platform::check_cpu_placement().is_ok_and(|p| {
            p.status == zenbench::platform::PlacementStatus::EnforcedUniform
                && p.effective_cpus == selected
        });
        if already_enforced {
            std::process::Command::new(&executable)
        } else {
            let mut child = std::process::Command::new("systemd-run");
            child
                .args([
                    "--user",
                    "--scope",
                    "--quiet",
                    &format!("--property=AllowedCPUs={set}"),
                ])
                .arg(&executable);
            child
        }
    };
    #[cfg(not(target_os = "linux"))]
    let mut child = std::process::Command::new(&executable);
    let status = child
        .arg("cpu-child")
        .arg(program)
        .arg("--")
        .args(args)
        .env("ZENBENCH_CPU_POLICY", policy.to_string())
        .env("ZENBENCH_CPU_SET", &set)
        .env("RAYON_NUM_THREADS", selected.len().to_string())
        .status()
        .unwrap_or_else(|e| {
            eprintln!("CPU scope launch failed: {e}");
            std::process::exit(2);
        });
    std::process::exit(status.code().unwrap_or(1));
}

fn cmd_run(
    project: &Path,
    bench_name: &str,
    wait: bool,
    timeout_secs: u64,
    cargo_args: Option<&str>,
) {
    let mut args = vec!["bench", "--bench", bench_name];
    let extra_args: Vec<&str>;
    if let Some(extra) = cargo_args {
        extra_args = extra.split_whitespace().collect();
        args.extend(&extra_args);
    }

    let str_args: Vec<&str> = args.to_vec();

    match daemon::spawn_detached(project, "cargo", &str_args) {
        Ok((run_id, mut child)) => {
            eprintln!("[zenbench] run {run_id} spawned");

            if wait {
                // Wait for the child to prevent zombie processes
                let _exit_status = child.wait();
                eprintln!("[zenbench] waiting for completion (timeout: {timeout_secs}s)...");
                match daemon::wait_for_run(
                    project,
                    &run_id,
                    std::time::Duration::from_secs(2),
                    std::time::Duration::from_secs(timeout_secs),
                ) {
                    Ok(state) => match &state.status {
                        daemon::RunStatus::Completed => {
                            eprintln!("[zenbench] run {run_id} completed");
                            if let Some(result_path) = &state.result_path {
                                match zenbench::SuiteResult::load(result_path) {
                                    Ok(result) => result.print_report(),
                                    Err(e) => eprintln!("Error loading results: {e}"),
                                }
                            }
                        }
                        daemon::RunStatus::Failed(msg) => {
                            eprintln!("[zenbench] run {run_id} failed: {msg}");
                            // Print stderr log if available
                            let stderr_path =
                                daemon::runs_dir(project).join(format!("{run_id}.stderr.log"));
                            if let Ok(log) = std::fs::read_to_string(&stderr_path) {
                                let trimmed = log.trim();
                                if !trimmed.is_empty() {
                                    eprintln!("--- stderr ---");
                                    // Print last 2000 chars
                                    let start = trimmed.len().saturating_sub(2000);
                                    eprintln!("{}", &trimmed[start..]);
                                    eprintln!("--- end stderr ---");
                                }
                            }
                            std::process::exit(1);
                        }
                        daemon::RunStatus::Killed => {
                            eprintln!("[zenbench] run {run_id} was killed");
                            std::process::exit(1);
                        }
                        _ => {
                            eprintln!(
                                "[zenbench] run {run_id} in unexpected state: {:?}",
                                state.status
                            );
                            std::process::exit(1);
                        }
                    },
                    Err(e) => {
                        eprintln!("[zenbench] error: {e}");
                        std::process::exit(1);
                    }
                }
            } else {
                // Drop child without waiting — zombie will be reaped when this process exits
                drop(child);
                eprintln!("[zenbench] use `zenbench status {run_id}` to check progress");
                eprintln!("[zenbench] use `zenbench results {run_id}` when complete");
            }
        }
        Err(e) => {
            eprintln!("[zenbench] error spawning benchmark: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_list(project: &Path) {
    match daemon::list_runs(project) {
        Ok(runs) => {
            if runs.is_empty() {
                eprintln!("No benchmark runs found.");
                return;
            }
            println!(
                "{:<24} {:<12} {:<8} {:<10} COMMAND",
                "ID", "STATUS", "PID", "GIT"
            );
            for run in runs {
                let status = match &run.status {
                    daemon::RunStatus::Queued => "queued".to_string(),
                    daemon::RunStatus::Running => {
                        if daemon::is_process_alive(run.pid) {
                            "running".to_string()
                        } else {
                            "running?".to_string() // shouldn't happen with reconciliation
                        }
                    }
                    daemon::RunStatus::Completed => "completed".to_string(),
                    daemon::RunStatus::Failed(_) => "failed".to_string(),
                    daemon::RunStatus::Killed => "killed".to_string(),
                };
                let git = run.git_hash.as_deref().unwrap_or("-");
                let git_short = if git.len() > 8 { &git[..8] } else { git };
                println!(
                    "{:<24} {:<12} {:<8} {:<10} {}",
                    run.id, status, run.pid, git_short, run.command
                );
            }
        }
        Err(e) => eprintln!("Error listing runs: {e}"),
    }
}

fn cmd_status(project: &Path, run_id: &str) {
    match daemon::load_run_state(project, run_id) {
        Ok(state) => {
            let alive = daemon::is_process_alive(state.pid);
            println!("Run: {}", state.id);
            println!("Status: {:?}", state.status);
            println!(
                "PID: {} ({})",
                state.pid,
                if alive { "alive" } else { "dead" }
            );
            println!("Git: {}", state.git_hash.as_deref().unwrap_or("unknown"));
            println!("Command: {}", state.command);
            if let Some(path) = &state.result_path {
                let exists = path.exists();
                println!(
                    "Results: {} ({})",
                    path.display(),
                    if exists { "exists" } else { "pending" }
                );
            }
            // Show elapsed time
            let now = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let elapsed = now.saturating_sub(state.started_at);
            if let Some(finished) = state.finished_at {
                let duration = finished.saturating_sub(state.started_at);
                println!("Duration: {duration}s");
            } else {
                println!("Elapsed: {elapsed}s");
            }
        }
        Err(e) => eprintln!("Error loading run {run_id}: {e}"),
    }
}

fn cmd_kill(project: &Path, target: &str) {
    if target == "stale" {
        let hash = zenbench::platform::git_commit_hash().unwrap_or_default();
        if hash.is_empty() {
            eprintln!("Cannot determine current git hash. Use a specific run ID.");
            return;
        }
        match daemon::kill_stale_runs(project, &hash) {
            Ok(n) => eprintln!("Killed {n} stale run(s)."),
            Err(e) => eprintln!("Error: {e}"),
        }
    } else {
        match daemon::kill_run(project, target) {
            Ok(true) => eprintln!("Killed run {target}."),
            Ok(false) => eprintln!("Run {target} was not running."),
            Err(e) => eprintln!("Error: {e}"),
        }
    }
}

fn cmd_results(
    project: &Path,
    run_id: &str,
    json: bool,
    markdown: bool,
    csv: bool,
    save_charts: Option<&Path>,
    #[cfg(feature = "charts")] publish_config: Option<(&Path, zenbench::charts::ChartConfig)>,
    #[cfg(not(feature = "charts"))] publish_config: Option<(&Path, ())>,
) {
    let save_all_charts = |result: &zenbench::SuiteResult| {
        if let Some(dir) = save_charts {
            if let Err(e) = result.save_charts(dir) {
                eprintln!("Error saving charts: {e}");
            } else {
                eprintln!("Charts saved to {}", dir.display());
            }
        }
        #[cfg(feature = "charts")]
        if let Some((dir, ref config)) = publish_config {
            if let Err(e) = result.save_publication_charts(dir, config) {
                eprintln!("Error saving publication charts: {e}");
            } else {
                eprintln!(
                    "Publication charts ({}, {:?}) saved to {}",
                    config.theme,
                    config.orientation,
                    dir.display()
                );
            }
        }
        #[cfg(not(feature = "charts"))]
        let _ = publish_config;
    };

    // Check if it's a direct file path first
    let file_path = PathBuf::from(run_id);
    if file_path.exists() && file_path.extension().is_some_and(|e| e == "json") {
        match zenbench::SuiteResult::load(&file_path) {
            Ok(result) => {
                output_result(&result, json, markdown, csv);
                save_all_charts(&result);
                return;
            }
            Err(e) => {
                eprintln!("Error loading results from {}: {e}", file_path.display());
                return;
            }
        }
    }

    // Resolve "latest" to the most recent completed run
    let actual_id = if run_id == "latest" {
        match daemon::find_latest_with_results(project) {
            Ok(Some(state)) => state.id,
            Ok(None) => {
                eprintln!("No completed runs with results found.");
                return;
            }
            Err(e) => {
                eprintln!("Error: {e}");
                return;
            }
        }
    } else {
        run_id.to_string()
    };

    match daemon::load_run_state(project, &actual_id) {
        Ok(state) => {
            if let Some(result_path) = &state.result_path {
                match zenbench::SuiteResult::load(result_path) {
                    Ok(result) => {
                        output_result(&result, json, markdown, csv);
                        save_all_charts(&result);
                    }
                    Err(e) => {
                        eprintln!("Error loading results from {}: {e}", result_path.display());
                        if !result_path.exists() {
                            eprintln!(
                                "Run {} status: {:?}. Results file does not exist.",
                                actual_id, state.status
                            );
                        }
                    }
                }
            } else {
                eprintln!(
                    "Run {} has no results path (status: {:?})",
                    actual_id, state.status
                );
            }
        }
        Err(e) => eprintln!("Error: {e}"),
    }
}

fn output_result(result: &zenbench::SuiteResult, json: bool, markdown: bool, csv: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(result).unwrap());
    } else if markdown {
        print!("{}", result.to_markdown());
    } else if csv {
        print!("{}", result.to_csv());
    } else {
        result.print_report();
    }
}

fn cmd_compare(baseline_path: &Path, candidate_path: &Path) {
    let baseline = match zenbench::SuiteResult::load(baseline_path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error loading baseline: {e}");
            std::process::exit(2);
        }
    };
    let candidate = match zenbench::SuiteResult::load(candidate_path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error loading candidate: {e}");
            std::process::exit(2);
        }
    };

    std::process::exit(print_comparison(&baseline, &candidate));
}

fn cmd_self_compare(bench_name: &str, git_ref: Option<&str>, cargo_args: Option<&str>) {
    // Resolve the git ref to compare against
    let reference = match git_ref {
        Some(r) => r.to_string(),
        None => match find_last_version_tag() {
            Some(tag) => {
                eprintln!("[zenbench] comparing against tag: {tag}");
                tag
            }
            None => {
                eprintln!("Error: no version tags found and no --ref specified.");
                eprintln!("  Create a tag first (git tag v0.1.0) or specify --ref <commit>");
                std::process::exit(1);
            }
        },
    };

    // Verify the ref exists
    if !git_ref_exists(&reference) {
        eprintln!("Error: git ref '{reference}' does not exist.");
        std::process::exit(1);
    }

    eprintln!(
        "[zenbench] exploratory source comparison: baseline runs before candidate; no randomized A/B schedule"
    );
    eprintln!(
        "[zenbench] candidate uses the current working tree, including local edits; runtime Git HEAD is not build provenance"
    );
    let current_hash = zenbench::platform::git_short_hash().unwrap_or_else(|| "HEAD".to_string());
    eprintln!("[zenbench] self-compare: {reference} (baseline) vs {current_hash} (candidate)");

    // Build workspaces are disposable; measurement packets are durable.
    let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let tmp_dir = project_target_dir(&current_dir)
        .join("zenbench")
        .join("self-compare");
    std::fs::create_dir_all(&tmp_dir).unwrap_or_else(|e| {
        eprintln!("Error creating {}: {e}", tmp_dir.display());
        std::process::exit(1);
    });
    let evidence_root = std::env::var_os("ZENBENCH_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| current_dir.join(".zenbench/data"));
    std::fs::create_dir_all(&evidence_root).expect("cannot create evidence root");
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(evidence_root.join(".gitignore"))
    {
        Ok(mut file) => {
            use std::io::Write;
            file.write_all(b"*\n")
                .expect("cannot protect raw evidence from Git");
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => panic!("cannot protect raw evidence: {e}"),
    }
    let evidence_dir = evidence_root
        .canonicalize()
        .expect("cannot resolve evidence root")
        .join("self-compare")
        .join(zenbench::RunId::generate().0);
    std::fs::create_dir_all(&evidence_dir).expect("cannot retain self-compare evidence");
    let baseline_result_path = evidence_dir.join("baseline.json");
    let candidate_result_path = evidence_dir.join("candidate.json");
    eprintln!(
        "[zenbench] retained self-compare evidence → {}",
        evidence_dir.display()
    );

    // Step 1: Create worktree for the old ref
    let worktree_path = tmp_dir.join("worktree");
    eprintln!("[zenbench] creating worktree at {reference}...");
    // A `cargo clean` deletes the directory but leaves git's registration
    // behind, which would make `worktree add` refuse the path.
    run_git(&["worktree", "prune"]);
    if worktree_path.exists() {
        // Clean up leftover worktree
        run_git(&[
            "worktree",
            "remove",
            "--force",
            worktree_path.to_str().unwrap(),
        ]);
    }
    if !run_git(&[
        "worktree",
        "add",
        "--detach",
        worktree_path.to_str().unwrap(),
        &reference,
    ]) {
        eprintln!("Error: failed to create git worktree at {reference}.");
        std::process::exit(1);
    }

    // Step 2: Build and run baseline (old version)
    eprintln!("[zenbench] building and running baseline ({reference})...");
    let baseline_ok = run_bench_in_dir(
        &worktree_path,
        bench_name,
        &baseline_result_path,
        cargo_args,
    );

    // Step 3: Build and run candidate (current version)
    eprintln!("[zenbench] building and running candidate ({current_hash})...");
    let candidate_ok =
        run_bench_in_dir(&current_dir, bench_name, &candidate_result_path, cargo_args);

    // Step 4: Clean up worktree
    eprintln!("[zenbench] cleaning up worktree...");
    run_git(&[
        "worktree",
        "remove",
        "--force",
        worktree_path.to_str().unwrap(),
    ]);

    // Step 5: Compare results
    if !baseline_ok {
        eprintln!("Error: baseline benchmark failed to produce results.");
        eprintln!("  Make sure the benchmark uses zenbench::main!() macro.");
        std::process::exit(1);
    }
    if !candidate_ok {
        eprintln!("Error: candidate benchmark failed to produce results.");
        std::process::exit(1);
    }

    let baseline = match zenbench::SuiteResult::load(&baseline_result_path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error loading baseline results: {e}");
            std::process::exit(1);
        }
    };
    let candidate = match zenbench::SuiteResult::load(&candidate_result_path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error loading candidate results: {e}");
            std::process::exit(1);
        }
    };

    eprintln!();
    std::process::exit(print_comparison(&baseline, &candidate));
}

/// The cargo target directory of the project in `dir`: `$CARGO_TARGET_DIR` when
/// absolute, else `target_directory` from `cargo metadata`, else `dir/target`.
fn project_target_dir(dir: &Path) -> PathBuf {
    if let Some(t) = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
    {
        return t;
    }
    std::process::Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(dir)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok())
        .and_then(|v| v.get("target_directory")?.as_str().map(PathBuf::from))
        .unwrap_or_else(|| dir.join("target"))
}

/// Find the most recent version tag (matching v* pattern).
fn find_last_version_tag() -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["tag", "--sort=-version:refname", "--list", "v*"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.lines().next().map(|s| s.trim().to_string())
}

/// Check if a git ref exists.
fn git_ref_exists(git_ref: &str) -> bool {
    std::process::Command::new("git")
        .args(["rev-parse", "--verify", git_ref])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Run a git command, returning true on success.
fn run_git(args: &[&str]) -> bool {
    std::process::Command::new("git")
        .args(args)
        .status()
        .is_ok_and(|s| s.success())
}

/// Build and run a benchmark in a specific directory.
/// Returns true if the result file was produced.
fn run_bench_in_dir(
    dir: &Path,
    bench_name: &str,
    result_path: &Path,
    cargo_args: Option<&str>,
) -> bool {
    let mut cmd = std::process::Command::new("cargo");
    cmd.current_dir(dir)
        .args(["bench", "--bench", bench_name])
        .env("ZENBENCH_RESULT_PATH", result_path);

    // Tell the child benchmark to exclude the launcher's PID from the
    // benchmark-process gate. Without this, the child detects the parent
    // `zenbench self-compare` process and waits for it to exit (deadlock).
    let our_pid = std::process::id();
    let launcher_pids = match std::env::var("ZENBENCH_LAUNCHER_PIDS") {
        Ok(existing) => format!("{existing},{our_pid}"),
        Err(_) => our_pid.to_string(),
    };
    cmd.env("ZENBENCH_LAUNCHER_PIDS", &launcher_pids);
    // Do not leave auto-saved child evidence inside a disposable build workspace.
    cmd.env(
        "ZENBENCH_DATA_DIR",
        result_path.parent().expect("result parent"),
    );

    if let Some(args) = cargo_args {
        for arg in args.split_whitespace() {
            cmd.arg(arg);
        }
    }

    let status = cmd.status();

    match status {
        Ok(s) if s.success() => result_path.exists(),
        Ok(s) => {
            eprintln!(
                "  cargo bench exited with status {} in {}",
                s,
                dir.display()
            );
            result_path.exists() // might still have results
        }
        Err(e) => {
            eprintln!("  failed to run cargo bench in {}: {e}", dir.display());
            false
        }
    }
}

/// Print a colored comparison between two suite results.
fn print_cpu_evidence(label: &str, placement: &Option<zenbench::platform::CpuPlacement>) {
    use zenbench::platform::PlacementStatus;
    let status = match placement.as_ref().map(|p| p.status) {
        Some(PlacementStatus::UniformHardware) => "uniform_hardware",
        Some(PlacementStatus::EnforcedUniform) => "enforced_uniform",
        Some(PlacementStatus::Unrestricted) => "UNVERIFIED_UNRESTRICTED",
        _ => "LEGACY_UNKNOWN",
    };
    eprintln!(
        "{label}: cpu_placement={status} evidence={}",
        serde_json::to_string(placement).unwrap()
    );
}

fn print_comparison(baseline: &zenbench::SuiteResult, candidate: &zenbench::SuiteResult) -> i32 {
    print_cpu_evidence("baseline CPU", &baseline.cpu_placement);
    print_cpu_evidence("candidate CPU", &candidate.cpu_placement);
    let margin = candidate
        .execution
        .as_ref()
        .map(|e| e.plan.margin_pct)
        .unwrap_or(1.0);
    if let Err(e) = baseline.check_cpu_compatibility(candidate) {
        eprintln!("INVALID comparison: {e}");
        std::process::exit(2);
    }
    let comparison = zenbench::baseline::compare_against_baseline(baseline, candidate, margin);
    zenbench::baseline::print_comparison_report(&comparison);
    if comparison
        .confirmed
        .iter()
        .any(|c| c.verdict == zenbench::Verdict::Regression)
    {
        1
    } else if comparison.promotable {
        0
    } else {
        2
    }
}

fn cmd_clean(project: &Path, max_age_hours: u64) {
    let max_age_secs = max_age_hours * 3600;
    match daemon::cleanup_old_runs(project, max_age_secs) {
        Ok(n) => eprintln!("Cleaned up {n} old run(s)."),
        Err(e) => eprintln!("Error: {e}"),
    }
}

use std::time::SystemTime;
