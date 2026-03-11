use serde::{Deserialize, Serialize};

/// Streaming statistical summary using Welford's online algorithm.
///
/// Calculates mean, variance, min, max in a single pass.
/// From _The Art of Computer Programming, Vol 2, page 232_.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary {
    pub n: usize,
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    pub variance: f64,
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
    pub fn from_slice(values: &[f64]) -> Self {
        let mut s = Self::new();
        for &v in values {
            s.push(v);
        }
        s
    }
}

impl Default for Summary {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of paired statistical analysis between two interleaved benchmarks.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub ci_lower: f64,
    pub ci_upper: f64,
    /// Drift correlation: Spearman rank correlation of measurement index vs time.
    /// Values near ±1 indicate systematic drift (thermal throttling, etc.).
    pub drift_correlation: f64,
}

impl PairedAnalysis {
    /// Compute paired analysis from raw paired measurements.
    ///
    /// `baseline` and `candidate` are per-iteration times in nanoseconds,
    /// from interleaved rounds. They must have the same length.
    pub fn compute(
        baseline: &[f64],
        candidate: &[f64],
        iterations_per_sample: &[usize],
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

        // Significance: z-test on paired diffs
        let std_err = diff_summary.std_err();
        let significant = if diff_summary.n > 1 {
            if std_err < f64::EPSILON {
                // Zero variance: if mean diff is non-zero, it's perfectly significant
                diff_summary.mean.abs() > f64::EPSILON
            } else {
                let z = diff_summary.mean / std_err;
                // z >= 2.576 corresponds to 99% confidence
                z.abs() >= 2.576
                    && (diff_summary.mean / base_summary.mean).abs() > 0.005
            }
        } else {
            false
        };

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
        let (ci_lower, ci_upper) = bootstrap_ci(&clean_diffs, 10_000, 0.95);

        // Drift detection: Spearman correlation of index vs diff
        let drift_correlation = spearman_correlation(&clean_diffs);

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
            ci_upper,
            drift_correlation,
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
        None => (
            baseline.to_vec(),
            candidate.to_vec(),
            diffs.to_vec(),
            0,
        ),
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
/// Uses a simple xoshiro256** PRNG to avoid depending on `rand`.
fn bootstrap_ci(values: &[f64], n_resamples: usize, confidence: f64) -> (f64, f64) {
    if values.len() < 2 {
        let m = values.first().copied().unwrap_or(0.0);
        return (m, m);
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

    (
        means[lo_idx.min(means.len() - 1)],
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
        let (lo, hi) = bootstrap_ci(&vals, 5000, 0.95);
        assert!(lo < hi);
        let mean = vals.iter().sum::<f64>() / vals.len() as f64;
        assert!(lo <= mean && mean <= hi);
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
    }
}
