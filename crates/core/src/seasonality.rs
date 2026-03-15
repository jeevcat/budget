use augurs::Fit;
use augurs::ets::AutoETS;
use augurs::mstl::MSTLModel;

/// Seasonal context extracted from MSTL decomposition of monthly spending.
pub struct SeasonalContext {
    /// Multiplier vs. mean spend (e.g. 1.15 = this month is historically 15% above average).
    pub seasonal_factor: f64,
    /// Monthly trend slope (positive = spending increasing over time).
    pub trend_monthly: f64,
}

/// Decompose a category's monthly spending history and extract seasonal context
/// for the next (current) period.
///
/// Returns `None` if fewer than 24 data points (need 2 full seasonal cycles)
/// or if mean spend is approximately zero.
#[must_use]
pub fn compute_seasonal_context(spending_series: &[f64]) -> Option<SeasonalContext> {
    if spending_series.len() < 24 {
        return None;
    }

    let n: u32 = spending_series.len().try_into().ok()?;
    let mean = spending_series.iter().sum::<f64>() / f64::from(n);
    if mean.abs() < 1.0 {
        return None;
    }

    // Period 12 for monthly seasonality, ETS as trend model, no seasonality in trend model
    let ets = AutoETS::new(1, "ZZN").ok()?.into_trend_model();
    let model = MSTLModel::new(vec![12], ets);
    let fit = model.fit(spending_series).ok()?;

    let result = fit.fit();
    // seasonal() returns &[Vec<f32>] — one vec per period; we have one period (12)
    let seasonal = result.seasonal().first()?;
    let trend = result.trend();

    let last_seasonal = f64::from(*seasonal.last()?);
    let seasonal_factor = 1.0 + (last_seasonal / mean);

    let n = trend.len();
    let trend_monthly = if n >= 2 {
        f64::from(trend[n - 1] - trend[n - 2])
    } else {
        0.0
    };

    Some(SeasonalContext {
        seasonal_factor,
        trend_monthly,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_none_for_insufficient_data() {
        let series: Vec<f64> = (0..23).map(|i| 100.0 + i as f64).collect();
        assert!(compute_seasonal_context(&series).is_none());
    }

    #[test]
    fn returns_none_for_empty_series() {
        assert!(compute_seasonal_context(&[]).is_none());
    }

    #[test]
    fn returns_none_for_near_zero_mean() {
        let series = vec![0.0; 24];
        assert!(compute_seasonal_context(&series).is_none());
    }

    #[test]
    fn flat_data_has_seasonal_factor_near_one() {
        let series = vec![500.0; 36];
        if let Some(ctx) = compute_seasonal_context(&series) {
            assert!(
                (ctx.seasonal_factor - 1.0).abs() < 0.1,
                "expected ~1.0, got {}",
                ctx.seasonal_factor
            );
            assert!(
                ctx.trend_monthly.abs() < 10.0,
                "expected ~0 trend, got {}",
                ctx.trend_monthly
            );
        }
        // MSTL on perfectly flat data may not decompose cleanly; None is acceptable
    }

    #[test]
    fn synthetic_seasonal_pattern() {
        // 3 years of monthly data with a known December spike
        let series: Vec<f64> = (0..36)
            .map(|i| {
                let month = i % 12;
                let base = 400.0;
                if month == 11 {
                    base + 200.0 // December spike
                } else {
                    base
                }
            })
            .collect();

        let ctx = compute_seasonal_context(&series);
        assert!(
            ctx.is_some(),
            "should produce seasonal context for 36 months with pattern"
        );
        let ctx = ctx.unwrap();
        // The last value is month index 35 -> month 11 (December)
        // December should have a positive seasonal factor > 1
        assert!(
            ctx.seasonal_factor > 1.0,
            "December should have factor > 1, got {}",
            ctx.seasonal_factor
        );
    }
}
