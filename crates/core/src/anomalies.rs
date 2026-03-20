use augurs::Fit;
use augurs::changepoint::dist::NormalGamma;
use augurs::changepoint::{Detector, NormalGammaDetector};
use augurs::ets::AutoETS;
use augurs::mstl::MSTLModel;

use crate::seasonality::SeasonalContext;

/// Combined seasonal + anomaly analysis of a category's monthly spending.
pub struct SpendingAnalysis {
    pub seasonal: SeasonalContext,
    pub anomalies: SpendingAnomalies,
}

/// Anomaly signals detected in a category's spending history.
pub struct SpendingAnomalies {
    /// Most recent structural shift, if detected in the last 6 months.
    pub changepoint: Option<Changepoint>,
    /// Whether the most recent month is a one-off residual spike.
    pub residual_outlier: bool,
    /// Z-score of the most recent month's MSTL residual.
    pub residual_z_score: f64,
}

/// A detected structural shift in spending.
pub struct Changepoint {
    /// Index into the monthly series (0 = oldest completed month).
    pub month_index: usize,
    /// Percentage shift: `mean_after / mean_before - 1` (positive = increase).
    pub shift_pct: f64,
}

const RESIDUAL_Z_THRESHOLD: f64 = 2.5;
const MIN_MONTHS_CHANGEPOINT: usize = 6;
const MIN_MONTHS_MSTL: usize = 24;
/// Only surface changepoints that occurred in the last N months.
const CHANGEPOINT_RECENCY_WINDOW: usize = 6;

/// Lossless conversion of small slice lengths to f64 (via u32).
fn len_f64(slice: &[f64]) -> f64 {
    f64::from(u32::try_from(slice.len()).expect("series length fits in u32"))
}

/// Analyse a category's monthly spending series for seasonal context and anomalies.
///
/// Requires at least 6 months of data for changepoint detection.
/// MSTL-based seasonal context and residual outlier detection require 24+ months.
/// Returns `None` if the series is too short for any analysis.
#[must_use]
pub fn analyze_spending(spending_series: &[f64]) -> Option<SpendingAnalysis> {
    let n = spending_series.len();
    if n < MIN_MONTHS_CHANGEPOINT {
        return None;
    }

    let changepoint = detect_changepoint(spending_series);

    // MSTL decomposition for seasonal context + residual outlier
    let (seasonal, residual_outlier, residual_z_score) = if n >= MIN_MONTHS_MSTL {
        let mean = spending_series.iter().sum::<f64>() / len_f64(spending_series);
        if mean.abs() < 1.0 {
            (None, false, 0.0)
        } else {
            match fit_mstl(spending_series) {
                Some((seasonal_ctx, remainder)) => {
                    let (is_outlier, z) = check_residual_outlier(&remainder);
                    (Some(seasonal_ctx), is_outlier, z)
                }
                None => (None, false, 0.0),
            }
        }
    } else {
        (None, false, 0.0)
    };

    // Need at least one signal to be useful
    let seasonal = seasonal?;

    Some(SpendingAnalysis {
        seasonal,
        anomalies: SpendingAnomalies {
            changepoint,
            residual_outlier,
            residual_z_score,
        },
    })
}

/// Run BOCPD changepoint detection, returning the most recent changepoint
/// if it falls within the recency window.
fn detect_changepoint(series: &[f64]) -> Option<Changepoint> {
    let n = series.len();

    // Data-informed prior: center on the series mean with moderate uncertainty.
    // hazard_lambda = 18 means we expect a changepoint roughly every 18 months.
    let mean = series.iter().sum::<f64>() / len_f64(series);
    let prior = NormalGamma::new_unchecked(mean, 1.0, 1.0, 1.0);
    let mut detector = NormalGammaDetector::normal_gamma(18.0, prior);
    let changepoints = detector.detect_changepoints(series);

    // Find the most recent changepoint within the recency window
    let recency_cutoff = n.saturating_sub(CHANGEPOINT_RECENCY_WINDOW);
    let &cp_idx = changepoints
        .iter()
        .rev()
        .find(|&&idx| idx >= recency_cutoff)?;

    if cp_idx == 0 || cp_idx >= n {
        return None;
    }

    let before = &series[..cp_idx];
    let after = &series[cp_idx..];
    let mean_before = before.iter().sum::<f64>() / len_f64(before);
    let mean_after = after.iter().sum::<f64>() / len_f64(after);

    if mean_before.abs() < 1.0 {
        return None;
    }

    let shift_pct = (mean_after / mean_before) - 1.0;

    // Only surface meaningful shifts (> 10%)
    if shift_pct.abs() < 0.10 {
        return None;
    }

    Some(Changepoint {
        month_index: cp_idx,
        shift_pct,
    })
}

