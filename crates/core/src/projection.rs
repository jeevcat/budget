use std::collections::{HashMap, HashSet};
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
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetWorthPoint {
    pub date: NaiveDate,
    #[cfg_attr(feature = "openapi", schema(value_type = String))]
    pub value: Decimal,
}

/// A single forecasted data point with confidence bounds.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForecastPoint {
    pub date: NaiveDate,
    pub value: f64,
    pub lower: f64,
    pub upper: f64,
}

/// Combined historical series and forward projection.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
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
    FitFailed(String),
}

impl std::fmt::Display for ProjectionUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoData => write!(f, "no balance data available"),
            Self::InsufficientData { have, need } => {
                write!(f, "need at least {need} data points, but only have {have}")
            }
            Self::FitFailed(msg) => write!(f, "forecast model failed: {msg}"),
        }
    }
}

/// Build a daily net worth time series from raw balance snapshots.
///
/// For each day in the range, carries forward the latest known balance per
/// account and sums them. When multiple snapshots exist for the same account
/// on the same day, the latest timestamp wins.
///
/// When `daily_deltas` is non-empty, the function interpolates between
/// consecutive snapshots using transaction data. A snapshot-to-snapshot
/// interval is only interpolated if the sum of daily deltas matches the
/// balance change (within ±0.01), proving transactions are complete for
/// that period. Days after the last snapshot are extended using deltas
/// unconditionally (best-effort).
///
/// # Panics
///
/// Panics if date arithmetic overflows (only possible near `NaiveDate::MAX`).
#[must_use]
#[allow(clippy::implicit_hasher)]
pub fn build_net_worth_series(
    snapshots: &[BalanceSnapshot],
    daily_deltas: &HashMap<(AccountId, NaiveDate), Decimal>,
) -> Vec<NetWorthPoint> {
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
    let mut last_day = sorted
        .last()
        .map(|s| s.snapshot_at.date_naive())
        .expect("non-empty");

    // Extend last_day to cover deltas after the last snapshot
    if let Some(&max_delta_date) = daily_deltas.keys().map(|(_, d)| d).max()
        && max_delta_date > last_day
    {
        last_day = max_delta_date;
    }

    // Build reconciled intervals: for each account, walk consecutive snapshot
    // pairs and check if transaction deltas fully explain the balance change.
    let interpolatable = build_interpolatable_set(&day_snapshots, daily_deltas);

    // Per-account sorted snapshot dates for linear interpolation lookups
    let mut account_snap_dates: HashMap<AccountId, Vec<NaiveDate>> = HashMap::new();
    for &(account_id, day) in day_snapshots.keys() {
        account_snap_dates.entry(account_id).or_default().push(day);
    }
    for dates in account_snap_dates.values_mut() {
        dates.sort();
    }

    // Walk day-by-day, carrying forward balances
    let mut running: HashMap<AccountId, Decimal> = HashMap::new();
    let mut series = Vec::new();
    let mut current_day = first_day;

    while current_day <= last_day {
        // Apply snapshot resets (authoritative)
        let mut snapshot_accounts = HashSet::new();
        if let Some(updates) = snapshot_days.get(&current_day) {
            for (account_id, balance) in updates {
                running.insert(*account_id, *balance);
                snapshot_accounts.insert(*account_id);
            }
        }

        // Interpolate between snapshots or apply transaction deltas
        for (account_id, balance) in &mut running {
            if snapshot_accounts.contains(account_id) {
                continue;
            }

            if interpolatable.contains(&(*account_id, current_day)) {
                // Reconciled interval: apply exact transaction deltas
                if let Some(delta) = daily_deltas.get(&(*account_id, current_day)) {
                    *balance += delta;
                }
            } else if let Some(lerped) = lerp_between_snapshots(
                *account_id,
                current_day,
                &account_snap_dates,
                &day_snapshots,
            ) {
                // Non-reconciled interval with a future snapshot: linear interpolation
                *balance = lerped;
            } else if let Some(delta) = daily_deltas.get(&(*account_id, current_day)) {
                // Tail region (past last snapshot): apply deltas unconditionally
                *balance += delta;
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

/// Determine which (account, date) pairs fall in reconciled snapshot intervals.
///
/// For each account, walks consecutive snapshot pairs chronologically. If the
/// sum of daily deltas between two snapshots matches the balance change
/// (within ±0.01), every date in that interval is marked as interpolatable.
fn build_interpolatable_set(
    day_snapshots: &HashMap<(AccountId, NaiveDate), &BalanceSnapshot>,
    daily_deltas: &HashMap<(AccountId, NaiveDate), Decimal>,
) -> HashSet<(AccountId, NaiveDate)> {
    let tolerance = Decimal::new(1, 2); // 0.01
    let mut result = HashSet::new();

    // Group snapshot dates by account
    let mut account_dates: HashMap<AccountId, Vec<NaiveDate>> = HashMap::new();
    for &(account_id, day) in day_snapshots.keys() {
        account_dates.entry(account_id).or_default().push(day);
    }

    for (account_id, dates) in &mut account_dates {
        dates.sort();

        for pair in dates.windows(2) {
            let (date_a, date_b) = (pair[0], pair[1]);
            let snap_a = day_snapshots[&(*account_id, date_a)];
            let snap_b = day_snapshots[&(*account_id, date_b)];
            let balance_delta = snap_b.current - snap_a.current;

            // Sum deltas for days (date_a + 1) through date_b inclusive
            let mut txn_sum = Decimal::ZERO;
            let mut day = date_a.succ_opt().expect("date overflow");
            while day <= date_b {
                if let Some(delta) = daily_deltas.get(&(*account_id, day)) {
                    txn_sum += delta;
                }
                day = day.succ_opt().expect("date overflow");
            }

            let diff = (txn_sum - balance_delta).abs();
            if diff <= tolerance {
                // Mark all days in (date_a, date_b] as interpolatable
                let mut day = date_a.succ_opt().expect("date overflow");
                while day <= date_b {
                    result.insert((*account_id, day));
                    day = day.succ_opt().expect("date overflow");
                }
            }
        }
    }

    result
}

/// Linearly interpolate an account's balance between surrounding snapshots.
///
/// Returns `Some(value)` if `current_day` falls strictly between two snapshots
/// for the given account. Returns `None` if there is no enclosing interval
/// (e.g. before the first snapshot or after the last).
fn lerp_between_snapshots(
    account_id: AccountId,
    current_day: NaiveDate,
    account_snap_dates: &HashMap<AccountId, Vec<NaiveDate>>,
    day_snapshots: &HashMap<(AccountId, NaiveDate), &BalanceSnapshot>,
) -> Option<Decimal> {
    let dates = account_snap_dates.get(&account_id)?;

    // Binary search for the first snapshot date > current_day
    let idx = dates.partition_point(|&d| d <= current_day);
    if idx == 0 || idx >= dates.len() {
        return None;
    }

    let date_a = dates[idx - 1];
    let date_b = dates[idx];
    let span = (date_b - date_a).num_days();
    if span == 0 {
        return None;
    }

    let elapsed = (current_day - date_a).num_days();
    let bal_a = day_snapshots[&(account_id, date_a)].current;
    let bal_b = day_snapshots[&(account_id, date_b)].current;

    let frac = Decimal::from(elapsed) / Decimal::from(span);
    Some(bal_a + (bal_b - bal_a) * frac)
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

    let training = TrainingData::new(ds, y.clone())
        .map_err(|e| ProjectionUnavailable::FitFailed(e.to_string()))?;

    prophet
        .fit(training, OptimizeOpts::default())
        .map_err(|e| ProjectionUnavailable::FitFailed(e.to_string()))?;

    let horizon = NonZeroU32::new(u32::try_from(horizon_days).unwrap_or(183))
        .unwrap_or(NonZeroU32::new(183).expect("183 is non-zero"));

    let prediction_data = prophet
        .make_future_dataframe(horizon, IncludeHistory::No)
        .map_err(|e| ProjectionUnavailable::FitFailed(e.to_string()))?;

    let predictions = prophet
        .predict(prediction_data)
        .map_err(|e| ProjectionUnavailable::FitFailed(e.to_string()))?;

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
#[allow(clippy::implicit_hasher)]
pub fn project_net_worth(
    snapshots: &[BalanceSnapshot],
    daily_deltas: &HashMap<(AccountId, NaiveDate), Decimal>,
    forecast_months: u32,
    interval_width: f64,
) -> Result<NetWorthProjection, ProjectionUnavailable> {
    let history = build_net_worth_series(snapshots, daily_deltas);
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
        let series = build_net_worth_series(&[], &HashMap::new());
        assert!(series.is_empty());
    }

    #[test]
    fn single_snapshot_single_point() {
        let a = AccountId::new();
        let snapshots = vec![make_snapshot(a, dec!(1000), d(2025, 1, 15))];
        let series = build_net_worth_series(&snapshots, &HashMap::new());
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
        let series = build_net_worth_series(&snapshots, &HashMap::new());
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
        let series = build_net_worth_series(&snapshots, &HashMap::new());
        assert_eq!(series.len(), 3);
        assert_eq!(series[0], (d(2025, 1, 1), dec!(1000)));
        assert_eq!(series[1], (d(2025, 1, 2), dec!(1000)));
        assert_eq!(series[2], (d(2025, 1, 3), dec!(1500)));
    }

    #[test]
    fn balance_update_lerps_between_snapshots() {
        let a = AccountId::new();
        let snapshots = vec![
            make_snapshot(a, dec!(1000), d(2025, 1, 1)),
            make_snapshot(a, dec!(1200), d(2025, 1, 3)),
        ];
        let series = build_net_worth_series(&snapshots, &HashMap::new());
        assert_eq!(series.len(), 3);
        assert_eq!(series[0], (d(2025, 1, 1), dec!(1000)));
        assert_eq!(series[1], (d(2025, 1, 2), dec!(1100)));
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
        let series = build_net_worth_series(&snapshots, &HashMap::new());
        assert_eq!(series.len(), 3);
        assert_eq!(series[0], (d(2025, 1, 1), dec!(1000)));
        // A lerps to 1050, B first appears at 500
        assert_eq!(series[1], (d(2025, 1, 2), dec!(1550)));
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
        let series = build_net_worth_series(&snapshots, &HashMap::new());
        assert_eq!(series.len(), 3);
        assert_eq!(series[0], (d(2025, 1, 1), dec!(1000)));
        // A lerps to 1050, B first appears at 500
        assert_eq!(series[1], (d(2025, 1, 2), dec!(1550)));
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
        let series = build_net_worth_series(&snapshots, &HashMap::new());
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
        let series = build_net_worth_series(&snapshots, &HashMap::new());
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

        let projection =
            project_net_worth(&snapshots, &HashMap::new(), 6, 0.8).expect("should succeed");
        assert_eq!(projection.history.len(), 90);
        assert!(!projection.forecast.is_empty());
    }

    #[test]
    fn single_snapshot_returns_error() {
        let a = AccountId::new();
        let snapshots = vec![make_snapshot(a, dec!(1000), d(2025, 1, 1))];
        let err = project_net_worth(&snapshots, &HashMap::new(), 6, 0.8).unwrap_err();
        assert_eq!(
            err,
            ProjectionUnavailable::InsufficientData { have: 1, need: 2 }
        );
    }

    // -----------------------------------------------------------------------
    // Phase D: delta interpolation
    // -----------------------------------------------------------------------

    #[test]
    fn deltas_interpolate_reconciled_intervals() {
        let a = AccountId::new();
        let snapshots = vec![
            make_snapshot(a, dec!(1000), d(2025, 1, 1)),
            make_snapshot(a, dec!(1050), d(2025, 1, 4)),
        ];
        let deltas = HashMap::from([
            ((a, d(2025, 1, 2)), dec!(20)),
            ((a, d(2025, 1, 3)), dec!(30)),
        ]);
        let series = build_net_worth_series(&snapshots, &deltas);
        assert_eq!(series.len(), 4);
        assert_eq!(series[0], (d(2025, 1, 1), dec!(1000)));
        assert_eq!(series[1], (d(2025, 1, 2), dec!(1020)));
        assert_eq!(series[2], (d(2025, 1, 3), dec!(1050)));
        assert_eq!(series[3], (d(2025, 1, 4), dec!(1050)));
    }

    #[test]
    fn non_reconciled_intervals_use_linear_interpolation() {
        let a = AccountId::new();
        // 2-day span: lerp gives 1000, 1050, 1100
        let snapshots = vec![
            make_snapshot(a, dec!(1000), d(2025, 1, 1)),
            make_snapshot(a, dec!(1100), d(2025, 1, 3)),
        ];
        // Sum = 50, but balance delta = 100 — not reconciled
        let deltas = HashMap::from([((a, d(2025, 1, 2)), dec!(50))]);
        let series = build_net_worth_series(&snapshots, &deltas);
        assert_eq!(series.len(), 3);
        assert_eq!(series[0], (d(2025, 1, 1), dec!(1000)));
        assert_eq!(series[1], (d(2025, 1, 2), dec!(1050)));
        assert_eq!(series[2], (d(2025, 1, 3), dec!(1100)));
    }

    #[test]
    fn deltas_per_account_independent_reconciliation() {
        let a = AccountId::new();
        let b = AccountId::new();
        let snapshots = vec![
            make_snapshot(a, dec!(1000), d(2025, 1, 1)),
            make_snapshot(a, dec!(1050), d(2025, 1, 3)),
            make_snapshot(b, dec!(500), d(2025, 1, 1)),
            make_snapshot(b, dec!(700), d(2025, 1, 3)),
        ];
        // Account A: delta sum = 50, balance change = 50 — reconciled
        // Account B: delta sum = 80, balance change = 200 — NOT reconciled (lerped)
        let deltas = HashMap::from([
            ((a, d(2025, 1, 2)), dec!(50)),
            ((b, d(2025, 1, 2)), dec!(80)),
        ]);
        let series = build_net_worth_series(&snapshots, &deltas);
        assert_eq!(series.len(), 3);
        // Day 1: A=1000, B=500 => 1500
        assert_eq!(series[0], (d(2025, 1, 1), dec!(1500)));
        // Day 2: A=1050 (txn delta), B=600 (lerp: 500 + 200*1/2) => 1650
        assert_eq!(series[1], (d(2025, 1, 2), dec!(1650)));
        // Day 3: A=1050 (snapshot), B=700 (snapshot) => 1750
        assert_eq!(series[2], (d(2025, 1, 3), dec!(1750)));
    }

    #[test]
    fn deltas_extend_series_past_last_snapshot() {
        let a = AccountId::new();
        let snapshots = vec![make_snapshot(a, dec!(1000), d(2025, 1, 1))];
        let deltas = HashMap::from([
            ((a, d(2025, 1, 2)), dec!(50)),
            ((a, d(2025, 1, 3)), dec!(-20)),
        ]);
        let series = build_net_worth_series(&snapshots, &deltas);
        assert_eq!(series.len(), 3);
        assert_eq!(series[0], (d(2025, 1, 1), dec!(1000)));
        assert_eq!(series[1], (d(2025, 1, 2), dec!(1050)));
        assert_eq!(series[2], (d(2025, 1, 3), dec!(1030)));
    }
}
