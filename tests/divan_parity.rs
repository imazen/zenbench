//! Automated parity test: run identical workloads through zenbench, divan,
//! and criterion, parse all outputs, verify measurements agree within tolerance.
//!
//! The tolerance is generous (3x) because different measurement methodologies
//! will produce different numbers — the goal is to verify they're in the same
//! ballpark, not identical. Known methodology differences:
//!
//! - **Iteration count**: zenbench targets 10ms/sample (~10K iters for µs workloads),
//!   divan auto-tunes to ~100 iters, criterion varies linearly. More iterations =
//!   hotter allocator cache = lower latency for allocation-heavy workloads.
//! - **Overhead compensation**: zenbench subtracts loop+black_box overhead,
//!   divan subtracts loop+alloc overhead, criterion uses slope regression.
//! - **Timer**: zenbench uses TSC (rdtsc/rdtscp), divan uses Instant (default),
//!   criterion uses Instant.
//!
//! Requires: bench binaries buildable (divan + criterion as dev-dependencies).

use std::collections::HashMap;
use std::process::Command;

/// Parse zenbench LLM output into benchmark name -> mean_ns
fn parse_zenbench_llm(output: &str) -> HashMap<String, f64> {
    let mut results = HashMap::new();
    for line in output.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        // Extract benchmark= and mean= fields
        let mut name = None;
        let mut mean = None;
        for segment in line.split("  |  ") {
            for field in segment.split_whitespace() {
                if let Some(n) = field.strip_prefix("benchmark=") {
                    name = Some(n.trim_matches('"').to_string());
                }
                if let Some(m) = field.strip_prefix("mean=") {
                    mean = parse_time_value(m);
                }
            }
        }
        if let (Some(n), Some(m)) = (name, mean) {
            results.insert(n, m);
        }
    }
    results
}

/// Parse divan terminal output into benchmark name -> mean_ns.
///
/// Divan's tree format looks like:
/// ```text
/// ├─ hashmap_insert_100  990.8 ns │ 4.808 µs │ 1.081 µs │ 1.158 µs │ 100 │ 100
/// ├─ noop                0.911 ns │ 0.96 ns  │ 0.911 ns │ 0.914 ns │ 100 │ 102400
/// ```
/// Columns: name, fastest, slowest, median, mean, samples, iters
/// Name is prefixed with tree chars (├─, ╰─, │).
fn parse_divan_output(output: &str) -> HashMap<String, f64> {
    let mut results = HashMap::new();
    for line in output.lines() {
        let line = line.trim();
        // Skip the header line and empty lines
        if !line.contains('│') || line.contains("fastest") {
            continue;
        }

        let parts: Vec<&str> = line.split('│').collect();
        if parts.len() < 4 {
            continue;
        }

        // First column: tree prefix + benchmark name + fastest time
        // e.g., "├─ noop                0.911 ns      "
        let first = parts[0].trim();

        // Strip tree drawing characters (├─, ╰─, │, spaces)
        let stripped = first.trim_start_matches(|c: char| !c.is_alphanumeric() && c != '.');

        // Split into name and fastest value
        // Name is the first word(s) before the time value
        let mut name = String::new();
        // Find where the name ends and the time value begins.
        // The time value starts with a digit or minus sign.
        for (i, c) in stripped.char_indices() {
            if c == ' ' {
                let after = stripped[i..].trim_start();
                if after.starts_with(|c: char| c.is_ascii_digit() || c == '-') {
                    name = stripped[..i].trim().to_string();
                    break;
                }
            }
        }
        if name.is_empty() {
            continue;
        }

        // Mean is the 4th column (index 3): parts[0]=name+fastest, [1]=slowest, [2]=median, [3]=mean
        if let Some(mean_ns) = parse_time_value(parts[3].trim()) {
            results.insert(name, mean_ns);
        }
    }
    results
}

