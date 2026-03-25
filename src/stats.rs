use serde::{Deserialize, Serialize};

/// Streaming statistical summary using Welford's online algorithm.
///
/// Calculates mean, variance, min, max in a single pass.
/// From _The Art of Computer Programming, Vol 2, page 232_.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Summary {
    pub n: usize,
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    pub variance: f64,
    /// Median value (computed from stored samples).
    pub median: f64,
    /// Median Absolute Deviation — robust spread metric.
    /// Scaled by 1.4826 to estimate sigma for normal distributions.
    pub mad: f64,
    // Internal state for Welford's
    #[serde(skip)]
    m2: f64,
}

impl Summary {
    pub fn new() -> Self {
        Self {
            n: 0,
            min: f64::INFINITY,
            max: f64::NEG_INFINITY,
            mean: 0.0,
            variance: 0.0,
            median: 0.0,
            mad: 0.0,
            m2: 0.0,
        }
    }

    /// Add a new observation.
    pub fn push(&mut self, value: f64) {
        self.n += 1;
        if value < self.min {
            self.min = value;
        }
        if value > self.max {
            self.max = value;
        }
        let delta = value - self.mean;
        self.mean += delta / self.n as f64;
        let delta2 = value - self.mean;
        self.m2 += delta * delta2;
        self.variance = if self.n > 1 {
            self.m2 / (self.n - 1) as f64
        } else {
            0.0
        };
    }

    /// Standard deviation.
    pub fn std_dev(&self) -> f64 {
        self.variance.sqrt()
    }

    /// Standard error of the mean.
    pub fn std_err(&self) -> f64 {
        if self.n > 0 {
            self.std_dev() / (self.n as f64).sqrt()
        } else {
            0.0
        }
    }

    /// Coefficient of variation (relative standard deviation).
    pub fn cv(&self) -> f64 {
        if self.mean.abs() > f64::EPSILON {
            self.std_dev() / self.mean.abs()
        } else {
            0.0
        }
    }

    /// Build from a slice of values.
    ///
    /// Computes all statistics including median and MAD, which require
    /// the full dataset (can't be computed incrementally).
    pub fn from_slice(values: &[f64]) -> Self {
        let mut s = Self::new();
        for &v in values {
            s.push(v);
        }

        if !values.is_empty() {
            let mut sorted = values.to_vec();
            sorted.sort_unstable_by(|a, b| a.total_cmp(b));

            // Median
            let n = sorted.len();
            s.median = if n.is_multiple_of(2) {
                (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
            } else {
                sorted[n / 2]
            };

            // MAD: median of |x_i - median|, scaled by 1.4826
            let mut deviations: Vec<f64> = sorted.iter().map(|&x| (x - s.median).abs()).collect();
            deviations.sort_unstable_by(|a, b| a.total_cmp(b));
            let raw_mad = if n.is_multiple_of(2) {
                (deviations[n / 2 - 1] + deviations[n / 2]) / 2.0
            } else {
                deviations[n / 2]
            };
            s.mad = raw_mad * 1.4826;
        }

        s
    }
}

impl Default for Summary {
    fn default() -> Self {
        Self::new()
    }
}

/// Bootstrap confidence interval for a single benchmark's mean.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct MeanCi {
    /// Lower bound of 95% CI on the mean (ns).
    pub lower: f64,
    /// Bootstrap median of the mean (ns).
    pub median: f64,
    /// Upper bound of 95% CI on the mean (ns).
    pub upper: f64,
}

impl MeanCi {
    /// Compute a bootstrap CI for the mean from raw per-iteration samples.
    pub fn from_samples(samples: &[f64], n_resamples: usize) -> Option<Self> {
        if samples.len() < 2 {
            return None;
        }
        let (lower, median, upper) = bootstrap_ci(samples, n_resamples, 0.95);
        Some(Self {
            lower,
            median,
            upper,
        })
    }
}

