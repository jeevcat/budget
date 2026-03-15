use std::collections::HashMap;
use std::num::NonZeroU32;

use augurs::prophet::{
    IncludeHistory, IntervalWidth, Prophet, ProphetOptions, SeasonalityOption, TrainingData,
    optimizer::OptimizeOpts, wasmstan::WasmstanOptimizer,
};
use chrono::{Datelike, NaiveDate};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::models::{AccountId, BalanceSnapshot};

/// A single day's aggregated net worth.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetWorthPoint {
    pub date: NaiveDate,
    pub value: Decimal,
}

/// A single forecasted data point with confidence bounds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForecastPoint {
    pub date: NaiveDate,
    pub value: f64,
    pub lower: f64,
    pub upper: f64,
}

/// Combined historical series and forward projection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetWorthProjection {
    pub history: Vec<NetWorthPoint>,
    pub forecast: Vec<ForecastPoint>,
}

/// Reasons a projection cannot be produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectionUnavailable {
    NoData,
    InsufficientData { have: usize, need: usize },
}

impl std::fmt::Display for ProjectionUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoData => write!(f, "no balance data available"),
            Self::InsufficientData { have, need } => {
                write!(f, "need at least {need} data points, but only have {have}")
            }
        }
    }
}

/// Build a daily net worth time series from raw balance snapshots.
///
/// For each day in the range, carries forward the latest known balance per
/// account and sums them. When multiple snapshots exist for the same account
/// on the same day, the latest timestamp wins.
///
/// # Panics
///
/// Panics if date arithmetic overflows (only possible near `NaiveDate::MAX`).
#[must_use]
pub fn build_net_worth_series(snapshots: &[BalanceSnapshot]) -> Vec<NetWorthPoint> {
    if snapshots.is_empty() {
        return Vec::new();
    }

    // Sort by timestamp ascending
    let mut sorted: Vec<&BalanceSnapshot> = snapshots.iter().collect();
    sorted.sort_by_key(|s| s.snapshot_at);

    // Deduplicate: for same account + same day, keep latest timestamp
    let mut day_snapshots: HashMap<(AccountId, NaiveDate), &BalanceSnapshot> = HashMap::new();
    for s in &sorted {
        let day = s.snapshot_at.date_naive();
        let key = (s.account_id, day);
        day_snapshots.insert(key, s);
    }

    // Collect all unique days that have snapshots, and all unique accounts
    let mut snapshot_days: HashMap<NaiveDate, Vec<(AccountId, Decimal)>> = HashMap::new();
    for ((account_id, day), s) in &day_snapshots {
        snapshot_days
            .entry(*day)
            .or_default()
            .push((*account_id, s.current));
    }

    let first_day = sorted
        .first()
        .map(|s| s.snapshot_at.date_naive())
        .expect("non-empty");
    let last_day = sorted
        .last()
        .map(|s| s.snapshot_at.date_naive())
        .expect("non-empty");

    // Walk day-by-day, carrying forward balances
    let mut running: HashMap<AccountId, Decimal> = HashMap::new();
    let mut series = Vec::new();
    let mut current_day = first_day;

    while current_day <= last_day {
        if let Some(updates) = snapshot_days.get(&current_day) {
            for (account_id, balance) in updates {
                running.insert(*account_id, *balance);
            }
        }

        let total: Decimal = running.values().sum();
        series.push(NetWorthPoint {
            date: current_day,
            value: total,
        });

        current_day = current_day.succ_opt().expect("date overflow");
    }

    series
}