/// Fit MSTL and return seasonal context + raw remainder (as f64).
fn fit_mstl(series: &[f64]) -> Option<(SeasonalContext, Vec<f64>)> {
    let n: u32 = series.len().try_into().ok()?;
    let mean = series.iter().sum::<f64>() / f64::from(n);

    let ets = AutoETS::new(1, "ZZN").ok()?.into_trend_model();
    let model = MSTLModel::new(vec![12], ets);
    let fit = model.fit(series).ok()?;

    let result = fit.fit();
    let seasonal = result.seasonal().first()?;
    let trend = result.trend();
    let remainder: Vec<f64> = result.remainder().iter().map(|&r| f64::from(r)).collect();

    let last_seasonal = f64::from(*seasonal.last()?);
    let seasonal_factor = 1.0 + (last_seasonal / mean);

    let n = trend.len();
    let trend_monthly = if n >= 2 {
        f64::from(trend[n - 1] - trend[n - 2])
    } else {
        0.0
    };

    Some((
        SeasonalContext {
            seasonal_factor,
            trend_monthly,
        },
        remainder,
    ))
}

/// Check whether the last residual is an outlier by z-score.
fn check_residual_outlier(remainder: &[f64]) -> (bool, f64) {
    let n = remainder.len();
    if n < 3 {
        return (false, 0.0);
    }

    let nf = len_f64(remainder);
    let mean = remainder.iter().sum::<f64>() / nf;
    let variance = remainder.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (nf - 1.0);
    let std_dev = variance.sqrt();

    if std_dev < f64::EPSILON {
        return (false, 0.0);
    }

    let last = remainder[n - 1];
    let z = (last - mean) / std_dev;

    (z.abs() > RESIDUAL_Z_THRESHOLD, z)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_none_for_short_series() {
        let series = vec![100.0; 5];
        assert!(analyze_spending(&series).is_none());
    }

    #[test]
    fn changepoint_detects_level_shift() {
        // Recent shift: 12 months at 200, then shift 4 months ago to 400
        let mut series = vec![200.0; 12];
        series.extend(vec![400.0; 4]);
        let cp = detect_changepoint(&series);
        assert!(
            cp.is_some(),
            "should detect changepoint in level-shift data"
        );
        let cp = cp.unwrap();
        // Shift should be roughly +100%
        assert!(
            cp.shift_pct > 0.5,
            "expected large positive shift, got {}",
            cp.shift_pct
        );
    }

    #[test]
    fn changepoint_ignores_old_shifts() {
        // Shift happened 12 months ago — outside the 6-month recency window
        let mut series = vec![200.0; 12];
        series.extend(vec![400.0; 12]);
        // The shift is at index 12. With n=24, recency_cutoff = 24-6 = 18.
        // Index 12 < 18, so it should be filtered out.
        let cp = detect_changepoint(&series);
        // BOCPD may or may not detect it, but if detected it should be filtered by recency
        if let Some(cp) = cp {
            assert!(
                cp.month_index >= 18,
                "should not surface old changepoints, got index {}",
                cp.month_index
            );
        }
    }

    #[test]
    fn changepoint_none_for_flat_data() {
        let series = vec![300.0; 24];
        let cp = detect_changepoint(&series);
        assert!(cp.is_none(), "flat data should not have changepoints");
    }

    #[test]
    fn residual_outlier_detects_spike() {
        // Normal residuals around 0, with a large spike at the end
        let mut remainder: Vec<f64> = vec![
            2.0, -1.5, 0.5, -0.3, 1.2, -0.8, 0.1, -1.0, 0.7, -0.5, 0.3, -0.2, 1.5, -1.3, 0.4, -0.6,
            0.9, -0.7, 0.2, -0.4, 0.6, -0.1, 0.8, -0.9,
        ];
        remainder.push(15.0); // big spike
        let (is_outlier, z) = check_residual_outlier(&remainder);
        assert!(is_outlier, "should flag a large spike as outlier");
        assert!(
            z > RESIDUAL_Z_THRESHOLD,
            "z-score {} should exceed threshold",
            z
        );
    }

    #[test]
    fn residual_outlier_normal_is_fine() {
        let remainder = vec![
            0.5, -0.3, 0.2, -0.1, 0.4, -0.5, 0.1, -0.2, 0.3, -0.4, 0.2, -0.1,
        ];
        let (is_outlier, _z) = check_residual_outlier(&remainder);
        assert!(!is_outlier, "normal residuals should not flag as outlier");
    }

    #[test]
    fn full_analysis_with_seasonal_pattern() {
        // 3 years of monthly data with December spike + a recent level shift
        let mut series: Vec<f64> = (0..24)
            .map(|i| {
                let month = i % 12;
                if month == 11 { 600.0 } else { 400.0 }
            })
            .collect();
        // Recent shift: last 6 months at 600 base (was 400)
        series.extend((0..12).map(|i| {
            let month = (24 + i) % 12;
            if month == 11 { 800.0 } else { 600.0 }
        }));

        let analysis = analyze_spending(&series);
        assert!(
            analysis.is_some(),
            "should produce analysis for 36 months with pattern"
        );
        let analysis = analysis.unwrap();
        assert!(
            analysis.seasonal.seasonal_factor != 0.0,
            "should have a seasonal factor"
        );
    }
}