/// OLS slope estimate through the origin: `time = slope × iterations`.
///
/// Used with linear sampling mode. Separates per-iteration cost from
/// constant overhead (timer calls, function dispatch, black_box).
pub fn slope_estimate(iter_counts: &[f64], elapsed_ns: &[f64]) -> Option<(f64, f64)> {
    if iter_counts.len() != elapsed_ns.len() || iter_counts.len() < 3 {
        return None;
    }

    // OLS through origin: slope = Σ(x·y) / Σ(x²)
    let mut sum_xy = 0.0_f64;
    let mut sum_xx = 0.0_f64;
    let mut sum_yy = 0.0_f64;
    let n = iter_counts.len();

    for i in 0..n {
        let x = iter_counts[i];
        let y = elapsed_ns[i];
        sum_xy += x * y;
        sum_xx += x * x;
        sum_yy += y * y;
    }

    if sum_xx < f64::EPSILON {
        return None;
    }

    let slope = sum_xy / sum_xx;

    // R² = 1 - SS_res / SS_tot (through origin)
    let ss_res: f64 = (0..n)
        .map(|i| {
            let pred = slope * iter_counts[i];
            (elapsed_ns[i] - pred).powi(2)
        })
        .sum();
    let r_squared = if sum_yy > f64::EPSILON {
        1.0 - ss_res / sum_yy
    } else {
        1.0
    };

    Some((slope, r_squared))
}

/// Bootstrap CI for the slope estimate.
#[allow(dead_code)] // Public API for future use
pub fn slope_ci(
    iter_counts: &[f64],
    elapsed_ns: &[f64],
    n_resamples: usize,
) -> Option<(f64, f64, f64)> {
    if iter_counts.len() != elapsed_ns.len() || iter_counts.len() < 3 {
        return None;
    }

    let n = iter_counts.len();
    let mut rng = Xoshiro256SS::seed(0x534C_4F50_4500_0001); // "SLOPE"
    let mut slopes = Vec::with_capacity(n_resamples);

    for _ in 0..n_resamples {
        let mut sum_xy = 0.0_f64;
        let mut sum_xx = 0.0_f64;
        for _ in 0..n {
            let idx = (rng.next_u64() as usize) % n;
            let x = iter_counts[idx];
            let y = elapsed_ns[idx];
            sum_xy += x * y;
            sum_xx += x * x;
        }
        if sum_xx > f64::EPSILON {
            slopes.push(sum_xy / sum_xx);
        }
    }

    if slopes.len() < 10 {
        return None;
    }

    slopes.sort_unstable_by(|a, b| a.total_cmp(b));
    let lo = slopes[slopes.len() * 25 / 1000]; // 2.5th percentile
    let mid = slopes[slopes.len() / 2];
    let hi = slopes[slopes.len() * 975 / 1000]; // 97.5th percentile

    Some((lo, mid, hi))
}

/// Result of paired statistical analysis between two interleaved benchmarks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PairedAnalysis {
    /// Summary of per-iteration times for baseline.
    pub baseline: Summary,
    /// Summary of per-iteration times for candidate.
    pub candidate: Summary,
    /// Summary of paired differences (candidate - baseline), per iteration.
    pub diff: Summary,
    /// Percentage change: (candidate - baseline) / baseline * 100.
    /// Negative = candidate is faster.
    pub pct_change: f64,
    /// Whether the difference is statistically significant.
    pub significant: bool,
    /// Cohen's d effect size.
    pub cohens_d: f64,
    /// Number of samples.
    pub n_samples: usize,
    /// Number of outliers detected.
    pub n_outliers: usize,
    /// Bootstrap 95% confidence interval for the mean difference (ns).
    /// All three are percentiles of the bootstrap distribution.
    pub ci_lower: f64,
    /// Bootstrap median of the paired difference (p50).
    pub ci_median: f64,
    pub ci_upper: f64,
    /// Drift correlation: Spearman rank correlation of measurement index vs time.
    /// Values near ±1 indicate systematic drift (thermal throttling, etc.).
    pub drift_correlation: f64,
    /// Wilcoxon signed-rank test p-value (two-sided).
    /// Non-parametric — valid even for non-normal benchmark distributions.
    pub wilcoxon_p: f64,
}