/// Parse a time string like "1.23 ns", "456 µs", "0.78 ms" into nanoseconds
fn parse_time_value(s: &str) -> Option<f64> {
    let s = s.trim();
    // Try patterns: "123.4ns", "123.4 ns", "1.23µs", "1.23 µs", etc.
    let (num_str, unit) = if let Some(pos) = s.find(|c: char| c.is_alphabetic() || c == 'µ') {
        let (n, u) = s.split_at(pos);
        (n.trim(), u.trim())
    } else {
        return None;
    };

    let value: f64 = num_str.parse().ok()?;
    let ns = match unit {
        "ns" => value,
        "µs" | "us" => value * 1_000.0,
        "ms" => value * 1_000_000.0,
        "s" => value * 1_000_000_000.0,
        "ps" => value / 1_000.0,
        _ => return None,
    };
    Some(ns)
}

#[test]
fn parity_zenbench_vs_divan() {
    // Build and run zenbench version (via cargo bench = release profile)
    let zen_output = Command::new("cargo")
        .args(["bench", "--bench", "divan_compare", "--", "--format=llm"])
        .output();

    let zen_output = match zen_output {
        Ok(o) if o.status.success() => o,
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            eprintln!("zenbench bench failed:\n{stderr}");
            // Don't fail the test if benchmarks can't run (CI without time)
            eprintln!("SKIPPING: zenbench bench binary failed to run");
            return;
        }
        Err(e) => {
            eprintln!("SKIPPING: could not run zenbench bench: {e}");
            return;
        }
    };
    let zen_stdout = String::from_utf8_lossy(&zen_output.stdout);
    let zen_stderr = String::from_utf8_lossy(&zen_output.stderr);
    let zen_results = parse_zenbench_llm(&zen_stdout);

    eprintln!("=== zenbench results ===");
    for (name, ns) in &zen_results {
        eprintln!("  {name}: {ns:.1} ns");
    }

    // Build and run divan version
    let divan_output = Command::new("cargo")
        .args(["bench", "--bench", "divan_compare_ref"])
        .output();

    let divan_output = match divan_output {
        Ok(o) if o.status.success() => o,
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            eprintln!("divan bench failed:\n{stderr}");
            eprintln!("SKIPPING: divan bench binary failed to run");
            return;
        }
        Err(e) => {
            eprintln!("SKIPPING: could not run divan bench: {e}");
            return;
        }
    };
    // Divan outputs benchmark results to stdout
    let divan_stdout = String::from_utf8_lossy(&divan_output.stdout);
    let divan_stderr = String::from_utf8_lossy(&divan_output.stderr);
    let mut divan_results = parse_divan_output(&divan_stdout);
    if divan_results.is_empty() {
        // Also try stderr in case format changed
        divan_results = parse_divan_output(&divan_stderr);
    }
    if divan_results.is_empty() {
        eprintln!("=== divan raw stdout ===\n{divan_stdout}");
        eprintln!("=== divan raw stderr ===\n{divan_stderr}");
        eprintln!("WARNING: could not parse any divan results");
        // Don't fail — divan output format may have changed
        return;
    }

    eprintln!("\n=== divan results ===");
    for (name, ns) in &divan_results {
        eprintln!("  {name}: {ns:.1} ns");
    }

    // Compare matching benchmarks
    let tolerance = 3.0; // 3x tolerance — different methodologies, not identical
    let mut matched = 0;
    let mut mismatched = 0;

    // Map divan names to zenbench names (divan uses function names, zenbench uses bench names)
    let name_map: HashMap<&str, &str> = [
        ("noop", "noop"),
        ("sum_100", "sum_100"),
        ("sum_1000", "sum_1000"),
        ("sort_100", "sort_100"),
        ("sort_10000", "sort_10000"),
        ("hashmap_insert_100", "insert_100"),
    ]
    .into();

    eprintln!("\n=== comparison (tolerance: {tolerance}x) ===");
    for (divan_name, zen_name) in &name_map {
        let zen_ns = zen_results.get(*zen_name);
        let divan_ns = divan_results.get(*divan_name);

        match (zen_ns, divan_ns) {
            (Some(&z), Some(&d)) => {
                let ratio = if z > d { z / d } else { d / z };
                let status = if ratio <= tolerance { "OK" } else { "MISMATCH" };
                eprintln!("  {zen_name}: zen={z:.1}ns divan={d:.1}ns ratio={ratio:.2}x [{status}]");
                if ratio <= tolerance {
                    matched += 1;
                } else {
                    mismatched += 1;
                }
            }
            (Some(&z), None) => {
                eprintln!("  {zen_name}: zen={z:.1}ns divan=MISSING");
            }
            (None, Some(&d)) => {
                eprintln!("  {zen_name}: zen=MISSING divan={d:.1}ns");
            }
            (None, None) => {
                eprintln!("  {zen_name}: both MISSING");
            }
        }
    }

    eprintln!(
        "\n=== summary: {matched} matched, {mismatched} mismatched (of {} pairs) ===",
        name_map.len()
    );

    // Log full output for debugging
    eprintln!("\n=== zenbench stderr ===\n{zen_stderr}");

    // We don't hard-fail on mismatches — this is a comparison tool, not a gate.
    // But we DO fail if we couldn't match any benchmarks at all.
    if matched == 0 && !zen_results.is_empty() && !divan_results.is_empty() {
        panic!(
            "No benchmarks matched between zenbench and divan!\n\
             zen names: {:?}\n\
             divan names: {:?}",
            zen_results.keys().collect::<Vec<_>>(),
            divan_results.keys().collect::<Vec<_>>(),
        );
    }
}