/// Forecast future net worth from a historical daily series.
///
/// Uses Prophet (via augurs) to fit the series and project `forecast_months`
/// into the future. `interval_width` controls the confidence band width
/// (e.g. 0.8 = 80% interval).
///
/// # Errors
///
/// Returns `ProjectionUnavailable` if the series is empty or has fewer than
/// 2 unique dates.
///
/// # Panics
///
/// Panics if date arithmetic overflows (only possible near `NaiveDate::MAX`).
pub fn forecast_net_worth(
    series: &[NetWorthPoint],
    forecast_months: u32,
    interval_width: f64,
) -> Result<Vec<ForecastPoint>, ProjectionUnavailable> {
    if series.is_empty() {
        return Err(ProjectionUnavailable::NoData);
    }
    if series.len() < 2 {
        return Err(ProjectionUnavailable::InsufficientData {
            have: series.len(),
            need: 2,
        });
    }

    let last_date = series.last().expect("non-empty").date;
    let horizon_end = add_months(last_date, forecast_months);
    let horizon_days = (horizon_end - last_date).num_days();

    // Convert to Prophet input: timestamps as i64 seconds since epoch
    let ds: Vec<i64> = series
        .iter()
        .map(|p| {
            p.date
                .and_hms_opt(0, 0, 0)
                .expect("valid time")
                .and_utc()
                .timestamp()
        })
        .collect();
    let y: Vec<f64> = series
        .iter()
        .map(|p| p.value.to_string().parse::<f64>().unwrap_or(0.0))
        .collect();

    let series_days = (series.last().expect("non-empty").date
        - series.first().expect("non-empty").date)
        .num_days();

    let yearly = if series_days >= 365 {
        SeasonalityOption::Auto
    } else {
        SeasonalityOption::Manual(false)
    };

    let opts = ProphetOptions {
        weekly_seasonality: SeasonalityOption::Manual(false),
        daily_seasonality: SeasonalityOption::Manual(false),
        yearly_seasonality: yearly,
        interval_width: IntervalWidth::try_from(interval_width).unwrap_or_default(),
        ..ProphetOptions::default()
    };

    let optimizer = WasmstanOptimizer::new();
    let mut prophet = Prophet::new(opts, optimizer);

    let training =
        TrainingData::new(ds, y.clone()).map_err(|_| ProjectionUnavailable::InsufficientData {
            have: series.len(),
            need: 2,
        })?;

    prophet
        .fit(training, OptimizeOpts::default())
        .map_err(|_| ProjectionUnavailable::InsufficientData {
            have: series.len(),
            need: 2,
        })?;

    let horizon = NonZeroU32::new(u32::try_from(horizon_days).unwrap_or(183))
        .unwrap_or(NonZeroU32::new(183).expect("183 is non-zero"));

    let prediction_data = prophet
        .make_future_dataframe(horizon, IncludeHistory::No)
        .map_err(|_| ProjectionUnavailable::InsufficientData {
            have: series.len(),
            need: 2,
        })?;

    let predictions =
        prophet
            .predict(prediction_data)
            .map_err(|_| ProjectionUnavailable::InsufficientData {
                have: series.len(),
                need: 2,
            })?;

    let forecast: Vec<ForecastPoint> = predictions
        .yhat
        .point
        .iter()
        .enumerate()
        .map(|(i, &yhat)| {
            let date = last_date
                .succ_opt()
                .expect("date overflow")
                .checked_add_signed(chrono::Duration::days(
                    i64::try_from(i).expect("forecast index fits i64"),
                ))
                .expect("date overflow");
            let lower = predictions.yhat.lower.as_ref().map_or(yhat, |l| l[i]);
            let upper = predictions.yhat.upper.as_ref().map_or(yhat, |u| u[i]);
            ForecastPoint {
                date,
                value: yhat,
                lower,
                upper,
            }
        })
        .collect();

    Ok(forecast)
}

/// Convenience wrapper: build series from snapshots and forecast.
///
/// # Errors
///
/// Returns `ProjectionUnavailable` if there are insufficient snapshots.
pub fn project_net_worth(
    snapshots: &[BalanceSnapshot],
    forecast_months: u32,
    interval_width: f64,
) -> Result<NetWorthProjection, ProjectionUnavailable> {
    let history = build_net_worth_series(snapshots);
    let forecast = forecast_net_worth(&history, forecast_months, interval_width)?;
    Ok(NetWorthProjection { history, forecast })
}

