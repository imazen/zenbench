/// Format nanoseconds as human-readable time.
pub fn format_ns(ns: f64) -> String {
    let abs = ns.abs();
    let sign = if ns < 0.0 { "-" } else { "" };
    if abs >= 1_000_000_000.0 {
        format!("{}{:.2}s", sign, abs / 1_000_000_000.0)
    } else if abs >= 1_000_000.0 {
        format!("{}{:.2}ms", sign, abs / 1_000_000.0)
    } else if abs >= 1_000.0 {
        format!("{}{:.2}\u{b5}s", sign, abs / 1_000.0)
    } else if abs >= 0.01 {
        format!("{}{:.1}ns", sign, abs)
    } else {
        format!("{}{:.3}ns", sign, abs)
    }
}

/// Pick unit and decimal places for a nanosecond value.
/// Returns (divisor, unit_str, decimal_places).
pub(crate) fn ns_unit(mean_abs: f64) -> (f64, &'static str, usize) {
    if mean_abs >= 1_000_000_000.0 {
        (1_000_000_000.0, "s", 2)
    } else if mean_abs >= 1_000_000.0 {
        (1_000_000.0, "ms", 1)
    } else if mean_abs >= 1_000.0 {
        (1_000.0, "\u{b5}s", 1)
    } else if mean_abs >= 100.0 {
        (1.0, "ns", 0)
    } else if mean_abs >= 10.0 {
        (1.0, "ns", 1)
    } else {
        (1.0, "ns", 2)
    }
}

/// Format a [lo mean hi] range with shared unit and aligned columns.
pub(crate) fn format_ns_range(lo: f64, mean: f64, hi: f64) -> String {
    let (divisor, unit, dp) = ns_unit(mean.abs());
    let vals: Vec<String> = [lo, mean, hi]
        .iter()
        .map(|&v| format!("{:.*}", dp, v / divisor))
        .collect();
    let w = vals.iter().map(|s| s.len()).max().unwrap_or(1);
    format!("[{:>w$}  {:>w$}  {:>w$}]{unit}", vals[0], vals[1], vals[2],)
}

/// Detect terminal width. Checks `COLUMNS` env var, falls back to None.
pub(crate) fn terminal_width() -> Option<usize> {
    if let Ok(cols) = std::env::var("COLUMNS") {
        if let Ok(w) = cols.parse::<usize>() {
            if w > 0 {
                return Some(w);
            }
        }
    }
    None
}

/// Escape a string for CSV (double-quote if it contains comma, quote, or newline).
pub(crate) fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}