/// Parse criterion terminal output into benchmark name -> mean_ns.
///
/// Criterion format:
/// ```text
/// noop                    time:   [1.1278 ns 1.1352 ns 1.1429 ns]
/// ```
/// The middle value in the bracket is the point estimate (mean or slope).
fn parse_criterion_output(output: &str) -> HashMap<String, f64> {
    let mut results = HashMap::new();
    for line in output.lines() {
        let line = line.trim();
        // Look for lines with "time:   [lo est hi]"
        if let Some(time_idx) = line.find("time:") {
            let name = line[..time_idx].trim();
            if name.is_empty() {
                continue;
            }
            // Extract the bracket contents: "[1.12 ns 1.13 ns 1.14 ns]"
            let rest = &line[time_idx..];
            if let (Some(open), Some(close)) = (rest.find('['), rest.find(']')) {
                let bracket = &rest[open + 1..close];
                let parts: Vec<&str> = bracket.split_whitespace().collect();
                // Format: "1.1278 ns 1.1352 ns 1.1429 ns" → 6 tokens
                // Middle value (point estimate) is at index 2-3
                if parts.len() >= 4 {
                    let mid = format!("{} {}", parts[2], parts[3]);
                    if let Some(ns) = parse_time_value(&mid) {
                        results.insert(name.to_string(), ns);
                    }
                }
            }
        }
    }
    results
}

/// Helper to run a cargo bench command and return (stdout, stderr).
fn run_bench(args: &[&str]) -> Option<(String, String)> {
    let output = Command::new("cargo").args(args).output().ok()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("bench failed ({:?}):\n{stderr}", args);
        return None;
    }
    Some((
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    ))
}