impl PairedAnalysis {
    /// Compute paired analysis from raw paired measurements.
    ///
    /// `baseline` and `candidate` are per-iteration times in nanoseconds,
    /// from interleaved rounds. They must have the same length.
    ///
    /// `n_resamples`: bootstrap iterations (default 10,000).
    /// `noise_threshold`: fraction of baseline (0.01 = 1%). When set > 0,
    /// significance requires the CI to fall entirely outside ±threshold.
    #[allow(dead_code)] // Used by tests and result helpers
    pub(crate) fn compute(
        baseline: &[f64],
        candidate: &[f64],
        iterations_per_sample: &[usize],
    ) -> Option<Self> {
        Self::compute_with_config(baseline, candidate, iterations_per_sample, 10_000, 0.0)
    }

    /// Compute paired analysis with configurable bootstrap resamples and noise threshold.
    pub(crate) fn compute_with_config(
        baseline: &[f64],
        candidate: &[f64],
        iterations_per_sample: &[usize],
        n_resamples: usize,
        noise_threshold: f64,
    ) -> Option<Self> {
        if baseline.len() != candidate.len()
            || baseline.len() != iterations_per_sample.len()
            || baseline.is_empty()
        {
            return None;
        }

        let n = baseline.len();

        // Normalize to per-iteration times
        let base_norm: Vec<f64> = baseline
            .iter()
            .zip(iterations_per_sample)
            .map(|(&t, &iters)| t / iters as f64)
            .collect();
        let cand_norm: Vec<f64> = candidate
            .iter()
            .zip(iterations_per_sample)
            .map(|(&t, &iters)| t / iters as f64)
            .collect();

        // Compute paired diffs
        let diffs: Vec<f64> = cand_norm
            .iter()
            .zip(base_norm.iter())
            .map(|(&c, &b)| c - b)
            .collect();

        // IQR outlier detection on diffs
        let (clean_base, clean_cand, clean_diffs, n_outliers) =
            filter_outliers_paired(&base_norm, &cand_norm, &diffs);

        let base_summary = Summary::from_slice(&clean_base);
        let cand_summary = Summary::from_slice(&clean_cand);
        let diff_summary = Summary::from_slice(&clean_diffs);

        // Significance: based on whether the 95% bootstrap CI excludes zero.
        // This is more robust than a z-test (no normality assumption) and
        // doesn't mix in arbitrary practical-significance thresholds.
        // The CI is computed below; we set `significant` after.

        let pct_change = if base_summary.mean.abs() > f64::EPSILON {
            diff_summary.mean / base_summary.mean * 100.0
        } else {
            0.0
        };

        // Cohen's d: effect size
        let pooled_sd = ((base_summary.variance + cand_summary.variance) / 2.0).sqrt();
        let cohens_d = if pooled_sd > f64::EPSILON {
            diff_summary.mean / pooled_sd
        } else {
            0.0
        };

        // Bootstrap 95% CI on mean diff
        let (ci_lower, ci_median, ci_upper) = bootstrap_ci(&clean_diffs, n_resamples, 0.95);

        // Drift detection: Spearman correlation of index vs diff
        let drift_correlation = spearman_correlation(&clean_diffs);

        // Wilcoxon signed-rank test (non-parametric)
        let wilcoxon_p = wilcoxon_signed_rank(&clean_diffs);

        // Significance: 95% CI must exclude zero AND the noise threshold.
        // Without noise_threshold (0.0): pure CI-based — CI must not straddle zero.
        // With noise_threshold (e.g., 0.01): CI must be entirely outside ±1% of baseline.
        // This prevents "statistically significant but unmeasurably small" reports.
        let significant = if noise_threshold > 0.0 && base_summary.mean.abs() > f64::EPSILON {
            let threshold_ns = base_summary.mean.abs() * noise_threshold;
            ci_lower > threshold_ns || ci_upper < -threshold_ns
        } else {
            ci_lower > 0.0 || ci_upper < 0.0
        };

        Some(PairedAnalysis {
            baseline: base_summary,
            candidate: cand_summary,
            diff: diff_summary,
            pct_change,
            significant,
            cohens_d,
            n_samples: n,
            n_outliers,
            ci_lower,
            ci_median,
            ci_upper,
            drift_correlation,
            wilcoxon_p,
        })
    }
}

