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
    if let Ok(cols) = std::env::var("COLUMNS")
        && let Ok(w) = cols.parse::<usize>()
        && w > 0
    {
        return Some(w);
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

#[cfg(test)]
mod tests {
    use super::*;

    // --- ns_unit ---

    #[test]
    fn ns_unit_sub_ns() {
        // < 10 ns: 2 decimal places
        let (div, unit, dp) = ns_unit(0.5);
        assert_eq!(div, 1.0, "divisor for sub-ns should be 1.0");
        assert_eq!(unit, "ns");
        assert_eq!(dp, 2, "sub-ns should use 2 decimal places");
    }

    #[test]
    fn ns_unit_single_digit_ns() {
        // 10 <= abs < 100: 1 decimal place
        let (div, unit, dp) = ns_unit(15.0);
        assert_eq!(div, 1.0);
        assert_eq!(unit, "ns");
        assert_eq!(dp, 1, "single-digit ns range should use 1 dp");
    }

    #[test]
    fn ns_unit_triple_digit_ns() {
        // 100 <= abs < 1000: 0 decimal places
        let (div, unit, dp) = ns_unit(150.0);
        assert_eq!(div, 1.0);
        assert_eq!(unit, "ns");
        assert_eq!(dp, 0, "triple-digit ns range should use 0 dp");
    }

    #[test]
    fn ns_unit_microseconds() {
        // 1000 <= abs < 1_000_000: µs
        let (div, unit, dp) = ns_unit(1_500.0);
        assert_eq!(div, 1_000.0);
        assert_eq!(unit, "\u{b5}s");
        assert_eq!(dp, 1);
    }

    #[test]
    fn ns_unit_milliseconds() {
        // 1_000_000 <= abs < 1_000_000_000: ms
        let (div, unit, dp) = ns_unit(1_500_000.0);
        assert_eq!(div, 1_000_000.0);
        assert_eq!(unit, "ms");
        assert_eq!(dp, 1);
    }

    #[test]
    fn ns_unit_seconds() {
        // >= 1_000_000_000: s
        let (div, unit, dp) = ns_unit(1_500_000_000.0);
        assert_eq!(div, 1_000_000_000.0);
        assert_eq!(unit, "s");
        assert_eq!(dp, 2);
    }

    // --- format_ns ---

    #[test]
    fn format_ns_sub_ns_band() {
        // < 0.01 ns: 3 decimal places
        let s = format_ns(0.005);
        assert!(s.ends_with("ns"), "sub-ns values should show ns, got: {s}");
        assert!(s.contains("0.005"), "expected 3 dp, got: {s}");
    }

    #[test]
    fn format_ns_ns_band() {
        let s = format_ns(500.0);
        assert_eq!(s, "500.0ns");
    }

    #[test]
    fn format_ns_microsecond_band() {
        let s = format_ns(1_500.0);
        assert!(s.contains('\u{b5}'), "expected µs unit, got: {s}");
    }

    #[test]
    fn format_ns_millisecond_band() {
        let s = format_ns(1_500_000.0);
        assert!(s.ends_with("ms"), "expected ms, got: {s}");
    }

    #[test]
    fn format_ns_second_band() {
        let s = format_ns(1_500_000_000.0);
        assert!(
            s.ends_with('s') && !s.ends_with("ms"),
            "expected s, got: {s}"
        );
    }

    #[test]
    fn format_ns_negative() {
        let s = format_ns(-1_500_000.0);
        assert!(
            s.starts_with('-'),
            "negative value should start with '-', got: {s}"
        );
        assert!(s.ends_with("ms"), "expected ms for magnitude, got: {s}");
    }

    // --- format_ns_range ---

    #[test]
    fn format_ns_range_bracket_alignment() {
        // All three values are in the µs band; columns must be same width
        let s = format_ns_range(250_000.0, 260_000.0, 270_000.0);
        // Should look like "[250.0  260.0  270.0]µs" — three equal-width fields
        assert!(s.starts_with('['), "should start with '[', got: {s}");
        assert!(s.contains('\u{b5}'), "should use µs unit, got: {s}");

        // Extract the inner part between '[' and ']'
        let inner = s.trim_start_matches('[');
        let inner = &inner[..inner.rfind(']').unwrap()];
        let parts: Vec<&str> = inner.split("  ").collect();
        assert_eq!(parts.len(), 3, "should have three columns: {s}");
        // Each rendered number should be the same string length (right-padded/aligned)
        let widths: Vec<usize> = parts.iter().map(|p| p.trim().len()).collect();
        // The formatter aligns to max width, so all fields in the bracket are same len
        let rendered_widths: Vec<usize> = parts.iter().map(|p| p.len()).collect();
        assert!(
            rendered_widths.windows(2).all(|w| w[0] == w[1]),
            "all three column fields must be the same width, got widths {:?} in '{s}'",
            rendered_widths
        );
        let _ = widths; // used above via trim
    }

    #[test]
    fn format_ns_range_sub_ns_alignment() {
        // Sub-ns values — verifies 2 dp and equal-width columns
        let s = format_ns_range(0.18, 0.19, 0.20);
        assert!(s.starts_with('['), "should start with '[', got: {s}");
        assert!(s.ends_with("]ns"), "should end with ']ns', got: {s}");

        let inner = &s[1..s.rfind(']').unwrap()];
        let parts: Vec<&str> = inner.split("  ").collect();
        assert_eq!(parts.len(), 3, "should have 3 columns in: {s}");
        let rendered_widths: Vec<usize> = parts.iter().map(|p| p.len()).collect();
        assert!(
            rendered_widths.windows(2).all(|w| w[0] == w[1]),
            "all three column fields must be equal width in '{s}', got: {:?}",
            rendered_widths
        );
    }

    #[test]
    fn format_ns_range_shared_unit() {
        // Unit appears exactly once, at the end, not inside the brackets
        let s = format_ns_range(1_000.0, 1_100.0, 1_200.0);
        // Should end with "]µs"
        let bracket_end = s.rfind(']').expect("should contain ']'");
        let suffix = &s[bracket_end + 1..];
        assert!(!suffix.is_empty(), "unit should follow ']', got: {s}");
        // Unit must not appear inside the brackets
        let inside = &s[..bracket_end];
        assert!(
            !inside.contains('\u{b5}') && !inside.contains("ns") && !inside.contains("ms"),
            "unit must not appear inside brackets, got: {s}"
        );
    }

    // --- csv_escape ---

    #[test]
    fn csv_escape_plain() {
        assert_eq!(csv_escape("simple"), "simple");
    }

    #[test]
    fn csv_escape_with_comma() {
        let escaped = csv_escape("a,b");
        assert_eq!(escaped, "\"a,b\"", "commas must trigger quoting");
    }

    #[test]
    fn csv_escape_with_double_quote() {
        // Embedded quotes are doubled
        let escaped = csv_escape("say \"hi\"");
        assert_eq!(
            escaped, "\"say \"\"hi\"\"\"",
            "embedded quotes must be doubled"
        );
    }

    #[test]
    fn csv_escape_with_newline() {
        let escaped = csv_escape("line1\nline2");
        assert!(escaped.starts_with('"'), "newlines must trigger quoting");
        assert!(escaped.ends_with('"'));
    }

    #[test]
    fn csv_escape_empty() {
        assert_eq!(csv_escape(""), "");
    }
}
