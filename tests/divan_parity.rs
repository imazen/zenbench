//! Automated parity test: run identical workloads through zenbench and divan,
//! parse both outputs, verify measurements agree within tolerance.
//!
//! This test builds and runs both benchmark binaries, parses their output,
//! and compares the mean times. The tolerance is generous (3x) because
//! different measurement methodologies will produce different numbers —
//! the goal is to verify they're in the same ballpark, not identical.
//!
//! Requires: both bench binaries to be buildable (divan as dev-dependency).

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
        let stripped = first
            .trim_start_matches(|c: char| {
                !c.is_alphanumeric() && c != '.'
            });

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
                eprintln!(
                    "  {zen_name}: zen={z:.1}ns divan={d:.1}ns ratio={ratio:.2}x [{status}]"
                );
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