/// Filter outliers using Tukey's IQR method on paired diffs.
/// Returns cleaned (baseline, candidate, diffs) and outlier count.
fn filter_outliers_paired(
    baseline: &[f64],
    candidate: &[f64],
    diffs: &[f64],
) -> (Vec<f64>, Vec<f64>, Vec<f64>, usize) {
    let range = iqr_range(diffs);

    match range {
        Some((lo, hi)) => {
            let mut clean_b = Vec::new();
            let mut clean_c = Vec::new();
            let mut clean_d = Vec::new();
            let mut outliers = 0;

            for i in 0..diffs.len() {
                if diffs[i] >= lo && diffs[i] <= hi {
                    clean_b.push(baseline[i]);
                    clean_c.push(candidate[i]);
                    clean_d.push(diffs[i]);
                } else {
                    outliers += 1;
                }
            }
            (clean_b, clean_c, clean_d, outliers)
        }
        None => (baseline.to_vec(), candidate.to_vec(), diffs.to_vec(), 0),
    }
}

/// Compute IQR fences. Returns (lower_fence, upper_fence).
fn iqr_range(values: &[f64]) -> Option<(f64, f64)> {
    if values.len() < 4 {
        return None;
    }

    let mut sorted = values.to_vec();
    sorted.sort_unstable_by(|a, b| a.total_cmp(b));

    let q1_idx = sorted.len() / 4;
    let q3_idx = sorted.len() * 3 / 4;

    if q1_idx >= q3_idx {
        return None;
    }

    let q1 = sorted[q1_idx];
    let q3 = sorted[q3_idx];
    let iqr = (q3 - q1).max(1.0); // Minimum IQR of 1ns

    Some((q1 - 1.5 * iqr, q3 + 1.5 * iqr))
}

/// Bootstrap confidence interval for the mean of `values`.
///
/// Returns (lower, median, upper) — the lower and upper bounds of the CI
/// plus the bootstrap median. All three are percentiles of the bootstrap
/// distribution, so they're internally consistent.
///
/// Uses a simple xoshiro256** PRNG to avoid depending on `rand`.
pub(crate) fn bootstrap_ci(values: &[f64], n_resamples: usize, confidence: f64) -> (f64, f64, f64) {
    if values.len() < 2 {
        let m = values.first().copied().unwrap_or(0.0);
        return (m, m, m);
    }

    let n = values.len();
    let mut rng = Xoshiro256SS::seed(0x5A45_4E42_454E_4348); // "ZENBENCH" in hex-ish
    let mut means = Vec::with_capacity(n_resamples);

    for _ in 0..n_resamples {
        let mut sum = 0.0;
        for _ in 0..n {
            let idx = (rng.next_u64() as usize) % n;
            sum += values[idx];
        }
        means.push(sum / n as f64);
    }

    means.sort_unstable_by(|a, b| a.total_cmp(b));

    let alpha = 1.0 - confidence;
    let lo_idx = ((alpha / 2.0) * means.len() as f64) as usize;
    let hi_idx = ((1.0 - alpha / 2.0) * means.len() as f64) as usize;

    let med_idx = means.len() / 2;
    (
        means[lo_idx.min(means.len() - 1)],
        means[med_idx],
        means[hi_idx.min(means.len() - 1)],
    )
}

/// Spearman rank correlation between index (0..n) and values.
///
/// Returns a value in [-1, 1]. Values near ±1 indicate systematic drift.
fn spearman_correlation(values: &[f64]) -> f64 {
    let n = values.len();
    if n < 3 {
        return 0.0;
    }

    // Rank the values
    let mut indexed: Vec<(usize, f64)> = values.iter().copied().enumerate().collect();
    indexed.sort_unstable_by(|a, b| a.1.total_cmp(&b.1));

    let mut ranks = vec![0.0f64; n];
    for (rank, &(orig_idx, _)) in indexed.iter().enumerate() {
        ranks[orig_idx] = rank as f64;
    }

    // Spearman's rho = 1 - 6 * sum(d^2) / (n * (n^2 - 1))
    let mut sum_d2 = 0.0;
    for (i, &rank) in ranks.iter().enumerate() {
        let d = i as f64 - rank;
        sum_d2 += d * d;
    }

    let n_f = n as f64;
    1.0 - (6.0 * sum_d2) / (n_f * (n_f * n_f - 1.0))
}