/// Add calendar months to a date, clamping to month end.
fn add_months(date: NaiveDate, months: u32) -> NaiveDate {
    let total_months = date.month0() + months;
    let year_offset = i32::try_from(total_months / 12).expect("year offset fits i32");
    let target_year = date.year() + year_offset;
    let target_month = (total_months % 12) + 1;

    // Try the same day, fall back to end of month
    NaiveDate::from_ymd_opt(target_year, target_month, date.day())
        .or_else(|| NaiveDate::from_ymd_opt(target_year, target_month, 28))
        .expect("valid date")
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    use crate::models::{AccountId, BalanceSnapshot, BalanceSnapshotId, CurrencyCode};

    use super::*;

    fn make_snapshot(account_id: AccountId, current: Decimal, date: NaiveDate) -> BalanceSnapshot {
        BalanceSnapshot {
            id: BalanceSnapshotId::new(),
            account_id,
            current,
            available: None,
            currency: "EUR".parse::<CurrencyCode>().expect("valid"),
            snapshot_at: date.and_hms_opt(12, 0, 0).expect("valid").and_utc(),
        }
    }

    fn make_snapshot_at_hour(
        account_id: AccountId,
        current: Decimal,
        date: NaiveDate,
        hour: u32,
    ) -> BalanceSnapshot {
        BalanceSnapshot {
            id: BalanceSnapshotId::new(),
            account_id,
            current,
            available: None,
            currency: "EUR".parse::<CurrencyCode>().expect("valid"),
            snapshot_at: date.and_hms_opt(hour, 0, 0).expect("valid").and_utc(),
        }
    }

    fn make_linear_series(
        start: NaiveDate,
        days: u32,
        start_val: f64,
        end_val: f64,
    ) -> Vec<NetWorthPoint> {
        (0..days)
            .map(|i| {
                let frac = if days <= 1 {
                    0.0
                } else {
                    f64::from(i) / f64::from(days - 1)
                };
                let val = start_val + (end_val - start_val) * frac;
                NetWorthPoint {
                    date: start
                        .checked_add_signed(chrono::Duration::days(i64::from(i)))
                        .expect("valid"),
                    value: Decimal::from_f64_retain(val).expect("valid decimal"),
                }
            })
            .collect()
    }

    fn d(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).expect("valid date")
    }

    // -----------------------------------------------------------------------
    // Phase A: build_net_worth_series
    // -----------------------------------------------------------------------

    #[test]
    fn empty_snapshots_produce_empty_series() {
        let series = build_net_worth_series(&[]);
        assert!(series.is_empty());
    }

    #[test]
    fn single_snapshot_single_point() {
        let a = AccountId::new();
        let snapshots = vec![make_snapshot(a, dec!(1000), d(2025, 1, 15))];
        let series = build_net_worth_series(&snapshots);
        assert_eq!(series.len(), 1);
        assert_eq!(series[0].date, d(2025, 1, 15));
        assert_eq!(series[0].value, dec!(1000));
    }

    #[test]
    fn two_accounts_single_day() {
        let a = AccountId::new();
        let b = AccountId::new();
        let snapshots = vec![
            make_snapshot(a, dec!(1000), d(2025, 1, 15)),
            make_snapshot(b, dec!(500), d(2025, 1, 15)),
        ];
        let series = build_net_worth_series(&snapshots);
        assert_eq!(series.len(), 1);
        assert_eq!(series[0].date, d(2025, 1, 15));
        assert_eq!(series[0].value, dec!(1500));
    }

    #[test]
    fn carry_forward_fills_gaps() {
        let a = AccountId::new();
        let b = AccountId::new();
        let snapshots = vec![
            make_snapshot(a, dec!(1000), d(2025, 1, 1)),
            make_snapshot(b, dec!(500), d(2025, 1, 3)),
        ];
        let series = build_net_worth_series(&snapshots);
        assert_eq!(series.len(), 3);
        assert_eq!(series[0], (d(2025, 1, 1), dec!(1000)));
        assert_eq!(series[1], (d(2025, 1, 2), dec!(1000)));
        assert_eq!(series[2], (d(2025, 1, 3), dec!(1500)));
    }

    #[test]
    fn balance_update_replaces_carried_value() {
        let a = AccountId::new();
        let snapshots = vec![
            make_snapshot(a, dec!(1000), d(2025, 1, 1)),
            make_snapshot(a, dec!(1200), d(2025, 1, 3)),
        ];
        let series = build_net_worth_series(&snapshots);
        assert_eq!(series.len(), 3);
        assert_eq!(series[0], (d(2025, 1, 1), dec!(1000)));
        assert_eq!(series[1], (d(2025, 1, 2), dec!(1000)));
        assert_eq!(series[2], (d(2025, 1, 3), dec!(1200)));
    }

    #[test]
    fn multi_account_staggered() {
        let a = AccountId::new();
        let b = AccountId::new();
        let snapshots = vec![
            make_snapshot(a, dec!(1000), d(2025, 1, 1)),
            make_snapshot(b, dec!(500), d(2025, 1, 2)),
            make_snapshot(a, dec!(1100), d(2025, 1, 3)),
        ];
        let series = build_net_worth_series(&snapshots);
        assert_eq!(series.len(), 3);
        assert_eq!(series[0], (d(2025, 1, 1), dec!(1000)));
        assert_eq!(series[1], (d(2025, 1, 2), dec!(1500)));
        assert_eq!(series[2], (d(2025, 1, 3), dec!(1600)));
    }

    #[test]
    fn unsorted_input_handled() {
        let a = AccountId::new();
        let b = AccountId::new();
        // Same data as multi_account_staggered but in reverse order
        let snapshots = vec![
            make_snapshot(a, dec!(1100), d(2025, 1, 3)),
            make_snapshot(b, dec!(500), d(2025, 1, 2)),
            make_snapshot(a, dec!(1000), d(2025, 1, 1)),
        ];
        let series = build_net_worth_series(&snapshots);
        assert_eq!(series.len(), 3);
        assert_eq!(series[0], (d(2025, 1, 1), dec!(1000)));
        assert_eq!(series[1], (d(2025, 1, 2), dec!(1500)));
        assert_eq!(series[2], (d(2025, 1, 3), dec!(1600)));
    }

    #[test]
    fn negative_balances() {
        let a = AccountId::new();
        let b = AccountId::new();
        let snapshots = vec![
            make_snapshot(a, dec!(1000), d(2025, 1, 1)),
            make_snapshot(b, dec!(-200), d(2025, 1, 1)),
        ];
        let series = build_net_worth_series(&snapshots);
        assert_eq!(series.len(), 1);
        assert_eq!(series[0].date, d(2025, 1, 1));
        assert_eq!(series[0].value, dec!(800));
    }

    #[test]
    fn same_day_same_account_uses_latest_timestamp() {
        let a = AccountId::new();
        let snapshots = vec![
            make_snapshot_at_hour(a, dec!(900), d(2025, 1, 5), 8),
            make_snapshot_at_hour(a, dec!(1000), d(2025, 1, 5), 16),
        ];
        let series = build_net_worth_series(&snapshots);
        assert_eq!(series.len(), 1);
        assert_eq!(series[0].date, d(2025, 1, 5));
        assert_eq!(series[0].value, dec!(1000));
    }

    // Implement PartialEq for test assertions: (date, value) tuples
    impl PartialEq<(NaiveDate, Decimal)> for NetWorthPoint {
        fn eq(&self, other: &(NaiveDate, Decimal)) -> bool {
            self.date == other.0 && self.value == other.1
        }
    }

    // -----------------------------------------------------------------------
    // Phase B: forecast_net_worth
    // -----------------------------------------------------------------------

    #[test]
    fn no_data_returns_error() {
        let err = forecast_net_worth(&[], 6, 0.8).unwrap_err();
        assert_eq!(err, ProjectionUnavailable::NoData);
    }

    #[test]
    fn insufficient_data_returns_error() {
        let series = vec![NetWorthPoint {
            date: d(2025, 1, 1),
            value: dec!(1000),
        }];
        let err = forecast_net_worth(&series, 6, 0.8).unwrap_err();
        assert_eq!(
            err,
            ProjectionUnavailable::InsufficientData { have: 1, need: 2 }
        );
    }

    #[test]
    fn forecast_length_matches_horizon() {
        let series = make_linear_series(d(2025, 1, 1), 90, 10000.0, 15000.0);
        let forecast = forecast_net_worth(&series, 6, 0.8).expect("should succeed");

        let last_historical = d(2025, 1, 1)
            .checked_add_signed(chrono::Duration::days(89))
            .expect("valid");
        let expected_end = add_months(last_historical, 6);
        let expected_days = (expected_end - last_historical).num_days();

        assert_eq!(forecast.len(), expected_days as usize);
        // All forecast dates are after last historical date
        for p in &forecast {
            assert!(p.date > last_historical);
        }
    }

    #[test]
    fn confidence_bands_bracket_estimate() {
        let series = make_linear_series(d(2025, 1, 1), 90, 10000.0, 15000.0);
        let forecast = forecast_net_worth(&series, 6, 0.8).expect("should succeed");
        for p in &forecast {
            assert!(
                p.lower <= p.value,
                "lower {} > value {} on {}",
                p.lower,
                p.value,
                p.date
            );
            assert!(
                p.value <= p.upper,
                "value {} > upper {} on {}",
                p.value,
                p.upper,
                p.date
            );
        }
    }

    #[test]
    fn upward_trend_continues() {
        let series = make_linear_series(d(2025, 1, 1), 100, 10000.0, 20000.0);
        let forecast = forecast_net_worth(&series, 6, 0.8).expect("should succeed");
        assert!(!forecast.is_empty());
        let first = forecast.first().expect("non-empty").value;
        let last = forecast.last().expect("non-empty").value;
        assert!(
            last > first,
            "upward trend should continue: first={first}, last={last}"
        );
    }

    #[test]
    fn sparse_series_still_forecasts() {
        // Prophet needs enough data to fit; build a short but dense-enough series
        let series = make_linear_series(d(2025, 1, 1), 14, 10000.0, 11000.0);
        let result = forecast_net_worth(&series, 6, 0.8);
        assert!(
            result.is_ok(),
            "14 daily points should be enough: {result:?}"
        );
    }

    // -----------------------------------------------------------------------
    // Phase C: project_net_worth (integration)
    // -----------------------------------------------------------------------

    #[test]
    fn end_to_end_from_snapshots() {
        let a = AccountId::new();
        let b = AccountId::new();
        let mut snapshots = Vec::new();
        for day in 0..90 {
            let date = d(2025, 1, 1)
                .checked_add_signed(chrono::Duration::days(day))
                .expect("valid");
            snapshots.push(make_snapshot(a, Decimal::from(10000 + day * 50), date));
            snapshots.push(make_snapshot(b, Decimal::from(5000 + day * 20), date));
        }

        let projection = project_net_worth(&snapshots, 6, 0.8).expect("should succeed");
        assert_eq!(projection.history.len(), 90);
        assert!(!projection.forecast.is_empty());
    }

    #[test]
    fn single_snapshot_returns_error() {
        let a = AccountId::new();
        let snapshots = vec![make_snapshot(a, dec!(1000), d(2025, 1, 1))];
        let err = project_net_worth(&snapshots, 6, 0.8).unwrap_err();
        assert_eq!(
            err,
            ProjectionUnavailable::InsufficientData { have: 1, need: 2 }
        );
    }
}