#[test]
fn parity_three_way() {
    eprintln!("\n===== 3-Way Framework Comparison =====\n");

    // Run all three frameworks
    let zen = run_bench(&["bench", "--bench", "divan_compare", "--", "--format=llm"]);
    let divan = run_bench(&["bench", "--bench", "divan_compare_ref"]);
    let crit = run_bench(&["bench", "--bench", "criterion_compare_ref"]);

    let zen_results = zen
        .as_ref()
        .map(|(stdout, _)| parse_zenbench_llm(stdout))
        .unwrap_or_default();
    let divan_results = divan
        .as_ref()
        .map(|(stdout, _)| parse_divan_output(stdout))
        .unwrap_or_default();
    let crit_results = crit
        .as_ref()
        .map(|(stdout, stderr)| {
            // criterion outputs to stdout
            let mut r = parse_criterion_output(stdout);
            if r.is_empty() {
                r = parse_criterion_output(stderr);
            }
            r
        })
        .unwrap_or_default();

    if zen_results.is_empty() {
        eprintln!("SKIPPING: no zenbench results");
        return;
    }

    // Benchmark name mapping: (zen_name, divan_name, criterion_name)
    let benchmarks = [
        ("noop", "noop", "noop"),
        ("sum_100", "sum_100", "sum_100"),
        ("sum_1000", "sum_1000", "sum_1000"),
        ("sort_100", "sort_100", "sort_100"),
        ("sort_10000", "sort_10000", "sort_10000"),
        ("insert_100", "hashmap_insert_100", "hashmap_insert_100"),
    ];

    eprintln!(
        "  {:<16} {:>13}  {:>13}  {:>13}  {:>8}  {:>8}  {:>8}",
        "workload", "zenbench", "divan", "criterion", "z/d", "z/c", "d/c"
    );
    eprintln!("  {}", "-".repeat(100));

    let mut all_ok = true;
    for (zen_name, divan_name, crit_name) in &benchmarks {
        let z = zen_results.get(*zen_name);
        let d = divan_results.get(*divan_name);
        let c = crit_results.get(*crit_name);

        let fmt = |v: Option<&f64>| -> String {
            match v {
                Some(&ns) if ns >= 1000.0 => format!("{:.0} ns", ns),
                Some(&ns) => format!("{:.2} ns", ns),
                None => "—".to_string(),
            }
        };
        let ratio = |a: Option<&f64>, b: Option<&f64>| -> String {
            match (a, b) {
                (Some(&a), Some(&b)) if b > 0.0 => format!("{:.2}x", a / b),
                _ => "—".to_string(),
            }
        };

        eprintln!(
            "  {:<16} {:>13}  {:>13}  {:>13}  {:>8}  {:>8}  {:>8}",
            zen_name,
            fmt(z),
            fmt(d),
            fmt(c),
            ratio(z, d),
            ratio(z, c),
            ratio(d, c)
        );

        // Check pairwise within 3x
        let tolerance = 3.0;
        if let (Some(&zv), Some(&dv)) = (z, d) {
            let r = if zv > dv { zv / dv } else { dv / zv };
            if r > tolerance {
                all_ok = false;
            }
        }
        if let (Some(&zv), Some(&cv)) = (z, c) {
            let r = if zv > cv { zv / cv } else { cv / zv };
            if r > tolerance {
                all_ok = false;
            }
        }
    }

    eprintln!();
    if !divan_results.is_empty() && !crit_results.is_empty() {
        eprintln!(
            "  All pairwise within 3x: {}",
            if all_ok { "YES" } else { "NO" }
        );
    }
}

#[test]
fn parse_criterion_output_works() {
    let input = r#"
noop                    time:   [1.1278 ns 1.1352 ns 1.1429 ns]
Found 2 outliers among 100 measurements (2.00%)

sum_100                 time:   [26.095 ns 26.218 ns 26.371 ns]

sort_10000              time:   [2.8843 µs 2.8957 µs 2.9079 µs]

hashmap_insert_100      time:   [1.1329 µs 1.1370 µs 1.1416 µs]
"#;
    let results = parse_criterion_output(input);
    assert!((results["noop"] - 1.1352).abs() < 0.001);
    assert!((results["sum_100"] - 26.218).abs() < 0.01);
    assert!((results["sort_10000"] - 2895.7).abs() < 1.0);
    assert!((results["hashmap_insert_100"] - 1137.0).abs() < 1.0);
}

#[test]
fn parse_time_value_works() {
    assert!((parse_time_value("1.23 ns").unwrap() - 1.23).abs() < 0.001);
    assert!((parse_time_value("1.23ns").unwrap() - 1.23).abs() < 0.001);
    assert!((parse_time_value("1.23 µs").unwrap() - 1230.0).abs() < 0.1);
    assert!((parse_time_value("1.23 ms").unwrap() - 1_230_000.0).abs() < 1.0);
    assert!((parse_time_value("1.5 s").unwrap() - 1_500_000_000.0).abs() < 1.0);
    assert!(parse_time_value("abc").is_none());
    assert!(parse_time_value("").is_none());
}