/// Wilcoxon signed-rank test (two-sided) on paired differences.
///
/// Tests H0: the median of the differences is zero.
/// Uses normal approximation with continuity correction for n >= 10.
/// Returns p-value. For small n (< 10), returns 1.0 (insufficient data).
fn wilcoxon_signed_rank(diffs: &[f64]) -> f64 {
    // Remove zeros (ties with zero)
    let nonzero: Vec<f64> = diffs
        .iter()
        .copied()
        .filter(|&d| d.abs() > f64::EPSILON)
        .collect();
    let n = nonzero.len();

    if n < 10 {
        return 1.0; // Not enough data for normal approximation
    }

    // Rank by absolute value
    let mut indexed: Vec<(usize, f64)> = nonzero.iter().copied().enumerate().collect();
    indexed.sort_unstable_by(|a, b| a.1.abs().total_cmp(&b.1.abs()));

    // Assign ranks (average ties)
    let mut ranks = vec![0.0f64; n];
    let mut i = 0;
    while i < n {
        let mut j = i;
        while j < n && (indexed[j].1.abs() - indexed[i].1.abs()).abs() < f64::EPSILON {
            j += 1;
        }
        // Average rank for tied group
        let avg_rank = (i + j + 1) as f64 / 2.0; // 1-based
        for item in &indexed[i..j] {
            ranks[item.0] = avg_rank;
        }
        i = j;
    }

    // W+ = sum of ranks for positive differences
    let w_plus: f64 = nonzero
        .iter()
        .zip(ranks.iter())
        .filter(|&(&d, _)| d > 0.0)
        .map(|(_, r)| *r)
        .sum();

    // Normal approximation
    let n_f = n as f64;
    let expected = n_f * (n_f + 1.0) / 4.0;
    let variance = n_f * (n_f + 1.0) * (2.0 * n_f + 1.0) / 24.0;
    let sd = variance.sqrt();

    if sd < f64::EPSILON {
        return 1.0;
    }

    // z with continuity correction
    let z = (w_plus - expected).abs() - 0.5;
    let z = z / sd;

    // Two-sided p-value from normal distribution
    // Use the complementary error function approximation
    2.0 * normal_cdf_complement(z)
}

/// Approximation of 1 - Φ(x) for x >= 0 using Abramowitz & Stegun 26.2.17.
/// Accurate to ~1.5e-7.
fn normal_cdf_complement(x: f64) -> f64 {
    if x < 0.0 {
        return 1.0 - normal_cdf_complement(-x);
    }

    let p = 0.2316419;
    let b1 = 0.319381530;
    let b2 = -0.356563782;
    let b3 = 1.781477937;
    let b4 = -1.821255978;
    let b5 = 1.330274429;

    let t = 1.0 / (1.0 + p * x);
    let t2 = t * t;
    let t3 = t2 * t;
    let t4 = t3 * t;
    let t5 = t4 * t;

    let pdf = (-0.5 * x * x).exp() / (2.0 * std::f64::consts::PI).sqrt();
    pdf * (b1 * t + b2 * t2 + b3 * t3 + b4 * t4 + b5 * t5)
}

/// Minimal xoshiro256** PRNG. No external deps, deterministic, fast.
pub(crate) struct Xoshiro256SS {
    s: [u64; 4],
}

impl Xoshiro256SS {
    pub(crate) fn seed(seed: u64) -> Self {
        // SplitMix64 to expand seed into state
        let mut s = [0u64; 4];
        let mut z = seed;
        for slot in &mut s {
            z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            *slot = z ^ (z >> 31);
        }
        Self { s }
    }

    pub(crate) fn next_u64(&mut self) -> u64 {
        let result = (self.s[1].wrapping_mul(5)).rotate_left(7).wrapping_mul(9);
        let t = self.s[1] << 17;

        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];

        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(45);

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_basic() {
        let s = Summary::from_slice(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        assert_eq!(s.n, 5);
        assert!((s.mean - 3.0).abs() < 1e-10);
        assert!((s.variance - 2.5).abs() < 1e-10);
        assert_eq!(s.min, 1.0);
        assert_eq!(s.max, 5.0);
        assert!((s.median - 3.0).abs() < 1e-10);
        // MAD of [1,2,3,4,5]: deviations from median 3 are [2,1,0,1,2]
        // sorted: [0,1,1,2,2], median=1, scaled=1.4826
        assert!((s.mad - 1.4826).abs() < 1e-10);
    }

    #[test]
    fn summary_empty() {
        let s = Summary::from_slice(&[]);
        assert_eq!(s.n, 0);
    }

    #[test]
    fn summary_single() {
        let s = Summary::from_slice(&[42.0]);
        assert_eq!(s.n, 1);
        assert!((s.mean - 42.0).abs() < 1e-10);
        assert!((s.variance - 0.0).abs() < 1e-10);
        assert!((s.median - 42.0).abs() < 1e-10);
        assert!((s.mad - 0.0).abs() < 1e-10);
    }

    #[test]
    fn summary_median_even_count() {
        let s = Summary::from_slice(&[1.0, 2.0, 3.0, 4.0]);
        // Median of even count: average of middle two = (2+3)/2 = 2.5
        assert!((s.median - 2.5).abs() < 1e-10);
    }

    #[test]
    fn iqr_filters_outliers() {
        let mut values: Vec<f64> = (0..20).map(|i| i as f64).collect();
        values.push(1000.0); // outlier
        values.push(-500.0); // outlier
        let range = iqr_range(&values);
        assert!(range.is_some());
        let (lo, hi) = range.unwrap();
        assert!(lo < 0.0);
        assert!(hi < 1000.0);
    }

    #[test]
    fn spearman_perfect_correlation() {
        let vals: Vec<f64> = (0..10).map(|i| i as f64).collect();
        let r = spearman_correlation(&vals);
        assert!((r - 1.0).abs() < 1e-10);
    }

    #[test]
    fn spearman_no_correlation() {
        // Alternating values have weak correlation
        let vals = vec![1.0, 10.0, 2.0, 9.0, 3.0, 8.0, 4.0, 7.0];
        let r = spearman_correlation(&vals);
        assert!(r.abs() < 0.5);
    }

    #[test]
    fn bootstrap_ci_basic() {
        let vals: Vec<f64> = (0..100).map(|i| 100.0 + i as f64 * 0.01).collect();
        let (lo, _med, hi) = bootstrap_ci(&vals, 5000, 0.95);
        assert!(lo < hi);
        let mean = vals.iter().sum::<f64>() / vals.len() as f64;
        assert!(lo <= mean && mean <= hi);
    }

    #[test]
    fn bootstrap_ci_ordering() {
        // lo <= median <= hi must always hold
        let vals: Vec<f64> = (0..50).map(|i| i as f64).collect();
        let (lo, med, hi) = bootstrap_ci(&vals, 2000, 0.95);
        assert!(
            lo <= med,
            "bootstrap lo should be <= median: lo={lo} med={med}"
        );
        assert!(
            med <= hi,
            "bootstrap median should be <= hi: med={med} hi={hi}"
        );
    }

    #[test]
    fn bootstrap_ci_median_near_mean_for_symmetric_data() {
        // For symmetric data the bootstrap median should be close to the sample mean
        let vals: Vec<f64> = (0..100).map(|i| i as f64).collect(); // 0..99, symmetric
        let sample_mean = vals.iter().sum::<f64>() / vals.len() as f64; // 49.5
        let (_lo, med, _hi) = bootstrap_ci(&vals, 5000, 0.95);
        let tol = 2.0; // allow 2 ns of drift
        assert!(
            (med - sample_mean).abs() < tol,
            "bootstrap median ({med:.2}) should be close to sample mean ({sample_mean:.2})"
        );
    }

    #[test]
    fn bootstrap_ci_single_value() {
        // Single-element slice: all three values are that element
        let (lo, med, hi) = bootstrap_ci(&[42.0], 1000, 0.95);
        assert_eq!(lo, 42.0);
        assert_eq!(med, 42.0);
        assert_eq!(hi, 42.0);
    }

    #[test]
    fn bootstrap_ci_excludes_zero_for_all_positive() {
        // If all diffs are strongly positive, CI should exclude zero (significant)
        let vals: Vec<f64> = (0..100).map(|_| 10.0_f64).collect();
        let (lo, _med, hi) = bootstrap_ci(&vals, 2000, 0.95);
        assert!(
            lo > 0.0,
            "CI lower bound should be > 0 for all-positive data, got lo={lo}"
        );
        assert!(
            hi > 0.0,
            "CI upper bound should be > 0 for all-positive data, got hi={hi}"
        );
    }

    #[test]
    fn xoshiro_deterministic() {
        let mut rng1 = Xoshiro256SS::seed(42);
        let mut rng2 = Xoshiro256SS::seed(42);
        for _ in 0..100 {
            assert_eq!(rng1.next_u64(), rng2.next_u64());
        }
    }

    #[test]
    fn paired_analysis_identical() {
        let base = vec![100.0; 50];
        let cand = vec![100.0; 50];
        let iters = vec![1usize; 50];
        let result = PairedAnalysis::compute(&base, &cand, &iters).unwrap();
        assert!(!result.significant);
        assert!((result.pct_change).abs() < 1e-10);
        // ci_median must be populated and in-order
        assert!(
            result.ci_lower <= result.ci_median,
            "ci_lower ({}) should be <= ci_median ({})",
            result.ci_lower,
            result.ci_median
        );
        assert!(
            result.ci_median <= result.ci_upper,
            "ci_median ({}) should be <= ci_upper ({})",
            result.ci_median,
            result.ci_upper
        );
    }

    #[test]
    fn paired_analysis_faster_zero_variance() {
        // Synthetic: zero variance, so Cohen's d is undefined (0)
        let base = vec![100.0; 100];
        let cand = vec![80.0; 100];
        let iters = vec![1usize; 100];
        let result = PairedAnalysis::compute(&base, &cand, &iters).unwrap();
        assert!(result.significant);
        assert!(result.pct_change < -10.0);
        // ci_median must be populated: for zero-variance all-negative diffs
        // it must be negative and in-order
        assert!(
            result.ci_lower <= result.ci_median,
            "ci_lower ({}) should be <= ci_median ({})",
            result.ci_lower,
            result.ci_median
        );
        assert!(
            result.ci_median <= result.ci_upper,
            "ci_median ({}) should be <= ci_upper ({})",
            result.ci_median,
            result.ci_upper
        );
        assert!(
            result.ci_median < 0.0,
            "ci_median ({}) should be negative when candidate is faster",
            result.ci_median
        );
    }

    #[test]
    fn paired_analysis_faster_with_noise() {
        // Realistic: some noise, but clear difference
        let mut rng = Xoshiro256SS::seed(42);
        let base: Vec<f64> = (0..200)
            .map(|_| 100.0 + (rng.next_u64() % 10) as f64 - 5.0)
            .collect();
        let cand: Vec<f64> = (0..200)
            .map(|_| 80.0 + (rng.next_u64() % 10) as f64 - 5.0)
            .collect();
        let iters = vec![1usize; 200];
        let result = PairedAnalysis::compute(&base, &cand, &iters).unwrap();
        assert!(result.significant);
        assert!(result.pct_change < -10.0);
        assert!(result.cohens_d < 0.0);
        // ci_median is populated and in-order
        assert!(
            result.ci_lower <= result.ci_median,
            "ci_lower ({}) should be <= ci_median ({})",
            result.ci_lower,
            result.ci_median
        );
        assert!(
            result.ci_median <= result.ci_upper,
            "ci_median ({}) should be <= ci_upper ({})",
            result.ci_median,
            result.ci_upper
        );
    }

    #[test]
    fn wilcoxon_no_difference() {
        // Symmetric around zero — should not be significant
        let diffs: Vec<f64> = (0..50)
            .map(|i| if i % 2 == 0 { 1.0 } else { -1.0 })
            .collect();
        let p = wilcoxon_signed_rank(&diffs);
        assert!(p > 0.05, "p={p} should be non-significant");
    }

    #[test]
    fn wilcoxon_clear_difference() {
        // All positive — should be highly significant
        let diffs: Vec<f64> = (1..=30).map(|i| i as f64).collect();
        let p = wilcoxon_signed_rank(&diffs);
        assert!(p < 0.01, "p={p} should be highly significant");
    }

    #[test]
    fn paired_analysis_slower() {
        let mut rng = Xoshiro256SS::seed(99);
        let base: Vec<f64> = (0..200)
            .map(|_| 100.0 + (rng.next_u64() % 10) as f64 - 5.0)
            .collect();
        let cand: Vec<f64> = (0..200)
            .map(|_| 120.0 + (rng.next_u64() % 10) as f64 - 5.0)
            .collect();
        let iters = vec![1usize; 200];
        let result = PairedAnalysis::compute(&base, &cand, &iters).unwrap();
        assert!(result.significant);
        assert!(result.pct_change > 10.0);
        // ci_median must be positive and in-order when candidate is slower
        assert!(
            result.ci_lower <= result.ci_median,
            "ci_lower ({}) should be <= ci_median ({})",
            result.ci_lower,
            result.ci_median
        );
        assert!(
            result.ci_median <= result.ci_upper,
            "ci_median ({}) should be <= ci_upper ({})",
            result.ci_median,
            result.ci_upper
        );
        assert!(
            result.ci_median > 0.0,
            "ci_median ({}) should be positive when candidate is slower",
            result.ci_median
        );
    }

    // ── Slope regression tests ──────────────────────────────────────

    #[test]
    fn slope_estimate_linear_relationship() {
        // Perfect linear: y = 10 * x (10ns per iteration)
        let xs: Vec<f64> = (1..=20).map(|x| x as f64).collect();
        let ys: Vec<f64> = xs.iter().map(|&x| 10.0 * x).collect();
        let (slope, r2) = slope_estimate(&xs, &ys).unwrap();
        assert!(
            (slope - 10.0).abs() < 0.001,
            "slope should be ~10, got {slope}"
        );
        assert!(
            (r2 - 1.0).abs() < 0.001,
            "R² should be ~1.0 for perfect fit, got {r2}"
        );
    }

    #[test]
    fn slope_estimate_with_noise() {
        // y = 5 * x + noise
        let mut rng = Xoshiro256SS::seed(42);
        let xs: Vec<f64> = (1..=100).map(|x| x as f64).collect();
        let ys: Vec<f64> = xs
            .iter()
            .map(|&x| {
                let noise = (rng.next_u64() % 100) as f64 / 100.0 - 0.5; // ±0.5ns
                5.0 * x + noise
            })
            .collect();
        let (slope, r2) = slope_estimate(&xs, &ys).unwrap();
        assert!((slope - 5.0).abs() < 0.5, "slope should be ~5, got {slope}");
        assert!(r2 > 0.99, "R² should be > 0.99, got {r2}");
    }

    #[test]
    fn slope_estimate_too_few_points() {
        let xs = vec![1.0, 2.0];
        let ys = vec![10.0, 20.0];
        assert!(slope_estimate(&xs, &ys).is_none(), "need >= 3 points");
    }

    #[test]
    fn slope_ci_brackets_true_slope() {
        // y = 7.5 * x + noise
        let mut rng = Xoshiro256SS::seed(123);
        let xs: Vec<f64> = (1..=50).map(|x| x as f64).collect();
        let ys: Vec<f64> = xs
            .iter()
            .map(|&x| {
                let noise = (rng.next_u64() % 200) as f64 / 100.0 - 1.0;
                7.5 * x + noise
            })
            .collect();
        let (lo, mid, hi) = slope_ci(&xs, &ys, 5000).unwrap();
        assert!(lo <= mid && mid <= hi, "CI ordering: {lo} <= {mid} <= {hi}");
        assert!(
            lo < 7.5 && hi > 7.5,
            "CI [{lo}, {hi}] should contain true slope 7.5"
        );
    }

    // ── Wilcoxon edge cases ─────────────────────────────────────────

    #[test]
    fn wilcoxon_small_n_returns_one() {
        // n < 10 should return p = 1.0
        let diffs = vec![1.0, 2.0, 3.0];
        let p = wilcoxon_signed_rank(&diffs);
        assert!(
            (p - 1.0).abs() < f64::EPSILON,
            "p should be 1.0 for n<10, got {p}"
        );
    }

    #[test]
    fn spearman_inverse_correlation() {
        // Perfectly decreasing: should give r ≈ -1
        let values: Vec<f64> = (0..20).rev().map(|x| x as f64).collect();
        let r = spearman_correlation(&values);
        assert!(
            r < -0.9,
            "inverse correlation should give r < -0.9, got {r}"
        );
    }
}
