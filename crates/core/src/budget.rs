use std::collections::{BTreeMap, HashSet};
use std::num::NonZeroU32;

use chrono::{Datelike, NaiveDate};
use rust_decimal::Decimal;
use serde::Serialize;
use uuid::Uuid;

use crate::error::Error;
use crate::models::{
    BudgetConfig, BudgetMode, BudgetMonth, BudgetMonthId, BudgetStatus, BudgetType, Category,
    CategoryId, CorrelationType, PaceIndicator, ProjectChildSpending, Transaction, TransactionId,
};

/// Derive a deterministic UUID from a start date so the same budget month
/// always gets the same ID across requests.
fn deterministic_month_id(start_date: NaiveDate) -> BudgetMonthId {
    // Simple hash: XOR a fixed namespace with the date string bytes,
    // then set version 4 and variant bits for a valid UUID.
    let date_str = start_date.to_string();
    let namespace: &[u8; 16] = b"budget-month-ns!";
    let mut bytes = *namespace;
    for (i, b) in date_str.as_bytes().iter().enumerate() {
        bytes[i % 16] ^= b;
    }
    // Set version 4 (random) and variant 1 bits for a valid UUID
    bytes[6] = (bytes[6] & 0x0F) | 0x40;
    bytes[8] = (bytes[8] & 0x3F) | 0x80;
    BudgetMonthId::from_uuid(Uuid::from_bytes(bytes))
}

/// Collect all descendant category IDs for a given category (including itself).
#[must_use]
pub fn collect_category_subtree(
    category_id: CategoryId,
    categories: &[Category],
) -> Vec<CategoryId> {
    let mut result = vec![category_id];
    let mut stack = vec![category_id];

    while let Some(current) = stack.pop() {
        for cat in categories {
            if cat.parent_id == Some(current) {
                result.push(cat.id);
                stack.push(cat.id);
            }
        }
    }

    result
}

/// Collect category IDs that belong to a project-mode category (or whose
/// ancestor is project-mode). Used to exclude project transactions from
/// regular budget math.
fn project_category_ids(categories: &[Category]) -> HashSet<CategoryId> {
    let project_roots: Vec<CategoryId> = categories
        .iter()
        .filter(|c| c.budget.mode() == Some(BudgetMode::Project))
        .map(|c| c.id)
        .collect();

    let mut ids = HashSet::new();
    for root in project_roots {
        for id in collect_category_subtree(root, categories) {
            ids.insert(id);
        }
    }
    ids
}

/// Collect category IDs that belong to a salary-mode category (or whose
/// ancestor is salary-mode). Used to exclude salary transactions from
/// regular budget math and to discover salary categories by mode.
#[must_use]
pub fn salary_category_ids(categories: &[Category]) -> HashSet<CategoryId> {
    let salary_roots: Vec<CategoryId> = categories
        .iter()
        .filter(|c| c.budget.mode() == Some(BudgetMode::Salary))
        .map(|c| c.id)
        .collect();

    let mut ids = HashSet::new();
    for root in salary_roots {
        for id in collect_category_subtree(root, categories) {
            ids.insert(id);
        }
    }
    ids
}

/// Collect category IDs that belong to a transfer-mode category (or whose
/// ancestor is transfer-mode). Used to exclude internal transfers from
/// regular budget math.
#[must_use]
pub fn transfer_category_ids(categories: &[Category]) -> HashSet<CategoryId> {
    let transfer_roots: Vec<CategoryId> = categories
        .iter()
        .filter(|c| c.budget.mode() == Some(BudgetMode::Transfer))
        .map(|c| c.id)
        .collect();

    let mut ids = HashSet::new();
    for root in transfer_roots {
        for id in collect_category_subtree(root, categories) {
            ids.insert(id);
        }
    }
    ids
}

// ---------------------------------------------------------------------------
// Shared transaction exclusion helpers
// ---------------------------------------------------------------------------

/// Collect IDs of transactions whose expenses have been reimbursed.
///
/// A reimbursement transaction points at its original via `correlation.partner_id`.
fn reimbursed_transaction_ids(transactions: &[Transaction]) -> HashSet<TransactionId> {
    transactions
        .iter()
        .filter(|t| {
            t.correlation
                .as_ref()
                .is_some_and(|c| c.correlation_type == CorrelationType::Reimbursement)
        })
        .filter_map(|t| t.correlation.as_ref().map(|c| c.partner_id))
        .collect()
}

/// Whether a transaction should be excluded from budget math due to correlation.
///
/// Excludes: transfers, reimbursements, and the original expenses that were reimbursed.
fn is_correlated_exclusion(txn: &Transaction, reimbursed_ids: &HashSet<TransactionId>) -> bool {
    if let Some(c) = &txn.correlation
        && (c.correlation_type == CorrelationType::Transfer
            || c.correlation_type == CorrelationType::Reimbursement)
    {
        return true;
    }
    reimbursed_ids.contains(&txn.id)
}

/// Effective budget mode for a category: its own mode, or inherited from parent.
#[must_use]
pub fn effective_budget_mode(cat: &Category, categories: &[Category]) -> Option<BudgetMode> {
    if let Some(mode) = cat.budget.mode() {
        return Some(mode);
    }
    cat.parent_id
        .and_then(|pid| categories.iter().find(|c| c.id == pid))
        .and_then(|p| p.budget.mode())
}

/// Filter transactions to only include those relevant to regular budget math.
///
/// Excludes:
/// - Transactions in project-mode categories (project isolation)
/// - Correlated transfers (net to zero, not an expense)
/// - Correlated reimbursements on the reimbursing side (the original expense
///   is also excluded since the reimbursement nets it out)
#[must_use]
pub fn filter_for_budget<'a>(
    transactions: &'a [Transaction],
    categories: &[Category],
) -> Vec<&'a Transaction> {
    let project_cats = project_category_ids(categories);
    let salary_cats = salary_category_ids(categories);
    let transfer_cats = transfer_category_ids(categories);
    let reimbursed_ids = reimbursed_transaction_ids(transactions);

    transactions
        .iter()
        .filter(|t| {
            // Exclude transactions in project-mode categories
            if t.categorization
                .category_id()
                .is_some_and(|cid| project_cats.contains(&cid))
            {
                return false;
            }
            // Exclude transactions in salary-mode categories
            if t.categorization
                .category_id()
                .is_some_and(|cid| salary_cats.contains(&cid))
            {
                return false;
            }
            // Exclude transactions in transfer-mode categories
            if t.categorization
                .category_id()
                .is_some_and(|cid| transfer_cats.contains(&cid))
            {
                return false;
            }
            !is_correlated_exclusion(t, &reimbursed_ids)
        })
        .collect()
}

/// Detect budget month boundaries from salary transactions.
///
/// Scans transactions for deposits in the salary category, groups them by
/// calendar month, and creates a `BudgetMonth` starting on the day the last
/// expected salary posts in each calendar month. The previous budget month's
/// end date is set to the day before the next one starts.
///
/// # Errors
///
/// Returns `Error::NoSalaryCategory` if no salary-mode category exists.
pub fn detect_budget_month_boundaries(
    transactions: &[Transaction],
    expected_salary_count: NonZeroU32,
    categories: &[Category],
) -> Result<Vec<BudgetMonth>, Error> {
    let salary_cat_ids: Vec<CategoryId> = salary_category_ids(categories).into_iter().collect();
    if salary_cat_ids.is_empty() {
        return Err(Error::NoSalaryCategory);
    }

    // Find salary transactions (positive amounts in salary categories)
    let mut salary_txns: Vec<&Transaction> = transactions
        .iter()
        .filter(|t| {
            t.categorization
                .category_id()
                .is_some_and(|cid| salary_cat_ids.contains(&cid))
                && t.amount > Decimal::ZERO
        })
        .collect();

    salary_txns.sort_by_key(|t| t.posted_date);

    // Group salary transactions by calendar month (year, month)
    let mut by_month: BTreeMap<(i32, u32), Vec<NaiveDate>> = BTreeMap::new();

    for txn in &salary_txns {
        let key = (txn.posted_date.year(), txn.posted_date.month());
        by_month.entry(key).or_default().push(txn.posted_date);
    }

    // For each calendar month that has >= expected_salary_count deposits,
    // the budget month starts on the first salary date so all deposits
    // fall within the month range
    let mut budget_months: Vec<BudgetMonth> = Vec::new();

    for dates in by_month.values() {
        if dates.len() >= usize::try_from(expected_salary_count.get()).unwrap_or(usize::MAX)
            && let Some(&first_salary_date) = dates.iter().min()
        {
            let detected: u32 = dates.len().try_into().unwrap_or(u32::MAX);
            budget_months.push(BudgetMonth {
                id: deterministic_month_id(first_salary_date),
                start_date: first_salary_date,
                end_date: None,
                salary_transactions_detected: detected,
            });
        }
    }

    budget_months.sort_by_key(|bm| bm.start_date);

    // Set end dates: each month ends the day before the next one starts
    for i in 0..budget_months.len().saturating_sub(1) {
        let next_start = budget_months[i + 1].start_date;
        budget_months[i].end_date = next_start.pred_opt();
    }

    // Extend the first budget month backwards to cover any non-salary
    // transactions that predate it, so they aren't orphaned without a budget month.
    if let Some(first_month) = budget_months.first_mut() {
        let earliest = transactions
            .iter()
            .filter(|t| {
                !t.categorization
                    .category_id()
                    .is_some_and(|cid| salary_cat_ids.contains(&cid))
            })
            .map(|t| t.posted_date)
            .min();
        if let Some(earliest) = earliest
            && earliest < first_month.start_date
        {
            first_month.start_date = earliest;
        }
    }

    Ok(budget_months)
}

/// Collect descendant category IDs whose budget mode is compatible with the root.
///
/// Children that declare their own explicit budget mode different from
/// `root_mode` are excluded (along with their subtrees), because they are
/// tracked under a separate budget entry.
#[must_use]
pub fn collect_budget_subtree(category_id: CategoryId, categories: &[Category]) -> Vec<CategoryId> {
    let mut result = vec![category_id];
    let mut stack = vec![category_id];

    while let Some(current) = stack.pop() {
        for cat in categories {
            if cat.parent_id == Some(current) {
                // Skip children that have their own explicit budget —
                // they get a separate StatusEntry and tracking their
                // spending here would double-count on the frontend.
                if cat.budget.mode().is_some() {
                    continue;
                }
                result.push(cat.id);
                stack.push(cat.id);
            }
        }
    }

    result
}

/// Sum spending in a category (including children) for a budget month.
///
/// Respects category hierarchy: parent includes all children's spending.
/// Excludes children that have their own different budget mode (they are
/// tracked separately), as well as project-mode, transfer, and reimbursed
/// transactions.
/// Sum spending from pre-filtered transactions for a set of category IDs
/// in a budget month.
fn compute_spending_for_subtree(
    filtered_txns: &[&Transaction],
    subtree: &[CategoryId],
    budget_month: &BudgetMonth,
) -> Decimal {
    -filtered_txns
        .iter()
        .filter(|t| {
            t.categorization
                .category_id()
                .is_some_and(|cid| subtree.contains(&cid))
                && is_in_budget_month(t.posted_date, budget_month)
        })
        .fold(Decimal::ZERO, |acc, t| acc + t.amount)
}

/// Sum spending in a category (including children) for a budget month.
///
/// Respects category hierarchy: parent includes all children's spending.
/// Excludes children that have their own different budget mode (they are
/// tracked separately), as well as project-mode, transfer, and reimbursed
/// transactions.
#[must_use]
pub fn compute_category_spending(
    transactions: &[Transaction],
    category_id: CategoryId,
    budget_month: &BudgetMonth,
    categories: &[Category],
) -> Decimal {
    let subtree = collect_budget_subtree(category_id, categories);
    let budget_txns = filter_for_budget(transactions, categories);

    compute_spending_for_subtree(&budget_txns, &subtree, budget_month)
}

/// Check if a date falls within a budget month's boundaries.
#[must_use]
pub fn is_in_budget_month(date: NaiveDate, budget_month: &BudgetMonth) -> bool {
    if date < budget_month.start_date {
        return false;
    }
    match budget_month.end_date {
        Some(end) => date <= end,
        None => true, // Open-ended month (current month)
    }
}

/// Build a per-month spending series for a category across closed budget months.
///
/// Returns one `f64` value per budget month (positive = money spent).
/// Excludes the current open month (no `end_date`) since incomplete data
/// would skew decomposition.
#[must_use]
pub fn build_monthly_spending_series(
    transactions: &[Transaction],
    category_id: CategoryId,
    budget_months: &[BudgetMonth],
    categories: &[Category],
) -> Vec<f64> {
    let subtree = collect_budget_subtree(category_id, categories);
    let filtered = filter_for_budget(transactions, categories);

    budget_months
        .iter()
        .filter(|bm| bm.end_date.is_some())
        .map(|bm| {
            let spent = compute_spending_for_subtree(&filtered, &subtree, bm);
            spent.to_string().parse::<f64>().unwrap_or(0.0)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Salary arrival prediction
// ---------------------------------------------------------------------------

/// Status of a single salary source (one root salary category).
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Debug, Clone, Serialize)]
pub struct SalarySourceStatus {
    pub category_id: CategoryId,
    pub category_name: String,
    pub arrived: bool,
    pub arrived_date: Option<NaiveDate>,
    /// ETS point forecast (day-of-month)
    pub predicted_day: Option<u32>,
    /// 80% confidence lower bound (day-of-month)
    pub predicted_day_lower: Option<u32>,
    /// 80% confidence upper bound (day-of-month)
    pub predicted_day_upper: Option<u32>,
    /// Today is past the upper bound and salary hasn't arrived
    pub late: bool,
}

/// Aggregate salary arrival status across all sources.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Debug, Clone, Serialize)]
pub struct SalaryStatus {
    pub expected_count: u32,
    pub arrived_count: u32,
    pub sources: Vec<SalarySourceStatus>,
    pub any_late: bool,
    pub all_arrived: bool,
}

/// Days in a given month.
fn days_in_month(year: i32, month: u32) -> u32 {
    // First day of next month minus one day
    let next = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)
    };
    next.and_then(|d| d.pred_opt()).map_or(28, |d| d.day())
}

/// Predict salary arrival dates for the current (open) budget month.
///
/// For each salary root category, examines historical positive transactions
/// in its subtree, extracts the earliest arrival day-of-month per calendar
/// month, and uses ETS (or median fallback) to predict when the next
/// salary will arrive.
#[must_use]
pub fn predict_salary_arrivals(
    transactions: &[Transaction],
    categories: &[Category],
    today: NaiveDate,
    expected_salary_count: NonZeroU32,
) -> SalaryStatus {
    let salary_roots: Vec<&Category> = categories
        .iter()
        .filter(|c| c.budget.mode() == Some(BudgetMode::Salary))
        .collect();

    if salary_roots.is_empty() {
        return SalaryStatus {
            expected_count: expected_salary_count.get(),
            arrived_count: 0,
            sources: Vec::new(),
            any_late: false,
            all_arrived: true,
        };
    }

    let current_ym = (today.year(), today.month());
    let max_day = days_in_month(today.year(), today.month());

    let mut sources = Vec::new();

    for root in &salary_roots {
        let subtree = collect_budget_subtree(root.id, categories);
        let subtree_set: HashSet<CategoryId> = subtree.into_iter().collect();

        // Collect positive transactions in the subtree
        let salary_txns: Vec<&Transaction> = transactions
            .iter()
            .filter(|t| {
                t.amount > Decimal::ZERO
                    && t.categorization
                        .category_id()
                        .is_some_and(|cid| subtree_set.contains(&cid))
            })
            .collect();

        // Group by (year, month), take earliest date per month
        let mut by_month: BTreeMap<(i32, u32), NaiveDate> = BTreeMap::new();
        for txn in &salary_txns {
            let key = (txn.posted_date.year(), txn.posted_date.month());
            by_month
                .entry(key)
                .and_modify(|existing| {
                    if txn.posted_date < *existing {
                        *existing = txn.posted_date;
                    }
                })
                .or_insert(txn.posted_date);
        }

        // Check if current month has an arrival
        let arrived_date = by_month.get(&current_ym).copied();
        let arrived = arrived_date.is_some();

        // Historical days (excluding current month)
        let historical_days: Vec<f64> = by_month
            .iter()
            .filter(|(ym, _)| **ym != current_ym)
            .map(|(_, date)| f64::from(date.day()))
            .collect();

        let (predicted_day, predicted_day_lower, predicted_day_upper) =
            predict_arrival_day(&historical_days, max_day);

        let late = !arrived && predicted_day_upper.is_some_and(|upper| today.day() > upper);

        sources.push(SalarySourceStatus {
            category_id: root.id,
            category_name: root.name.to_string(),
            arrived,
            arrived_date,
            predicted_day,
            predicted_day_lower,
            predicted_day_upper,
            late,
        });
    }

    let arrived_count: u32 = sources
        .iter()
        .filter(|s| s.arrived)
        .count()
        .try_into()
        .unwrap_or(u32::MAX);

    SalaryStatus {
        expected_count: expected_salary_count.get(),
        arrived_count,
        any_late: sources.iter().any(|s| s.late),
        all_arrived: sources.iter().all(|s| s.arrived),
        sources,
    }
}

/// Predict arrival day-of-month from historical data.
///
/// Returns `(point, lower, upper)` clamped to `1..=max_day`.
/// Uses ETS with ≥3 months of history, otherwise falls back to median/range.
fn predict_arrival_day(
    historical_days: &[f64],
    max_day: u32,
) -> (Option<u32>, Option<u32>, Option<u32>) {
    if historical_days.is_empty() {
        return (None, None, None);
    }

    let clamp = |v: f64| {
        let rounded = v.round().max(1.0).min(f64::from(max_day));
        // Safe: value is clamped to 1..=max_day (≤31)
        #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let day = rounded as u32;
        day
    };

    if historical_days.len() >= 3 {
        // Try ETS prediction
        if let Some((point, lower, upper)) = try_ets_predict(historical_days) {
            return (Some(clamp(point)), Some(clamp(lower)), Some(clamp(upper)));
        }
    }

    // Fallback: median as point, min/max as bounds
    let mut sorted = historical_days.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = sorted[sorted.len() / 2];
    let min = sorted.first().copied().unwrap_or(median);
    let max = sorted.last().copied().unwrap_or(median);

    (Some(clamp(median)), Some(clamp(min)), Some(clamp(max)))
}

/// Attempt ETS forecast for 1 step ahead with 80% prediction intervals.
fn try_ets_predict(data: &[f64]) -> Option<(f64, f64, f64)> {
    use augurs::Fit as _;
    use augurs::Predict as _;
    use augurs::ets::AutoETS;

    let ets = AutoETS::new(1, "ZZN").ok()?;
    let fitted = ets.fit(data).ok()?;
    let forecast = fitted.predict(1, 0.80).ok()?;
    let point = *forecast.point.first()?;
    let intervals = forecast.intervals.as_ref()?;
    let lower = *intervals.lower.first()?;
    let upper = *intervals.upper.first()?;
    Some((point, lower, upper))
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Debug, Clone, Serialize)]
pub struct DailySpendPoint {
    pub day: u16,
    #[cfg_attr(feature = "openapi", schema(value_type = String))]
    pub cumulative: Decimal,
}

/// Build a daily cumulative spending series for a category within a budget month.
///
/// Walks day-by-day from the budget month start through `through_date` (or the
/// month's end date), accumulating spending. Returns one point per day.
#[must_use]
pub fn build_daily_cumulative_series(
    filtered_txns: &[&Transaction],
    subtree: &[CategoryId],
    budget_month: &BudgetMonth,
    through_date: NaiveDate,
) -> Vec<DailySpendPoint> {
    let end = budget_month.end_date.unwrap_or(through_date);
    let end = end.min(through_date);

    let mut points = Vec::new();
    let mut cumulative = Decimal::ZERO;
    let mut current = budget_month.start_date;
    let mut day: u16 = 1;

    while current <= end {
        let day_spend: Decimal = filtered_txns
            .iter()
            .filter(|t| {
                t.posted_date == current
                    && t.categorization
                        .category_id()
                        .is_some_and(|cid| subtree.contains(&cid))
            })
            .fold(Decimal::ZERO, |acc, t| acc - t.amount);

        cumulative += day_spend;
        points.push(DailySpendPoint { day, cumulative });

        current = match current.succ_opt() {
            Some(next) => next,
            None => break,
        };
        day += 1;
    }

    points
}

/// Compute pace indicator and delta for a budget category.
///
/// Dispatches to fixed or variable logic based on `budget_type`.
/// When `seasonal_factor` is `Some`, adjusts the expected spend for variable
/// categories using the MSTL-derived seasonal multiplier.
/// Returns the pace indicator and the signed delta.
fn compute_pace(
    spent: Decimal,
    budget: Decimal,
    elapsed: i64,
    total: i64,
    budget_type: BudgetType,
    seasonal_factor: Option<f64>,
) -> (PaceIndicator, Decimal) {
    match budget_type {
        BudgetType::Fixed => compute_pace_fixed(spent, budget),
        BudgetType::Variable => {
            compute_pace_variable(spent, budget, elapsed, total, seasonal_factor)
        }
    }
}

/// Fixed expense pace: payment either hasn't arrived, is on track, or exceeded.
///
/// Only produces: `Pending`, `OnTrack`, `OverBudget`.
fn compute_pace_fixed(spent: Decimal, budget: Decimal) -> (PaceIndicator, Decimal) {
    let delta = spent - budget;
    if spent > budget {
        (PaceIndicator::OverBudget, delta)
    } else if spent == Decimal::ZERO {
        (PaceIndicator::Pending, delta)
    } else {
        (PaceIndicator::OnTrack, delta)
    }
}

/// Variable expense pace: pro-rata comparison with ±5% tolerance band.
///
/// When `seasonal_factor` is `Some`, the expected spend is adjusted by the
/// MSTL-derived seasonal multiplier, replacing naive linear pro-rata with a
/// seasonality-aware expectation.
///
/// Only produces: `UnderBudget`, `OnTrack`, `AbovePace`, `OverBudget`.
fn compute_pace_variable(
    spent: Decimal,
    budget: Decimal,
    elapsed: i64,
    total: i64,
    seasonal_factor: Option<f64>,
) -> (PaceIndicator, Decimal) {
    if total <= 0 || budget == Decimal::ZERO {
        let delta = spent - budget;
        if spent > budget {
            (PaceIndicator::OverBudget, delta)
        } else if spent == budget && budget > Decimal::ZERO {
            (PaceIndicator::OnTrack, Decimal::ZERO)
        } else {
            (PaceIndicator::OnTrack, delta)
        }
    } else {
        let fraction = Decimal::from(elapsed) / Decimal::from(total);
        let mut expected_spend = budget * fraction;
        if let Some(f) = seasonal_factor
            && let Some(factor) = Decimal::from_f64_retain(f)
        {
            expected_spend *= factor;
        }
        let delta = spent - expected_spend;
        if spent > budget {
            (PaceIndicator::OverBudget, delta)
        } else {
            let upper = expected_spend * Decimal::new(105, 2);
            let lower = expected_spend * Decimal::new(95, 2);
            if spent > upper {
                (PaceIndicator::AbovePace, delta)
            } else if spent >= lower {
                (PaceIndicator::OnTrack, delta)
            } else {
                (PaceIndicator::UnderBudget, delta)
            }
        }
    }
}

/// Return the budget months belonging to the current budget year.
///
/// The budget year starts at the first month whose `start_date` falls in January,
/// walking backwards from `reference_month`. Takes up to 12 months forward from
/// that anchor.
#[must_use]
pub fn budget_year_months<'a>(
    all_months: &'a [BudgetMonth],
    reference_month: &BudgetMonth,
) -> Vec<&'a BudgetMonth> {
    // Find the reference month's index
    let ref_idx = all_months
        .iter()
        .position(|bm| bm.id == reference_month.id)
        .unwrap_or(0);

    // Walk backwards to find the budget month that contains January 1st.
    // Budget months don't align with calendar months (salary arrives mid-to-late
    // month), so the month covering Jan 1 may start in late December.
    let ref_year = reference_month.start_date.year();
    let jan1 = NaiveDate::from_ymd_opt(ref_year, 1, 1).unwrap_or(reference_month.start_date);
    let mut year_start_idx = ref_idx;
    for i in (0..=ref_idx).rev() {
        year_start_idx = i;
        let bm = &all_months[i];
        let contains_jan1 = bm.start_date <= jan1 && bm.end_date.is_none_or(|end| end >= jan1);
        if contains_jan1 {
            break;
        }
    }

    // Take up to 12 months forward from the anchor
    let year_end_idx = (year_start_idx + 12).min(all_months.len());
    all_months[year_start_idx..year_end_idx].iter().collect()
}

/// Compute budget status for a monthly category.
fn compute_monthly_status(
    category: &Category,
    transactions: &[Transaction],
    current_month: &BudgetMonth,
    categories: &[Category],
    today: NaiveDate,
    analysis: Option<&crate::anomalies::SpendingAnalysis>,
) -> BudgetStatus {
    let spent = compute_category_spending(transactions, category.id, current_month, categories);
    let budget_amount = category.budget.amount().unwrap_or(Decimal::ZERO);
    let remaining = budget_amount - spent;

    let end_date = current_month
        .end_date
        .unwrap_or(current_month.start_date + chrono::Days::new(30));

    let time_left = (end_date - today).num_days().max(0);

    let total_days = (end_date - current_month.start_date).num_days();
    let elapsed_days = (today - current_month.start_date).num_days().max(0);
    let bt = category
        .budget
        .budget_type()
        .unwrap_or(BudgetType::Variable);
    let sf = analysis.map(|a| a.seasonal.seasonal_factor);
    let (pace, pace_delta) = compute_pace(spent, budget_amount, elapsed_days, total_days, bt, sf);

    BudgetStatus {
        category_id: category.id,
        category_name: category.name.to_string(),
        budget_amount,
        spent,
        remaining,
        time_left: Some(time_left),
        pace,
        pace_delta,
        budget_mode: BudgetMode::Monthly,
        seasonal_factor: sf,
        trend_monthly: analysis.map(|a| a.seasonal.trend_monthly),
        changepoint_shift_pct: analysis
            .and_then(|a| a.anomalies.changepoint.as_ref().map(|c| c.shift_pct)),
        residual_outlier: analysis.is_some_and(|a| a.anomalies.residual_outlier),
    }
}

/// Compute budget status for an annual category.
fn compute_annual_status(
    category: &Category,
    transactions: &[Transaction],
    current_month: &BudgetMonth,
    all_months: &[BudgetMonth],
    categories: &[Category],
    _today: NaiveDate,
) -> BudgetStatus {
    let year_months = budget_year_months(all_months, current_month);

    // Pre-filter once, then sum across all months in the budget year
    let subtree = collect_budget_subtree(category.id, categories);
    let budget_txns = filter_for_budget(transactions, categories);

    let spent = year_months
        .iter()
        .map(|bm| compute_spending_for_subtree(&budget_txns, &subtree, bm))
        .sum::<Decimal>();

    let budget_amount = category.budget.amount().unwrap_or(Decimal::ZERO);
    let remaining = budget_amount - spent;

    // Budget year is always 12 months
    let total_year_months: i64 = 12;
    // Elapsed = calendar months from the year anchor (January) to current month, inclusive
    let year_anchor = year_months
        .first()
        .map_or(current_month.start_date, |bm| bm.start_date);
    let elapsed_months = {
        let anchor_ord = year_anchor.year() * 12 + i32::try_from(year_anchor.month0()).unwrap_or(0);
        let current_ord = current_month.start_date.year() * 12
            + i32::try_from(current_month.start_date.month0()).unwrap_or(0);
        i64::from((current_ord - anchor_ord + 1).clamp(1, 12))
    };
    let time_left = (total_year_months - elapsed_months).max(0);

    let bt = category
        .budget
        .budget_type()
        .unwrap_or(BudgetType::Variable);
    let (pace, pace_delta) = compute_pace(
        spent,
        budget_amount,
        elapsed_months,
        total_year_months,
        bt,
        None,
    );

    BudgetStatus {
        category_id: category.id,
        category_name: category.name.to_string(),
        budget_amount,
        spent,
        remaining,
        time_left: Some(time_left),
        pace,
        pace_delta,
        budget_mode: BudgetMode::Annual,
        seasonal_factor: None,
        trend_monthly: None,
        changepoint_shift_pct: None,
        residual_outlier: false,
    }
}

/// Compute budget status for a project category.
fn compute_project_status(
    category: &Category,
    transactions: &[Transaction],
    categories: &[Category],
    today: NaiveDate,
) -> BudgetStatus {
    let subtree = collect_category_subtree(category.id, categories);
    let reimbursed_ids = reimbursed_transaction_ids(transactions);

    let (start, end) = match &category.budget {
        BudgetConfig::Project {
            start_date,
            end_date,
            ..
        } => (Some(*start_date), *end_date),
        _ => (None, None),
    };

    // Filter transactions within the project date range, excluding transfers/reimbursements
    let spent: Decimal = -transactions
        .iter()
        .filter(|t| {
            let in_subtree = t
                .categorization
                .category_id()
                .is_some_and(|cid| subtree.contains(&cid));
            if !in_subtree {
                return false;
            }
            if is_correlated_exclusion(t, &reimbursed_ids) {
                return false;
            }
            // Date range filter
            if let Some(s) = start
                && t.posted_date < s
            {
                return false;
            }
            if let Some(e) = end
                && t.posted_date > e
            {
                return false;
            }
            true
        })
        .fold(Decimal::ZERO, |acc, t| acc + t.amount);

    let budget_amount = category.budget.amount().unwrap_or(Decimal::ZERO);
    let remaining = budget_amount - spent;

    let bt = category
        .budget
        .budget_type()
        .unwrap_or(BudgetType::Variable);
    let (time_left, pace, pace_delta) = match (start, end) {
        (Some(s), Some(e)) if budget_amount > Decimal::ZERO => {
            let total_days = (e - s).num_days();
            let elapsed_days = (today - s).num_days().max(0);
            let tl = (e - today).num_days().max(0);
            let (p, d) = compute_pace(spent, budget_amount, elapsed_days, total_days, bt, None);
            (Some(tl), p, d)
        }
        // Open-ended or no budget: can't compute pace
        _ => (None, PaceIndicator::OnTrack, Decimal::ZERO),
    };

    BudgetStatus {
        category_id: category.id,
        category_name: category.name.to_string(),
        budget_amount,
        spent,
        remaining,
        time_left,
        pace,
        pace_delta,
        budget_mode: BudgetMode::Project,
        seasonal_factor: None,
        trend_monthly: None,
        changepoint_shift_pct: None,
        residual_outlier: false,
    }
}

/// Compute the full budget status for a category.
///
/// Dispatches to mode-specific logic based on the category's budget mode.
#[must_use]
pub fn compute_budget_status(
    category: &Category,
    transactions: &[Transaction],
    current_month: &BudgetMonth,
    all_months: &[BudgetMonth],
    categories: &[Category],
    today: NaiveDate,
    analysis: Option<&crate::anomalies::SpendingAnalysis>,
) -> BudgetStatus {
    match category.budget.mode() {
        Some(BudgetMode::Annual) => compute_annual_status(
            category,
            transactions,
            current_month,
            all_months,
            categories,
            today,
        ),
        Some(BudgetMode::Project) => {
            compute_project_status(category, transactions, categories, today)
        }
        // Monthly is the default for budgeted categories.
        // Salary/Transfer categories are excluded at the API layer but fall back here defensively.
        Some(BudgetMode::Monthly | BudgetMode::Salary | BudgetMode::Transfer) | None => {
            compute_monthly_status(
                category,
                transactions,
                current_month,
                categories,
                today,
                analysis,
            )
        }
    }
}

/// Filter transactions relevant to project budget math.
///
/// Includes only expenses in project-mode category subtrees, excluding
/// transfers, reimbursements, and reimbursed originals.
#[must_use]
pub fn filter_for_project<'a>(
    transactions: &'a [Transaction],
    categories: &[Category],
) -> Vec<&'a Transaction> {
    let project_cats = project_category_ids(categories);
    let reimbursed_ids = reimbursed_transaction_ids(transactions);

    transactions
        .iter()
        .filter(|t| {
            let in_project = t
                .categorization
                .category_id()
                .is_some_and(|cid| project_cats.contains(&cid));
            if !in_project {
                return false;
            }
            !is_correlated_exclusion(t, &reimbursed_ids)
        })
        .collect()
}

/// Compute per-child spending breakdown for a project category.
///
/// For each direct child of the project, sums spending in that child's subtree.
/// Transactions directly on the project root are collected under its own ID
/// with the name "(Direct)".
#[must_use]
pub fn compute_project_child_breakdowns(
    project_category: &Category,
    project_transactions: &[&Transaction],
    categories: &[Category],
) -> Vec<ProjectChildSpending> {
    let direct_children: Vec<&Category> = categories
        .iter()
        .filter(|c| c.parent_id == Some(project_category.id))
        .collect();

    // Build subtree for each direct child
    let child_subtrees: Vec<(CategoryId, &str, std::collections::HashSet<CategoryId>)> =
        direct_children
            .iter()
            .map(|c| {
                let subtree = collect_category_subtree(c.id, categories)
                    .into_iter()
                    .collect();
                (c.id, c.name.as_ref(), subtree)
            })
            .collect();

    let mut child_spent: std::collections::HashMap<CategoryId, Decimal> =
        std::collections::HashMap::new();
    let mut direct_spent = Decimal::ZERO;

    for t in project_transactions {
        let Some(cid) = t.categorization.category_id() else {
            continue;
        };
        let amt = -t.amount; // expenses are negative, flip to positive
        if cid == project_category.id {
            direct_spent += amt;
            continue;
        }
        // Find which direct child subtree this transaction belongs to
        for (child_id, _, subtree) in &child_subtrees {
            if subtree.contains(&cid) {
                *child_spent.entry(*child_id).or_insert(Decimal::ZERO) += amt;
                break;
            }
        }
    }

    let mut rows: Vec<ProjectChildSpending> = child_subtrees
        .iter()
        .filter_map(|(child_id, name, _)| {
            let spent = child_spent.get(child_id).copied().unwrap_or(Decimal::ZERO);
            if spent > Decimal::ZERO {
                Some(ProjectChildSpending {
                    category_id: *child_id,
                    category_name: (*name).to_owned(),
                    spent,
                })
            } else {
                None
            }
        })
        .collect();

    if direct_spent > Decimal::ZERO {
        rows.push(ProjectChildSpending {
            category_id: project_category.id,
            category_name: "(Direct)".to_owned(),
            spent: direct_spent,
        });
    }

    rows.sort_by(|a, b| b.spent.cmp(&a.spent));
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Categorization, CategoryId, CategoryName, Correlation};
    use chrono::NaiveDate;
    use rust_decimal_macros::dec;

    fn nz(n: u32) -> NonZeroU32 {
        NonZeroU32::new(n).expect("non-zero test value")
    }

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).expect("valid test date")
    }

    fn make_txn(
        categorization: Categorization,
        amount: Decimal,
        posted_date: NaiveDate,
    ) -> Transaction {
        Transaction {
            categorization,
            amount,
            merchant_name: "Test".to_owned(),
            remittance_information: vec!["Test transaction".to_owned()],
            posted_date,
            ..Default::default()
        }
    }

    fn make_category(id: u128, name: &str, parent_id: Option<u128>) -> Category {
        Category {
            id: CategoryId::from_uuid(uuid::Uuid::from_u128(id)),
            name: CategoryName::new(name).expect("valid test category name"),
            parent_id: parent_id.map(|p| CategoryId::from_uuid(uuid::Uuid::from_u128(p))),
            budget: BudgetConfig::None,
        }
    }

    fn salary_category() -> Category {
        Category {
            budget: BudgetConfig::Salary,
            ..make_category(1, "Salary", None)
        }
    }

    fn salary_cat_id() -> CategoryId {
        CategoryId::from_uuid(uuid::Uuid::from_u128(1))
    }

    fn food_category() -> Category {
        make_category(100, "Food", None)
    }

    fn groceries_category() -> Category {
        make_category(101, "Groceries", Some(100))
    }

    fn restaurants_category() -> Category {
        make_category(102, "Restaurants", Some(100))
    }

    fn food_with_budget(mode: BudgetMode, amount: Decimal) -> Category {
        let budget = match mode {
            BudgetMode::Monthly => BudgetConfig::Monthly {
                amount,
                budget_type: BudgetType::Variable,
            },
            BudgetMode::Annual => BudgetConfig::Annual {
                amount,
                budget_type: BudgetType::Variable,
            },
            BudgetMode::Project => BudgetConfig::Project {
                amount,
                start_date: date(2025, 1, 1),
                end_date: None,
            },
            BudgetMode::Salary => BudgetConfig::Salary,
            BudgetMode::Transfer => BudgetConfig::Transfer,
        };
        Category {
            budget,
            ..food_category()
        }
    }

    #[test]
    fn detect_single_salary_budget_months() {
        let categories = vec![salary_category()];
        let transactions = vec![
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(3000),
                date(2025, 1, 15),
            ),
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(3000),
                date(2025, 2, 14),
            ),
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(3000),
                date(2025, 3, 15),
            ),
        ];

        let months = detect_budget_month_boundaries(&transactions, nz(1), &categories)
            .expect("should detect months");

        assert_eq!(months.len(), 3);
        assert_eq!(months[0].start_date, date(2025, 1, 15));
        assert_eq!(months[1].start_date, date(2025, 2, 14));
        assert_eq!(months[2].start_date, date(2025, 3, 15));

        // First two months should have end dates
        assert_eq!(months[0].end_date, Some(date(2025, 2, 13)));
        assert_eq!(months[1].end_date, Some(date(2025, 3, 14)));
        // Last month is still open
        assert_eq!(months[2].end_date, None);
    }

    #[test]
    fn detect_two_salary_budget_months() {
        let categories = vec![salary_category()];
        let transactions = vec![
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(2000),
                date(2025, 1, 10),
            ),
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(2000),
                date(2025, 1, 25),
            ),
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(2000),
                date(2025, 2, 10),
            ),
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(2000),
                date(2025, 2, 24),
            ),
        ];

        let months = detect_budget_month_boundaries(&transactions, nz(2), &categories)
            .expect("should detect months");

        assert_eq!(months.len(), 2);
        // Budget month starts on first salary of the calendar month
        assert_eq!(months[0].start_date, date(2025, 1, 10));
        assert_eq!(months[1].start_date, date(2025, 2, 10));
    }

    #[test]
    fn incomplete_salary_month_skipped() {
        let categories = vec![salary_category()];
        // Only 1 salary in February when 2 expected
        let transactions = vec![
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(2000),
                date(2025, 1, 10),
            ),
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(2000),
                date(2025, 1, 25),
            ),
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(2000),
                date(2025, 2, 10),
            ),
            // Missing second salary in Feb
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(2000),
                date(2025, 3, 10),
            ),
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(2000),
                date(2025, 3, 25),
            ),
        ];

        let months = detect_budget_month_boundaries(&transactions, nz(2), &categories)
            .expect("should detect months");

        assert_eq!(months.len(), 2);
        assert_eq!(months[0].start_date, date(2025, 1, 10));
        assert_eq!(months[1].start_date, date(2025, 3, 10));
    }

    #[test]
    fn detect_months_with_mixed_categorized_and_uncategorized() {
        // Simulates the real scenario: salary transactions across 3 calendar
        // months with additional non-salary transactions that should be ignored.
        let categories = vec![salary_category(), food_category()];
        let transactions = vec![
            // Dec salary
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(10376.32),
                date(2025, 12, 18),
            ),
            // Jan salary
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(9330.13),
                date(2026, 1, 26),
            ),
            // Feb salary
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(14000.19),
                date(2026, 2, 26),
            ),
            // Non-salary positive transactions should be ignored
            make_txn(
                Categorization::Manual(food_category().id),
                dec!(50),
                date(2026, 2, 23),
            ),
            // Negative salary-category transactions (transfers out) should be ignored
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(-1000),
                date(2026, 1, 2),
            ),
        ];

        let months = detect_budget_month_boundaries(&transactions, nz(1), &categories)
            .expect("should detect 3 months");

        assert_eq!(months.len(), 3);
        assert_eq!(months[0].start_date, date(2025, 12, 18));
        assert_eq!(months[1].start_date, date(2026, 1, 26));
        assert_eq!(months[2].start_date, date(2026, 2, 26));

        // First two closed, last open
        assert_eq!(months[0].end_date, Some(date(2026, 1, 25)));
        assert_eq!(months[1].end_date, Some(date(2026, 2, 25)));
        assert_eq!(months[2].end_date, None);
    }

    #[test]
    fn no_salary_category_returns_error() {
        let result = detect_budget_month_boundaries(&[], nz(1), &[]);
        assert!(result.is_err());
    }

    #[test]
    fn month_starts_on_first_salary_all_deposits_in_range() {
        // With 3 salary deposits per month, the budget month should start
        // on the earliest one so all deposits fall within the month range.
        let sal = salary_category();
        let child_sal = Category {
            id: CategoryId::from_uuid(uuid::Uuid::from_u128(201)),
            name: CategoryName::new("Kindergeld").unwrap(),
            parent_id: None,
            budget: BudgetConfig::Salary,
        };
        let categories = vec![sal, child_sal];
        let sal_id = salary_cat_id();
        let child_id = CategoryId::from_uuid(uuid::Uuid::from_u128(201));

        let transactions = vec![
            // Feb: Kindergeld on 6th, Facebook on 26th, LBV on 27th
            make_txn(
                Categorization::Manual(child_id),
                dec!(518),
                date(2026, 2, 6),
            ),
            make_txn(
                Categorization::Manual(sal_id),
                dec!(14000),
                date(2026, 2, 26),
            ),
            make_txn(
                Categorization::Manual(sal_id),
                dec!(1721),
                date(2026, 2, 27),
            ),
            // Jan: Kindergeld on 8th, Facebook on 26th, LBV on 30th
            make_txn(
                Categorization::Manual(child_id),
                dec!(518),
                date(2026, 1, 8),
            ),
            make_txn(
                Categorization::Manual(sal_id),
                dec!(9330),
                date(2026, 1, 26),
            ),
            make_txn(
                Categorization::Manual(sal_id),
                dec!(1721),
                date(2026, 1, 30),
            ),
        ];

        let months = detect_budget_month_boundaries(&transactions, nz(3), &categories)
            .expect("should detect months");

        assert_eq!(months.len(), 2);
        assert_eq!(months[0].start_date, date(2026, 1, 8));
        assert_eq!(months[1].start_date, date(2026, 2, 6));

        // All Jan salary deposits fall within month 0 (01-08 to 02-05)
        assert_eq!(months[0].end_date, Some(date(2026, 2, 5)));
        assert!(date(2026, 1, 8) >= months[0].start_date);
        assert!(date(2026, 1, 26) <= months[0].end_date.unwrap());
        assert!(date(2026, 1, 30) <= months[0].end_date.unwrap());

        // All Feb salary deposits fall within month 1 (02-06 to open)
        assert_eq!(months[1].end_date, None);
        assert!(date(2026, 2, 6) >= months[1].start_date);
        assert!(date(2026, 2, 26) >= months[1].start_date);
        assert!(date(2026, 2, 27) >= months[1].start_date);
    }

    #[test]
    fn end_date_is_day_before_next_first_salary() {
        let categories = vec![salary_category()];
        let transactions = vec![
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(1000),
                date(2025, 3, 5),
            ),
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(2000),
                date(2025, 3, 20),
            ),
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(1000),
                date(2025, 4, 4),
            ),
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(2000),
                date(2025, 4, 19),
            ),
        ];

        let months = detect_budget_month_boundaries(&transactions, nz(2), &categories)
            .expect("should detect months");

        assert_eq!(months[0].start_date, date(2025, 3, 5));
        assert_eq!(months[0].end_date, Some(date(2025, 4, 3)));
        assert_eq!(months[1].start_date, date(2025, 4, 4));
    }

    #[test]
    fn negative_salary_transactions_ignored_for_boundary_detection() {
        let categories = vec![salary_category()];
        let transactions = vec![
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(3000),
                date(2025, 1, 15),
            ),
            // Negative amount in salary category (e.g. correction)
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(-500),
                date(2025, 1, 10),
            ),
        ];

        // expected_salary_count = 1, so only the positive one counts
        let months = detect_budget_month_boundaries(&transactions, nz(1), &categories)
            .expect("should detect months");

        assert_eq!(months.len(), 1);
        assert_eq!(months[0].start_date, date(2025, 1, 15));
    }

    #[test]
    fn three_salaries_same_day() {
        let sal = salary_category();
        let child_sal = Category {
            id: CategoryId::from_uuid(uuid::Uuid::from_u128(201)),
            name: CategoryName::new("Bonus").unwrap(),
            parent_id: None,
            budget: BudgetConfig::Salary,
        };
        let categories = vec![sal, child_sal];
        let sal_id = salary_cat_id();
        let child_id = CategoryId::from_uuid(uuid::Uuid::from_u128(201));

        let transactions = vec![
            make_txn(Categorization::Manual(sal_id), dec!(3000), date(2025, 6, 1)),
            make_txn(
                Categorization::Manual(child_id),
                dec!(500),
                date(2025, 6, 1),
            ),
            make_txn(Categorization::Manual(sal_id), dec!(1000), date(2025, 6, 1)),
        ];

        let months = detect_budget_month_boundaries(&transactions, nz(3), &categories)
            .expect("should detect months");

        assert_eq!(months.len(), 1);
        assert_eq!(months[0].start_date, date(2025, 6, 1));
    }

    #[test]
    fn category_spending_with_hierarchy() {
        let food = food_category();
        let groceries = groceries_category();
        let restaurants = restaurants_category();
        let categories = vec![food.clone(), groceries.clone(), restaurants.clone()];

        let bm = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 1, 15),
            end_date: Some(date(2025, 2, 13)),
            salary_transactions_detected: 1,
        };

        let transactions = vec![
            make_txn(
                Categorization::Manual(groceries.id),
                dec!(-50),
                date(2025, 1, 20),
            ),
            make_txn(
                Categorization::Manual(restaurants.id),
                dec!(-30),
                date(2025, 1, 22),
            ),
            make_txn(
                Categorization::Manual(food.id),
                dec!(-10),
                date(2025, 1, 25),
            ),
            // Outside budget month — should be excluded
            make_txn(
                Categorization::Manual(groceries.id),
                dec!(-100),
                date(2025, 2, 14),
            ),
        ];

        // Spending on Food (parent) includes all children
        let food_spending = compute_category_spending(&transactions, food.id, &bm, &categories);
        assert_eq!(food_spending, dec!(90)); // 50 + 30 + 10

        // Spending on Groceries only
        let grocery_spending =
            compute_category_spending(&transactions, groceries.id, &bm, &categories);
        assert_eq!(grocery_spending, dec!(50));
    }

    #[test]
    fn annual_subcategory_excluded_from_monthly_parent_spending() {
        let mut food = food_category();
        food.budget = BudgetConfig::Monthly {
            amount: dec!(500),
            budget_type: BudgetType::Variable,
        };

        let groceries = groceries_category(); // no explicit mode → inherits monthly
        let mut christmas_food = make_category(103, "Christmas Food", Some(100));
        christmas_food.budget = BudgetConfig::Annual {
            amount: dec!(200),
            budget_type: BudgetType::Variable,
        };

        let categories = vec![food.clone(), groceries.clone(), christmas_food.clone()];

        let bm = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 1, 15),
            end_date: Some(date(2025, 2, 13)),
            salary_transactions_detected: 1,
        };

        let transactions = vec![
            make_txn(
                Categorization::Manual(groceries.id),
                dec!(-50),
                date(2025, 1, 20),
            ),
            make_txn(
                Categorization::Manual(christmas_food.id),
                dec!(-80),
                date(2025, 1, 25),
            ),
        ];

        // Monthly parent should NOT include the annual subcategory's spending
        let food_spending = compute_category_spending(&transactions, food.id, &bm, &categories);
        assert_eq!(food_spending, dec!(50));

        // Annual subcategory tracks its own spending independently
        let xmas_spending =
            compute_category_spending(&transactions, christmas_food.id, &bm, &categories);
        assert_eq!(xmas_spending, dec!(80));
    }

    #[test]
    fn project_transactions_excluded_from_budget() {
        let food = food_category();
        // A project category whose transactions should be excluded
        let mut project_cat = make_category(200, "Renovation", None);
        project_cat.budget = BudgetConfig::Project {
            amount: Decimal::ZERO,
            start_date: date(2025, 1, 1),
            end_date: None,
        };
        let categories = vec![food.clone(), project_cat.clone()];

        let bm = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 1, 15),
            end_date: Some(date(2025, 2, 13)),
            salary_transactions_detected: 1,
        };

        let transactions = vec![
            make_txn(
                Categorization::Manual(food.id),
                dec!(-50),
                date(2025, 1, 20),
            ),
            // This transaction is in a project category — excluded from budget
            make_txn(
                Categorization::Manual(project_cat.id),
                dec!(-500),
                date(2025, 1, 20),
            ),
        ];

        let spending = compute_category_spending(&transactions, food.id, &bm, &categories);
        assert_eq!(spending, dec!(50));

        // The project category spending is also excluded when computing "all"
        // spending via a parent that doesn't exist, so let's verify via filter
        let filtered = filter_for_budget(&transactions, &categories);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].amount, dec!(-50));
    }

    #[test]
    fn salary_transactions_excluded_from_budget() {
        let food = food_category();
        let salary = salary_category();
        let categories = vec![food.clone(), salary.clone()];

        let transactions = vec![
            make_txn(
                Categorization::Manual(food.id),
                dec!(-50),
                date(2025, 1, 20),
            ),
            make_txn(
                Categorization::Manual(salary.id),
                dec!(3000),
                date(2025, 1, 15),
            ),
        ];

        let filtered = filter_for_budget(&transactions, &categories);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].amount, dec!(-50));
    }

    #[test]
    fn transfer_category_transactions_excluded_from_budget() {
        let food = food_category();
        let mut transfer = make_category(200, "Investments", None);
        transfer.budget = BudgetConfig::Transfer;
        let categories = vec![food.clone(), transfer.clone()];

        let transactions = vec![
            make_txn(
                Categorization::Manual(food.id),
                dec!(-50),
                date(2025, 1, 20),
            ),
            make_txn(
                Categorization::Manual(transfer.id),
                dec!(-2000),
                date(2025, 1, 18),
            ),
        ];

        let filtered = filter_for_budget(&transactions, &categories);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].amount, dec!(-50));
    }

    #[test]
    fn transfer_transactions_excluded() {
        let food = food_category();
        let categories = vec![food.clone()];

        let bm = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 1, 15),
            end_date: Some(date(2025, 2, 13)),
            salary_transactions_detected: 1,
        };

        let mut transfer_txn = make_txn(
            Categorization::Manual(food.id),
            dec!(-200),
            date(2025, 1, 20),
        );
        transfer_txn.correlation = Some(crate::models::Correlation {
            partner_id: TransactionId::new(),
            correlation_type: CorrelationType::Transfer,
        });

        let transactions = vec![
            make_txn(
                Categorization::Manual(food.id),
                dec!(-50),
                date(2025, 1, 20),
            ),
            transfer_txn,
        ];

        let spending = compute_category_spending(&transactions, food.id, &bm, &categories);
        assert_eq!(spending, dec!(50));
    }

    #[test]
    fn reimbursed_transactions_excluded() {
        let food = food_category();
        let categories = vec![food.clone()];

        let bm = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 1, 15),
            end_date: Some(date(2025, 2, 13)),
            salary_transactions_detected: 1,
        };

        // Original expense that gets reimbursed
        let original_txn = make_txn(
            Categorization::Manual(food.id),
            dec!(-200),
            date(2025, 1, 20),
        );
        let original_id = original_txn.id;

        // Reimbursement linked to the original (positive: money coming back)
        let mut reimbursement = make_txn(
            Categorization::Manual(food.id),
            dec!(200),
            date(2025, 1, 25),
        );
        reimbursement.correlation = Some(crate::models::Correlation {
            partner_id: original_id,
            correlation_type: CorrelationType::Reimbursement,
        });

        let transactions = vec![
            make_txn(
                Categorization::Manual(food.id),
                dec!(-50),
                date(2025, 1, 20),
            ),
            original_txn,
            reimbursement,
        ];

        let spending = compute_category_spending(&transactions, food.id, &bm, &categories);
        // -200 (reimbursed original) and +200 (reimbursement) both excluded
        assert_eq!(spending, dec!(50));
    }

    #[test]
    fn budget_status_under_budget() {
        let food = food_with_budget(BudgetMode::Monthly, dec!(500));
        let categories = vec![food.clone()];

        let bm = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 1, 15),
            end_date: Some(date(2025, 2, 13)),
            salary_transactions_detected: 1,
        };
        let all_months = [bm.clone()];

        let transactions = vec![make_txn(
            Categorization::Manual(food.id),
            dec!(-100),
            date(2025, 1, 20),
        )];

        let today = date(2025, 1, 25);
        let status = compute_budget_status(
            &food,
            &transactions,
            &bm,
            &all_months,
            &categories,
            today,
            None,
        );

        assert_eq!(status.spent, dec!(100));
        assert_eq!(status.remaining, dec!(400));
        assert_eq!(status.budget_amount, dec!(500));
        assert_eq!(status.pace, PaceIndicator::UnderBudget);
        assert!(
            status.pace_delta < Decimal::ZERO,
            "under-pace delta should be negative"
        );
        assert!(status.time_left.unwrap_or(0) > 0);
        assert_eq!(status.budget_mode, BudgetMode::Monthly);
    }

    #[test]
    fn budget_status_over_budget() {
        let food = food_with_budget(BudgetMode::Monthly, dec!(200));
        let categories = vec![food.clone()];

        let bm = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 1, 15),
            end_date: Some(date(2025, 2, 13)),
            salary_transactions_detected: 1,
        };
        let all_months = [bm.clone()];

        let transactions = vec![make_txn(
            Categorization::Manual(food.id),
            dec!(-250),
            date(2025, 1, 20),
        )];

        let today = date(2025, 1, 25);
        let status = compute_budget_status(
            &food,
            &transactions,
            &bm,
            &all_months,
            &categories,
            today,
            None,
        );

        assert_eq!(status.spent, dec!(250));
        assert_eq!(status.remaining, dec!(-50));
        assert_eq!(status.pace, PaceIndicator::OverBudget);
        assert!(
            status.pace_delta > Decimal::ZERO,
            "over-pace delta should be positive"
        );
        assert_eq!(status.budget_mode, BudgetMode::Monthly);
    }

    #[test]
    fn budget_year_months_finds_january_anchor() {
        let bm_jan = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 1, 15),
            end_date: Some(date(2025, 2, 13)),
            salary_transactions_detected: 1,
        };
        let bm_feb = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 2, 14),
            end_date: Some(date(2025, 3, 14)),
            salary_transactions_detected: 1,
        };
        let bm_mar = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 3, 15),
            end_date: None,
            salary_transactions_detected: 1,
        };
        let all = [bm_jan.clone(), bm_feb.clone(), bm_mar.clone()];

        let year = budget_year_months(&all, &bm_mar);
        assert_eq!(year.len(), 3);
        assert_eq!(year[0].id, bm_jan.id);
    }

    #[test]
    fn budget_year_months_caps_at_twelve() {
        // 14 contiguous budget months starting from Nov 2024, mid-month salary
        let months: Vec<BudgetMonth> = (0..14_u32)
            .map(|i| {
                let y = i32::try_from(2024 + (10 + i) / 12).expect("year fits i32");
                let m = ((10 + i) % 12) + 1;
                let next_y = i32::try_from(2024 + (11 + i) / 12).expect("year fits i32");
                let next_m = ((11 + i) % 12) + 1;
                BudgetMonth {
                    id: BudgetMonthId::new(),
                    start_date: date(y, m, 15),
                    end_date: Some(date(next_y, next_m, 14)),
                    salary_transactions_detected: 1,
                }
            })
            .collect();

        // Reference = month index 5 (March 2025)
        // Index 1 = Dec 15 2024 → Jan 14 2025 contains Jan 1 2025
        let year = budget_year_months(&months, &months[5]);
        assert_eq!(year[0].start_date, date(2024, 12, 15));
        assert!(year.len() <= 12);
    }

    #[test]
    fn annual_status_aggregates_across_year() {
        let food = food_with_budget(BudgetMode::Annual, dec!(6000));
        let categories = vec![food.clone()];

        let bm_jan = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 1, 15),
            end_date: Some(date(2025, 2, 13)),
            salary_transactions_detected: 1,
        };
        let bm_feb = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 2, 14),
            end_date: Some(date(2025, 3, 14)),
            salary_transactions_detected: 1,
        };
        let bm_mar = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 3, 15),
            end_date: None,
            salary_transactions_detected: 1,
        };
        let all_months = [bm_jan.clone(), bm_feb.clone(), bm_mar.clone()];

        let transactions = vec![
            make_txn(
                Categorization::Manual(food.id),
                dec!(-400),
                date(2025, 1, 20),
            ),
            make_txn(
                Categorization::Manual(food.id),
                dec!(-600),
                date(2025, 2, 20),
            ),
            make_txn(
                Categorization::Manual(food.id),
                dec!(-200),
                date(2025, 3, 20),
            ),
        ];

        let today = date(2025, 3, 25);
        let status = compute_budget_status(
            &food,
            &transactions,
            &bm_mar,
            &all_months,
            &categories,
            today,
            None,
        );

        assert_eq!(status.budget_mode, BudgetMode::Annual);
        // 400 + 600 + 200 = 1200 across three months
        assert_eq!(status.spent, dec!(1200));
        assert_eq!(status.remaining, dec!(4800));
        // 12-month year, reference is month 3 → 9 months left
        assert_eq!(status.time_left, Some(9));
    }

    #[test]
    fn project_status_filters_by_date_range() {
        let mut project = make_category(300, "Renovation", None);
        project.budget = BudgetConfig::Project {
            amount: dec!(10000),
            start_date: date(2025, 1, 1),
            end_date: Some(date(2025, 6, 30)),
        };
        let categories = vec![project.clone()];

        // A dummy budget month (needed for signature but project ignores it)
        let bm = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 3, 15),
            end_date: None,
            salary_transactions_detected: 1,
        };
        let all_months = [bm.clone()];

        let transactions = vec![
            make_txn(
                Categorization::Manual(project.id),
                dec!(-2000),
                date(2025, 2, 15),
            ),
            make_txn(
                Categorization::Manual(project.id),
                dec!(-3000),
                date(2025, 4, 10),
            ),
            // Outside project range — excluded
            make_txn(
                Categorization::Manual(project.id),
                dec!(-500),
                date(2024, 12, 20),
            ),
            make_txn(
                Categorization::Manual(project.id),
                dec!(-500),
                date(2025, 7, 1),
            ),
        ];

        let today = date(2025, 4, 15);
        let status = compute_budget_status(
            &project,
            &transactions,
            &bm,
            &all_months,
            &categories,
            today,
            None,
        );

        assert_eq!(status.budget_mode, BudgetMode::Project);
        assert_eq!(status.spent, dec!(5000));
        assert_eq!(status.remaining, dec!(5000));
        // Days left: June 30 - April 15 = 76 days
        assert_eq!(status.time_left, Some(76));
    }

    #[test]
    fn project_status_open_ended() {
        let mut project = make_category(301, "Ongoing", None);
        project.budget = BudgetConfig::Project {
            amount: dec!(5000),
            start_date: date(2025, 1, 1),
            end_date: None,
        };
        let categories = vec![project.clone()];

        let bm = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 3, 15),
            end_date: None,
            salary_transactions_detected: 1,
        };
        let all_months = [bm.clone()];

        let transactions = vec![make_txn(
            Categorization::Manual(project.id),
            dec!(-1000),
            date(2025, 2, 1),
        )];

        let today = date(2025, 4, 1);
        let status = compute_budget_status(
            &project,
            &transactions,
            &bm,
            &all_months,
            &categories,
            today,
            None,
        );

        assert_eq!(status.budget_mode, BudgetMode::Project);
        assert_eq!(status.spent, dec!(1000));
        assert_eq!(status.time_left, None);
        assert_eq!(status.pace, PaceIndicator::OnTrack);
        assert_eq!(status.pace_delta, Decimal::ZERO);
    }

    #[test]
    fn spending_nets_refund_against_expenses() {
        let food = food_category();
        let categories = vec![food.clone()];

        let bm = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 1, 15),
            end_date: Some(date(2025, 2, 13)),
            salary_transactions_detected: 1,
        };

        let transactions = vec![
            make_txn(
                Categorization::Manual(food.id),
                dec!(-80),
                date(2025, 1, 20),
            ),
            make_txn(
                Categorization::Manual(food.id),
                dec!(-45),
                date(2025, 1, 22),
            ),
            // Refund credited back (positive amount from the bank)
            make_txn(Categorization::Manual(food.id), dec!(25), date(2025, 1, 24)),
        ];

        let spending = compute_category_spending(&transactions, food.id, &bm, &categories);
        // Net outflow: -80 + -45 + 25 = -100, negated → 100
        assert_eq!(spending, dec!(100));
    }

    #[test]
    fn mixed_monthly_and_annual_categories() {
        let food = food_with_budget(BudgetMode::Monthly, dec!(500));
        let mut insurance = make_category(200, "Insurance", None);
        insurance.budget = BudgetConfig::Annual {
            amount: dec!(2400),
            budget_type: BudgetType::Variable,
        };
        let categories = vec![food.clone(), insurance.clone()];

        let bm_jan = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 1, 15),
            end_date: Some(date(2025, 2, 13)),
            salary_transactions_detected: 1,
        };
        let bm_feb = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 2, 14),
            end_date: None,
            salary_transactions_detected: 1,
        };
        let all_months = [bm_jan.clone(), bm_feb.clone()];

        let transactions = vec![
            // Food expenses across two months
            make_txn(
                Categorization::Manual(food.id),
                dec!(-300),
                date(2025, 1, 20),
            ),
            make_txn(
                Categorization::Manual(food.id),
                dec!(-200),
                date(2025, 2, 20),
            ),
            // Insurance in both months
            make_txn(
                Categorization::Manual(insurance.id),
                dec!(-100),
                date(2025, 1, 25),
            ),
            make_txn(
                Categorization::Manual(insurance.id),
                dec!(-150),
                date(2025, 2, 18),
            ),
        ];

        let today = date(2025, 2, 25);

        // Monthly food status for current month (Feb)
        let food_status = compute_budget_status(
            &food,
            &transactions,
            &bm_feb,
            &all_months,
            &categories,
            today,
            None,
        );
        assert_eq!(food_status.budget_mode, BudgetMode::Monthly);
        assert_eq!(food_status.spent, dec!(200));
        // remaining = 500 - 200 = 300
        assert_eq!(food_status.remaining, dec!(300));

        // Annual insurance status (sums all months in the year)
        let ins_status = compute_budget_status(
            &insurance,
            &transactions,
            &bm_feb,
            &all_months,
            &categories,
            today,
            None,
        );
        assert_eq!(ins_status.budget_mode, BudgetMode::Annual);
        // 100 + 150 = 250 across two months
        assert_eq!(ins_status.spent, dec!(250));
        assert_eq!(ins_status.remaining, dec!(2150));
    }

    #[test]
    fn annual_transactions_do_not_affect_monthly_budget() {
        let food = food_with_budget(BudgetMode::Monthly, dec!(500));
        let mut insurance = make_category(200, "Insurance", None);
        insurance.budget = BudgetConfig::Annual {
            amount: dec!(2400),
            budget_type: BudgetType::Variable,
        };
        let categories = vec![food.clone(), insurance.clone()];

        let bm = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 1, 15),
            end_date: Some(date(2025, 2, 13)),
            salary_transactions_detected: 1,
        };
        let all_months = [bm.clone()];

        let transactions = vec![
            make_txn(
                Categorization::Manual(food.id),
                dec!(-120),
                date(2025, 1, 20),
            ),
            // Large annual insurance payment in the same month
            make_txn(
                Categorization::Manual(insurance.id),
                dec!(-1200),
                date(2025, 1, 18),
            ),
        ];

        let today = date(2025, 1, 25);

        // Monthly food spending must only reflect food transactions
        let food_spending = compute_category_spending(&transactions, food.id, &bm, &categories);
        assert_eq!(food_spending, dec!(120));

        let food_status = compute_budget_status(
            &food,
            &transactions,
            &bm,
            &all_months,
            &categories,
            today,
            None,
        );
        assert_eq!(food_status.spent, dec!(120));
        assert_eq!(food_status.remaining, dec!(380));

        // Annual insurance must not include food
        let ins_spending = compute_category_spending(&transactions, insurance.id, &bm, &categories);
        assert_eq!(ins_spending, dec!(1200));

        let ins_status = compute_budget_status(
            &insurance,
            &transactions,
            &bm,
            &all_months,
            &categories,
            today,
            None,
        );
        assert_eq!(ins_status.spent, dec!(1200));
        assert_eq!(ins_status.remaining, dec!(1200));
    }

    #[test]
    fn dashboard_totals_with_monthly_and_annual() {
        let food = food_with_budget(BudgetMode::Monthly, dec!(500));
        let mut transport = make_category(150, "Transport", None);
        transport.budget = BudgetConfig::Monthly {
            amount: dec!(200),
            budget_type: BudgetType::Variable,
        };
        let mut insurance = make_category(200, "Insurance", None);
        insurance.budget = BudgetConfig::Annual {
            amount: dec!(2400),
            budget_type: BudgetType::Variable,
        };
        let categories = vec![food.clone(), transport.clone(), insurance.clone()];

        let bm_jan = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 1, 15),
            end_date: Some(date(2025, 2, 13)),
            salary_transactions_detected: 1,
        };
        let bm_feb = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 2, 14),
            end_date: None,
            salary_transactions_detected: 1,
        };
        let all_months = [bm_jan.clone(), bm_feb.clone()];

        let transactions = vec![
            // Jan food
            make_txn(
                Categorization::Manual(food.id),
                dec!(-350),
                date(2025, 1, 20),
            ),
            // Jan transport
            make_txn(
                Categorization::Manual(transport.id),
                dec!(-180),
                date(2025, 1, 22),
            ),
            // Jan insurance (annual)
            make_txn(
                Categorization::Manual(insurance.id),
                dec!(-200),
                date(2025, 1, 25),
            ),
            // Feb food
            make_txn(
                Categorization::Manual(food.id),
                dec!(-420),
                date(2025, 2, 18),
            ),
            // Feb transport
            make_txn(
                Categorization::Manual(transport.id),
                dec!(-90),
                date(2025, 2, 20),
            ),
            // Feb insurance (annual)
            make_txn(
                Categorization::Manual(insurance.id),
                dec!(-200),
                date(2025, 2, 16),
            ),
        ];

        let today = date(2025, 2, 25);
        let budgeted_categories = [&food, &transport, &insurance];

        let statuses: Vec<BudgetStatus> = budgeted_categories
            .iter()
            .map(|cat| {
                compute_budget_status(
                    cat,
                    &transactions,
                    &bm_feb,
                    &all_months,
                    &categories,
                    today,
                    None,
                )
            })
            .collect();

        // Verify individual statuses
        let food_s = &statuses[0];
        assert_eq!(food_s.budget_mode, BudgetMode::Monthly);
        assert_eq!(food_s.budget_amount, dec!(500));
        assert_eq!(food_s.spent, dec!(420));
        assert_eq!(food_s.remaining, dec!(80)); // 500 - 420

        let transport_s = &statuses[1];
        assert_eq!(transport_s.budget_mode, BudgetMode::Monthly);
        assert_eq!(transport_s.budget_amount, dec!(200));
        assert_eq!(transport_s.spent, dec!(90));
        assert_eq!(transport_s.remaining, dec!(110)); // 200 - 90

        let ins_s = &statuses[2];
        assert_eq!(ins_s.budget_mode, BudgetMode::Annual);
        assert_eq!(ins_s.budget_amount, dec!(2400));
        // Annual sums across all months: 200 + 200 = 400
        assert_eq!(ins_s.spent, dec!(400));
        assert_eq!(ins_s.remaining, dec!(2000));

        // Dashboard totals (mirrors frontend logic: sum budget_amount, sum spent)
        let total_budget: Decimal = statuses.iter().map(|s| s.budget_amount).sum();
        let total_spent: Decimal = statuses.iter().map(|s| s.spent).sum();
        let total_remaining = total_budget - total_spent;

        // 500 (monthly) + 200 (monthly) + 2400 (annual) = 3100
        assert_eq!(total_budget, dec!(3100));
        // 420 + 90 + 400 = 910
        assert_eq!(total_spent, dec!(910));
        assert_eq!(total_remaining, dec!(2190));
    }

    #[test]
    fn zero_spending_yields_full_remaining() {
        let food = food_with_budget(BudgetMode::Monthly, dec!(500));
        let categories = vec![food.clone()];

        let bm = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 1, 15),
            end_date: Some(date(2025, 2, 13)),
            salary_transactions_detected: 1,
        };
        let all_months = [bm.clone()];

        let transactions: Vec<Transaction> = vec![];

        let today = date(2025, 1, 25);
        let status = compute_budget_status(
            &food,
            &transactions,
            &bm,
            &all_months,
            &categories,
            today,
            None,
        );

        assert_eq!(status.spent, Decimal::ZERO);
        assert_eq!(status.remaining, dec!(500));
        assert_eq!(status.pace, PaceIndicator::UnderBudget);
    }

    #[test]
    fn income_in_expense_category_reduces_spending() {
        let food = food_with_budget(BudgetMode::Monthly, dec!(500));
        let categories = vec![food.clone()];

        let bm = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 1, 15),
            end_date: Some(date(2025, 2, 13)),
            salary_transactions_detected: 1,
        };
        let all_months = [bm.clone()];

        // Net positive inflow in an expense category (e.g. large refund)
        let transactions = vec![
            make_txn(
                Categorization::Manual(food.id),
                dec!(-100),
                date(2025, 1, 20),
            ),
            make_txn(
                Categorization::Manual(food.id),
                dec!(300),
                date(2025, 1, 22),
            ),
        ];

        let today = date(2025, 1, 25);
        let status = compute_budget_status(
            &food,
            &transactions,
            &bm,
            &all_months,
            &categories,
            today,
            None,
        );

        // Net: -100 + 300 = +200, negated → -200 (negative spending = net income)
        assert_eq!(status.spent, dec!(-200));
        // remaining = 500 - (-200) = 700
        assert_eq!(status.remaining, dec!(700));
    }

    #[test]
    fn project_with_mixed_expenses_and_refunds() {
        let mut project = make_category(300, "Renovation", None);
        project.budget = BudgetConfig::Project {
            amount: dec!(10000),
            start_date: date(2025, 1, 1),
            end_date: Some(date(2025, 6, 30)),
        };
        let categories = vec![project.clone()];

        let bm = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 3, 15),
            end_date: None,
            salary_transactions_detected: 1,
        };
        let all_months = [bm.clone()];

        let transactions = vec![
            make_txn(
                Categorization::Manual(project.id),
                dec!(-5000),
                date(2025, 2, 15),
            ),
            make_txn(
                Categorization::Manual(project.id),
                dec!(-3000),
                date(2025, 4, 10),
            ),
            // Partial refund on materials
            make_txn(
                Categorization::Manual(project.id),
                dec!(800),
                date(2025, 4, 20),
            ),
        ];

        let today = date(2025, 4, 25);
        let status = compute_budget_status(
            &project,
            &transactions,
            &bm,
            &all_months,
            &categories,
            today,
            None,
        );

        assert_eq!(status.budget_mode, BudgetMode::Project);
        // Net: -5000 + -3000 + 800 = -7200, negated → 7200
        assert_eq!(status.spent, dec!(7200));
        assert_eq!(status.remaining, dec!(2800));
    }

    // ── Variable: pro-rata branch (total > 0 and budget > 0) ───────

    const V: BudgetType = BudgetType::Variable;
    const F: BudgetType = BudgetType::Fixed;

    #[test]
    fn variable_exactly_at_expected_is_on_track() {
        let (pace, delta) = compute_pace(dec!(250), dec!(500), 15, 30, V, None);
        assert_eq!(pace, PaceIndicator::OnTrack);
        assert_eq!(delta, Decimal::ZERO);
    }

    #[test]
    fn variable_within_tolerance_band_is_on_track() {
        // 247.5 is within ±5% of expected 250 (range: 237.5–262.5)
        let (pace, delta) = compute_pace(dec!(247.5), dec!(500), 15, 30, V, None);
        assert_eq!(pace, PaceIndicator::OnTrack);
        assert!(delta < Decimal::ZERO);
    }

    #[test]
    fn variable_above_tolerance_but_within_budget_is_above_pace() {
        // 265 exceeds upper band of 262.5 (= 250 * 1.05) but is under budget 500
        let (pace, delta) = compute_pace(dec!(265), dec!(500), 15, 30, V, None);
        assert_eq!(pace, PaceIndicator::AbovePace);
        assert_eq!(delta, dec!(15));
    }

    #[test]
    fn variable_spent_at_budget_early_is_above_pace() {
        let (pace, delta) = compute_pace(dec!(500), dec!(500), 15, 30, V, None);
        assert_eq!(pace, PaceIndicator::AbovePace);
        assert_eq!(delta, dec!(250));
    }

    #[test]
    fn variable_exceeds_total_budget_is_over_budget() {
        let (pace, delta) = compute_pace(dec!(510), dec!(500), 15, 30, V, None);
        assert_eq!(pace, PaceIndicator::OverBudget);
        assert!(delta > Decimal::ZERO);
    }

    #[test]
    fn variable_well_below_expected_is_under_budget() {
        let (pace, delta) = compute_pace(dec!(100), dec!(500), 15, 30, V, None);
        assert_eq!(pace, PaceIndicator::UnderBudget);
        assert_eq!(delta, dec!(-150));
    }

    #[test]
    fn variable_just_below_lower_band_is_under_budget() {
        // 237.49 is just below 237.5 (= 250 * 0.95)
        let (pace, delta) = compute_pace(dec!(237.49), dec!(500), 15, 30, V, None);
        assert_eq!(pace, PaceIndicator::UnderBudget);
        assert!(delta < Decimal::ZERO);
    }

    #[test]
    fn variable_at_lower_band_boundary_is_on_track() {
        // 237.5 is exactly at the lower 5% band (= 250 * 0.95)
        let (pace, delta) = compute_pace(dec!(237.5), dec!(500), 15, 30, V, None);
        assert_eq!(pace, PaceIndicator::OnTrack);
        assert!(delta < Decimal::ZERO);
    }

    #[test]
    fn variable_at_upper_band_boundary_is_on_track() {
        // 262.5 is exactly at the upper 5% band (= 250 * 1.05)
        let (pace, delta) = compute_pace(dec!(262.5), dec!(500), 15, 30, V, None);
        assert_eq!(pace, PaceIndicator::OnTrack);
        assert!(delta > Decimal::ZERO);
    }

    #[test]
    fn variable_just_above_upper_band_within_budget_is_above_pace() {
        // 262.51 just exceeds 262.5 (= 250 * 1.05)
        let (pace, delta) = compute_pace(dec!(262.51), dec!(500), 15, 30, V, None);
        assert_eq!(pace, PaceIndicator::AbovePace);
        assert!(delta > Decimal::ZERO);
    }

    #[test]
    fn variable_last_day_spent_at_budget_is_on_track() {
        let (pace, delta) = compute_pace(dec!(500), dec!(500), 30, 30, V, None);
        assert_eq!(pace, PaceIndicator::OnTrack);
        assert_eq!(delta, Decimal::ZERO);
    }

    #[test]
    fn variable_last_day_over_budget_is_over_budget() {
        let (pace, delta) = compute_pace(dec!(510), dec!(500), 30, 30, V, None);
        assert_eq!(pace, PaceIndicator::OverBudget);
        assert_eq!(delta, dec!(10));
    }

    #[test]
    fn variable_zero_spent_is_under_budget() {
        let (pace, delta) = compute_pace(Decimal::ZERO, dec!(500), 15, 30, V, None);
        assert_eq!(pace, PaceIndicator::UnderBudget);
        assert_eq!(delta, dec!(-250));
    }

    #[test]
    fn variable_first_day_zero_spent_is_on_track() {
        let (pace, delta) = compute_pace(Decimal::ZERO, dec!(500), 0, 30, V, None);
        assert_eq!(pace, PaceIndicator::OnTrack);
        assert_eq!(delta, Decimal::ZERO);
    }

    #[test]
    fn variable_first_day_any_spend_within_budget_is_above_pace() {
        // On day 0, expected is 0, so any spend exceeds the upper band
        let (pace, delta) = compute_pace(dec!(50), dec!(500), 0, 30, V, None);
        assert_eq!(pace, PaceIndicator::AbovePace);
        assert_eq!(delta, dec!(50));
    }

    // ── Variable: edge cases (total <= 0 or budget == 0) ─────────────

    #[test]
    fn variable_zero_budget_zero_spent_is_on_track() {
        let (pace, delta) = compute_pace(Decimal::ZERO, Decimal::ZERO, 15, 30, V, None);
        assert_eq!(pace, PaceIndicator::OnTrack);
        assert_eq!(delta, Decimal::ZERO);
    }

    #[test]
    fn variable_zero_budget_with_spending_is_over_budget() {
        let (pace, delta) = compute_pace(dec!(100), Decimal::ZERO, 15, 30, V, None);
        assert_eq!(pace, PaceIndicator::OverBudget);
        assert_eq!(delta, dec!(100));
    }

    #[test]
    fn variable_zero_total_spent_equals_budget_is_on_track() {
        let (pace, delta) = compute_pace(dec!(500), dec!(500), 0, 0, V, None);
        assert_eq!(pace, PaceIndicator::OnTrack);
        assert_eq!(delta, Decimal::ZERO);
    }

    #[test]
    fn variable_zero_total_over_budget_is_over_budget() {
        let (pace, delta) = compute_pace(dec!(600), dec!(500), 0, 0, V, None);
        assert_eq!(pace, PaceIndicator::OverBudget);
        assert_eq!(delta, dec!(100));
    }

    #[test]
    fn variable_zero_total_under_budget_is_on_track() {
        let (pace, delta) = compute_pace(dec!(300), dec!(500), 0, 0, V, None);
        assert_eq!(pace, PaceIndicator::OnTrack);
        assert_eq!(delta, dec!(-200));
    }

    // ── Variable with seasonal factor ──────────────────────────────────

    #[test]
    fn seasonal_factor_adjusts_expected_spend() {
        // Without seasonality: expected = 500 * 15/30 = 250, so 300 is above pace
        let (pace_no_season, _) = compute_pace(dec!(300), dec!(500), 15, 30, V, None);
        assert_eq!(pace_no_season, PaceIndicator::AbovePace);

        // With seasonal factor 1.3: expected = 250 * 1.3 = 325
        // lower = 325 * 0.95 = 308.75, spent 300 < lower → under pace
        let (pace_season, delta) = compute_pace(dec!(300), dec!(500), 15, 30, V, Some(1.3));
        assert_eq!(pace_season, PaceIndicator::UnderBudget);
        assert!(delta < Decimal::ZERO);

        // 320 is within [308.75, 341.25] → on track
        let (pace_on_track, _) = compute_pace(dec!(320), dec!(500), 15, 30, V, Some(1.3));
        assert_eq!(pace_on_track, PaceIndicator::OnTrack);
    }

    #[test]
    fn seasonal_factor_below_one_tightens_expectation() {
        // With seasonal factor 0.7: expected = 250 * 0.7 = 175
        // 200 > 175 * 1.05 = 183.75, so above pace
        let (pace, _) = compute_pace(dec!(200), dec!(500), 15, 30, V, Some(0.7));
        assert_eq!(pace, PaceIndicator::AbovePace);
    }

    #[test]
    fn seasonal_factor_none_is_linear() {
        // Verify None gives same result as not having seasonality
        let (pace_none, delta_none) = compute_pace(dec!(250), dec!(500), 15, 30, V, None);
        let (pace_one, delta_one) = compute_pace(dec!(250), dec!(500), 15, 30, V, Some(1.0));
        assert_eq!(pace_none, pace_one);
        assert_eq!(delta_none, delta_one);
    }

    // ── Fixed: payment hasn't arrived, arrived on target, or exceeded ─

    #[test]
    fn fixed_no_spend_is_pending() {
        // Mortgage hasn't been paid yet this month
        let (pace, delta) = compute_pace(Decimal::ZERO, dec!(2000), 2, 30, F, None);
        assert_eq!(pace, PaceIndicator::Pending);
        assert_eq!(delta, dec!(-2000));
    }

    #[test]
    fn fixed_full_payment_is_on_track() {
        // Mortgage paid in full on day 2 — exactly the budget
        let (pace, delta) = compute_pace(dec!(2000), dec!(2000), 2, 30, F, None);
        assert_eq!(pace, PaceIndicator::OnTrack);
        assert_eq!(delta, Decimal::ZERO);
    }

    #[test]
    fn fixed_partial_payment_is_on_track() {
        // Partial payment (e.g. first of two installments)
        let (pace, delta) = compute_pace(dec!(500), dec!(2000), 5, 30, F, None);
        assert_eq!(pace, PaceIndicator::OnTrack);
        assert_eq!(delta, dec!(-1500));
    }

    #[test]
    fn fixed_over_budget_is_over_budget() {
        // Subscription price increased — exceeded the budgeted amount
        let (pace, delta) = compute_pace(dec!(2100), dec!(2000), 2, 30, F, None);
        assert_eq!(pace, PaceIndicator::OverBudget);
        assert_eq!(delta, dec!(100));
    }

    #[test]
    fn fixed_ignores_elapsed_time() {
        // Same result regardless of what day of the month it is
        let (pace_early, _) = compute_pace(dec!(2000), dec!(2000), 1, 30, F, None);
        let (pace_mid, _) = compute_pace(dec!(2000), dec!(2000), 15, 30, F, None);
        let (pace_late, _) = compute_pace(dec!(2000), dec!(2000), 30, 30, F, None);
        assert_eq!(pace_early, PaceIndicator::OnTrack);
        assert_eq!(pace_mid, PaceIndicator::OnTrack);
        assert_eq!(pace_late, PaceIndicator::OnTrack);
    }

    #[test]
    fn fixed_zero_budget_zero_spent_is_pending() {
        let (pace, _) = compute_pace(Decimal::ZERO, Decimal::ZERO, 15, 30, F, None);
        assert_eq!(pace, PaceIndicator::Pending);
    }

    #[test]
    fn fixed_zero_budget_with_spending_is_over_budget() {
        let (pace, delta) = compute_pace(dec!(50), Decimal::ZERO, 15, 30, F, None);
        assert_eq!(pace, PaceIndicator::OverBudget);
        assert_eq!(delta, dec!(50));
    }

    // -----------------------------------------------------------------------
    // collect_budget_subtree
    // -----------------------------------------------------------------------

    #[test]
    fn subtree_children_without_mode_inherit_parent() {
        let parent = {
            let mut c = make_category(10, "Parent", None);
            c.budget = BudgetConfig::Monthly {
                amount: dec!(500),
                budget_type: BudgetType::Variable,
            };
            c
        };
        let child = make_category(11, "Child", Some(10)); // no mode → inherits
        let categories = vec![parent.clone(), child.clone()];

        let subtree = collect_budget_subtree(parent.id, &categories);
        assert!(subtree.contains(&parent.id));
        assert!(subtree.contains(&child.id));
    }

    #[test]
    fn subtree_children_with_different_mode_excluded() {
        let parent = {
            let mut c = make_category(10, "Parent", None);
            c.budget = BudgetConfig::Monthly {
                amount: dec!(500),
                budget_type: BudgetType::Variable,
            };
            c
        };
        let child = {
            let mut c = make_category(11, "Child", Some(10));
            c.budget = BudgetConfig::Annual {
                amount: dec!(1200),
                budget_type: BudgetType::Variable,
            };
            c
        };
        let categories = vec![parent.clone(), child.clone()];

        let subtree = collect_budget_subtree(parent.id, &categories);
        assert!(subtree.contains(&parent.id));
        assert!(!subtree.contains(&child.id));
    }

    #[test]
    fn subtree_prunes_different_mode_child_and_its_descendants() {
        let parent = {
            let mut c = make_category(10, "Parent", None);
            c.budget = BudgetConfig::Monthly {
                amount: dec!(500),
                budget_type: BudgetType::Variable,
            };
            c
        };
        let child = {
            let mut c = make_category(11, "Annual Child", Some(10));
            c.budget = BudgetConfig::Annual {
                amount: dec!(1200),
                budget_type: BudgetType::Variable,
            };
            c
        };
        // Grandchild inherits from annual child, but since child is pruned,
        // grandchild is also unreachable
        let grandchild = make_category(12, "Grandchild", Some(11));
        let categories = vec![parent.clone(), child.clone(), grandchild.clone()];

        let subtree = collect_budget_subtree(parent.id, &categories);
        assert_eq!(subtree.len(), 1);
        assert!(subtree.contains(&parent.id));
    }

    #[test]
    fn subtree_children_with_same_explicit_mode_excluded() {
        // Children with their own explicit budget (even same mode) are
        // excluded — they get their own StatusEntry in the API response.
        let parent = {
            let mut c = make_category(10, "Parent", None);
            c.budget = BudgetConfig::Monthly {
                amount: dec!(500),
                budget_type: BudgetType::Variable,
            };
            c
        };
        let child = {
            let mut c = make_category(11, "Child", Some(10));
            c.budget = BudgetConfig::Monthly {
                amount: dec!(200),
                budget_type: BudgetType::Variable,
            };
            c
        };
        let categories = vec![parent.clone(), child.clone()];

        let subtree = collect_budget_subtree(parent.id, &categories);
        assert!(subtree.contains(&parent.id));
        assert!(!subtree.contains(&child.id));
    }

    #[test]
    fn parent_spending_excludes_child_with_own_budget_same_mode() {
        // When a child has its own explicit budget (same mode as parent),
        // the parent's spending should NOT include the child's transactions.
        // Otherwise the frontend double-counts: parent.spent includes child
        // spending, AND the child appears as a separate status entry.
        let parent = {
            let mut c = make_category(10, "Food", None);
            c.budget = BudgetConfig::Monthly {
                amount: dec!(500),
                budget_type: BudgetType::Variable,
            };
            c
        };
        let child_with_budget = {
            let mut c = make_category(11, "Dining Out", Some(10));
            c.budget = BudgetConfig::Monthly {
                amount: dec!(100),
                budget_type: BudgetType::Variable,
            };
            c
        };
        // Child without its own budget — should still be included in parent
        let child_no_budget = make_category(12, "Groceries", Some(10));

        let categories = vec![
            parent.clone(),
            child_with_budget.clone(),
            child_no_budget.clone(),
        ];

        let bm = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 1, 15),
            end_date: Some(date(2025, 2, 13)),
            salary_transactions_detected: 1,
        };

        let transactions = vec![
            make_txn(
                Categorization::Manual(parent.id),
                dec!(-20),
                date(2025, 1, 18),
            ),
            make_txn(
                Categorization::Manual(child_with_budget.id),
                dec!(-80),
                date(2025, 1, 20),
            ),
            make_txn(
                Categorization::Manual(child_no_budget.id),
                dec!(-50),
                date(2025, 1, 22),
            ),
        ];

        let parent_spending = compute_category_spending(&transactions, parent.id, &bm, &categories);
        // Parent should include its own $20 + Groceries $50 (no budget) = $70
        // but NOT Dining Out's $80 (has its own monthly budget)
        assert_eq!(parent_spending, dec!(70));

        let child_spending =
            compute_category_spending(&transactions, child_with_budget.id, &bm, &categories);
        // Child with its own budget tracks independently
        assert_eq!(child_spending, dec!(80));
    }

    #[test]
    fn subtree_excludes_child_with_own_budget_same_mode() {
        // collect_budget_subtree should exclude children that have their
        // own explicit budget, even when the mode matches the parent.
        let parent = {
            let mut c = make_category(10, "Parent", None);
            c.budget = BudgetConfig::Monthly {
                amount: dec!(500),
                budget_type: BudgetType::Variable,
            };
            c
        };
        let child = {
            let mut c = make_category(11, "Child", Some(10));
            c.budget = BudgetConfig::Monthly {
                amount: dec!(200),
                budget_type: BudgetType::Variable,
            };
            c
        };
        let categories = vec![parent.clone(), child.clone()];

        let subtree = collect_budget_subtree(parent.id, &categories);
        assert!(subtree.contains(&parent.id));
        // Child has its own budget → should NOT be in parent's subtree
        assert!(!subtree.contains(&child.id));
    }

    #[test]
    fn subtree_all_children_differ_returns_root_only() {
        let parent = {
            let mut c = make_category(10, "Parent", None);
            c.budget = BudgetConfig::Monthly {
                amount: dec!(500),
                budget_type: BudgetType::Variable,
            };
            c
        };
        let c1 = {
            let mut c = make_category(11, "Annual Child", Some(10));
            c.budget = BudgetConfig::Annual {
                amount: dec!(1200),
                budget_type: BudgetType::Variable,
            };
            c
        };
        let c2 = {
            let mut c = make_category(12, "Project Child", Some(10));
            c.budget = BudgetConfig::Project {
                amount: dec!(5000),
                start_date: date(2025, 1, 1),
                end_date: None,
            };
            c
        };
        let categories = vec![parent.clone(), c1, c2];

        let subtree = collect_budget_subtree(parent.id, &categories);
        assert_eq!(subtree, vec![parent.id]);
    }

    #[test]
    fn subtree_three_level_mixed_modes() {
        // monthly → (none/inherits) → (none/inherits)
        let root = {
            let mut c = make_category(10, "Root", None);
            c.budget = BudgetConfig::Monthly {
                amount: dec!(1000),
                budget_type: BudgetType::Variable,
            };
            c
        };
        let child = make_category(11, "Child", Some(10));
        let grandchild = make_category(12, "Grandchild", Some(11));
        let categories = vec![root.clone(), child.clone(), grandchild.clone()];

        let subtree = collect_budget_subtree(root.id, &categories);
        assert_eq!(subtree.len(), 3);
        assert!(subtree.contains(&root.id));
        assert!(subtree.contains(&child.id));
        assert!(subtree.contains(&grandchild.id));
    }

    #[test]
    fn subtree_grandchild_with_own_budget_excluded_but_inheriting_child_kept() {
        // Food (Monthly) → Groceries (inherits) → Organic (Monthly $50)
        // Groceries should be in Food's subtree; Organic should not.
        let root = {
            let mut c = make_category(10, "Food", None);
            c.budget = BudgetConfig::Monthly {
                amount: dec!(500),
                budget_type: BudgetType::Variable,
            };
            c
        };
        let child = make_category(11, "Groceries", Some(10));
        let grandchild = {
            let mut c = make_category(12, "Organic", Some(11));
            c.budget = BudgetConfig::Monthly {
                amount: dec!(50),
                budget_type: BudgetType::Variable,
            };
            c
        };
        let categories = vec![root.clone(), child.clone(), grandchild.clone()];

        let subtree = collect_budget_subtree(root.id, &categories);
        assert!(subtree.contains(&root.id));
        assert!(subtree.contains(&child.id));
        assert!(!subtree.contains(&grandchild.id));

        let bm = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 1, 15),
            end_date: Some(date(2025, 2, 13)),
            salary_transactions_detected: 1,
        };
        let transactions = vec![
            make_txn(
                Categorization::Manual(root.id),
                dec!(-10),
                date(2025, 1, 18),
            ),
            make_txn(
                Categorization::Manual(child.id),
                dec!(-40),
                date(2025, 1, 20),
            ),
            make_txn(
                Categorization::Manual(grandchild.id),
                dec!(-25),
                date(2025, 1, 22),
            ),
        ];

        // Food includes itself ($10) + Groceries ($40), but not Organic ($25)
        let root_spending = compute_category_spending(&transactions, root.id, &bm, &categories);
        assert_eq!(root_spending, dec!(50));

        // Organic tracks its own spending independently
        let gc_spending = compute_category_spending(&transactions, grandchild.id, &bm, &categories);
        assert_eq!(gc_spending, dec!(25));
    }

    // -----------------------------------------------------------------------
    // effective_budget_mode
    // -----------------------------------------------------------------------

    #[test]
    fn effective_mode_explicit_returned() {
        let cat = {
            let mut c = make_category(10, "Food", None);
            c.budget = BudgetConfig::Monthly {
                amount: dec!(500),
                budget_type: BudgetType::Variable,
            };
            c
        };
        assert_eq!(
            effective_budget_mode(&cat, &[cat.clone()]),
            Some(BudgetMode::Monthly)
        );
    }

    #[test]
    fn effective_mode_inherits_from_parent() {
        let parent = {
            let mut c = make_category(10, "Food", None);
            c.budget = BudgetConfig::Annual {
                amount: dec!(6000),
                budget_type: BudgetType::Variable,
            };
            c
        };
        let child = make_category(11, "Groceries", Some(10));
        let categories = vec![parent.clone(), child.clone()];

        assert_eq!(
            effective_budget_mode(&child, &categories),
            Some(BudgetMode::Annual)
        );
    }

    #[test]
    fn effective_mode_no_parent_no_mode_is_none() {
        let cat = make_category(10, "Misc", None);
        assert_eq!(effective_budget_mode(&cat, &[cat.clone()]), None);
    }

    #[test]
    fn effective_mode_child_overrides_parent() {
        let parent = {
            let mut c = make_category(10, "Food", None);
            c.budget = BudgetConfig::Monthly {
                amount: dec!(500),
                budget_type: BudgetType::Variable,
            };
            c
        };
        let child = {
            let mut c = make_category(11, "Groceries", Some(10));
            c.budget = BudgetConfig::Annual {
                amount: dec!(6000),
                budget_type: BudgetType::Variable,
            };
            c
        };
        let categories = vec![parent, child.clone()];

        assert_eq!(
            effective_budget_mode(&child, &categories),
            Some(BudgetMode::Annual)
        );
    }

    #[test]
    fn effective_mode_orphan_child_is_none() {
        // parent_id points to a non-existent category
        let child = make_category(11, "Orphan", Some(999));
        assert_eq!(effective_budget_mode(&child, &[child.clone()]), None);
    }

    // -----------------------------------------------------------------------
    // is_in_budget_month boundaries
    // -----------------------------------------------------------------------

    #[test]
    fn budget_month_on_start_date_is_true() {
        let bm = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 1, 15),
            end_date: Some(date(2025, 2, 14)),
            salary_transactions_detected: 1,
        };
        assert!(is_in_budget_month(date(2025, 1, 15), &bm));
    }

    #[test]
    fn budget_month_on_end_date_is_true() {
        let bm = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 1, 15),
            end_date: Some(date(2025, 2, 14)),
            salary_transactions_detected: 1,
        };
        assert!(is_in_budget_month(date(2025, 2, 14), &bm));
    }

    #[test]
    fn budget_month_day_before_start_is_false() {
        let bm = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 1, 15),
            end_date: Some(date(2025, 2, 14)),
            salary_transactions_detected: 1,
        };
        assert!(!is_in_budget_month(date(2025, 1, 14), &bm));
    }

    #[test]
    fn budget_month_day_after_end_is_false() {
        let bm = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 1, 15),
            end_date: Some(date(2025, 2, 14)),
            salary_transactions_detected: 1,
        };
        assert!(!is_in_budget_month(date(2025, 2, 15), &bm));
    }

    #[test]
    fn budget_month_open_ended_includes_far_future() {
        let bm = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 1, 15),
            end_date: None,
            salary_transactions_detected: 1,
        };
        assert!(is_in_budget_month(date(2030, 12, 31), &bm));
    }

    #[test]
    fn budget_month_open_ended_excludes_before_start() {
        let bm = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 1, 15),
            end_date: None,
            salary_transactions_detected: 1,
        };
        assert!(!is_in_budget_month(date(2025, 1, 14), &bm));
    }

    // -----------------------------------------------------------------------
    // Hierarchy + mode spending
    // -----------------------------------------------------------------------

    #[test]
    fn annual_parent_with_monthly_child_excludes_child() {
        let parent = {
            let mut c = make_category(10, "Insurance", None);
            c.budget = BudgetConfig::Annual {
                amount: dec!(2400),
                budget_type: BudgetType::Variable,
            };
            c
        };
        let child = {
            let mut c = make_category(11, "Monthly Sub", Some(10));
            c.budget = BudgetConfig::Monthly {
                amount: dec!(200),
                budget_type: BudgetType::Variable,
            };
            c
        };
        let categories = vec![parent.clone(), child.clone()];

        let bm = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 1, 15),
            end_date: Some(date(2025, 2, 14)),
            salary_transactions_detected: 1,
        };

        let transactions = vec![
            make_txn(
                Categorization::Manual(parent.id),
                dec!(-100),
                date(2025, 1, 20),
            ),
            make_txn(
                Categorization::Manual(child.id),
                dec!(-50),
                date(2025, 1, 25),
            ),
        ];

        // Annual parent spending should NOT include the monthly child
        let spending = compute_category_spending(&transactions, parent.id, &bm, &categories);
        assert_eq!(spending, dec!(100));
    }

    #[test]
    fn monthly_parent_with_project_child_excludes_child() {
        let parent = {
            let mut c = make_category(10, "Home", None);
            c.budget = BudgetConfig::Monthly {
                amount: dec!(1000),
                budget_type: BudgetType::Variable,
            };
            c
        };
        let child = {
            let mut c = make_category(11, "Renovation", Some(10));
            c.budget = BudgetConfig::Project {
                amount: dec!(10000),
                start_date: date(2025, 1, 1),
                end_date: None,
            };
            c
        };
        let categories = vec![parent.clone(), child.clone()];

        let bm = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 1, 15),
            end_date: Some(date(2025, 2, 14)),
            salary_transactions_detected: 1,
        };

        let transactions = vec![
            make_txn(
                Categorization::Manual(parent.id),
                dec!(-200),
                date(2025, 1, 20),
            ),
            make_txn(
                Categorization::Manual(child.id),
                dec!(-5000),
                date(2025, 1, 25),
            ),
        ];

        // Monthly parent should NOT include the project child
        let spending = compute_category_spending(&transactions, parent.id, &bm, &categories);
        assert_eq!(spending, dec!(200));
    }

    #[test]
    fn all_children_inherit_mode_all_contribute() {
        let parent = {
            let mut c = make_category(10, "Food", None);
            c.budget = BudgetConfig::Monthly {
                amount: dec!(500),
                budget_type: BudgetType::Variable,
            };
            c
        };
        let c1 = make_category(11, "Groceries", Some(10));
        let c2 = make_category(12, "Dining", Some(10));
        let categories = vec![parent.clone(), c1.clone(), c2.clone()];

        let bm = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 1, 15),
            end_date: Some(date(2025, 2, 14)),
            salary_transactions_detected: 1,
        };

        let transactions = vec![
            make_txn(Categorization::Manual(c1.id), dec!(-100), date(2025, 1, 20)),
            make_txn(Categorization::Manual(c2.id), dec!(-80), date(2025, 1, 25)),
        ];

        let spending = compute_category_spending(&transactions, parent.id, &bm, &categories);
        assert_eq!(spending, dec!(180));
    }

    #[test]
    fn mixed_some_inherit_some_override() {
        let parent = {
            let mut c = make_category(10, "Food", None);
            c.budget = BudgetConfig::Monthly {
                amount: dec!(500),
                budget_type: BudgetType::Variable,
            };
            c
        };
        let inherits = make_category(11, "Groceries", Some(10)); // inherits monthly
        let overrides = {
            let mut c = make_category(12, "Dining Annual", Some(10));
            c.budget = BudgetConfig::Annual {
                amount: dec!(3000),
                budget_type: BudgetType::Variable,
            };
            c
        };
        let categories = vec![parent.clone(), inherits.clone(), overrides.clone()];

        let bm = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 1, 15),
            end_date: Some(date(2025, 2, 14)),
            salary_transactions_detected: 1,
        };

        let transactions = vec![
            make_txn(
                Categorization::Manual(inherits.id),
                dec!(-100),
                date(2025, 1, 20),
            ),
            make_txn(
                Categorization::Manual(overrides.id),
                dec!(-200),
                date(2025, 1, 25),
            ),
        ];

        // Only groceries (inherits) contributes to monthly parent
        let spending = compute_category_spending(&transactions, parent.id, &bm, &categories);
        assert_eq!(spending, dec!(100));
    }

    // -----------------------------------------------------------------------
    // is_correlated_exclusion
    // -----------------------------------------------------------------------

    #[test]
    fn correlated_transfer_excluded() {
        let txn = Transaction {
            correlation: Some(Correlation {
                partner_id: TransactionId::new(),
                correlation_type: CorrelationType::Transfer,
            }),
            ..Default::default()
        };
        let empty = HashSet::new();
        assert!(is_correlated_exclusion(&txn, &empty));
    }

    #[test]
    fn correlated_reimbursement_excluded() {
        let txn = Transaction {
            correlation: Some(Correlation {
                partner_id: TransactionId::new(),
                correlation_type: CorrelationType::Reimbursement,
            }),
            ..Default::default()
        };
        let empty = HashSet::new();
        assert!(is_correlated_exclusion(&txn, &empty));
    }

    #[test]
    fn reimbursed_original_excluded() {
        let txn = Transaction::default();
        let mut reimbursed = HashSet::new();
        reimbursed.insert(txn.id);
        assert!(is_correlated_exclusion(&txn, &reimbursed));
    }

    #[test]
    fn normal_transaction_passes_exclusion() {
        let txn = Transaction::default();
        let empty = HashSet::new();
        assert!(!is_correlated_exclusion(&txn, &empty));
    }

    // -----------------------------------------------------------------------
    // BudgetConfig construction
    // -----------------------------------------------------------------------

    #[test]
    fn budget_config_from_parts_monthly() {
        let bc = BudgetConfig::from_parts(
            Some(BudgetMode::Monthly),
            Some(BudgetType::Fixed),
            Some(dec!(500)),
            None,
            None,
        );
        assert_eq!(bc.mode(), Some(BudgetMode::Monthly));
        assert_eq!(bc.amount(), Some(dec!(500)));
        assert_eq!(bc.budget_type(), Some(BudgetType::Fixed));
    }

    #[test]
    fn budget_config_from_parts_annual() {
        let bc =
            BudgetConfig::from_parts(Some(BudgetMode::Annual), None, Some(dec!(6000)), None, None);
        assert_eq!(bc.mode(), Some(BudgetMode::Annual));
        assert_eq!(bc.amount(), Some(dec!(6000)));
        // defaults to Variable
        assert_eq!(bc.budget_type(), Some(BudgetType::Variable));
    }

    #[test]
    fn budget_config_from_parts_project() {
        let bc = BudgetConfig::from_parts(
            Some(BudgetMode::Project),
            None,
            Some(dec!(10000)),
            Some(date(2025, 1, 1)),
            Some(date(2025, 6, 30)),
        );
        assert_eq!(bc.mode(), Some(BudgetMode::Project));
        assert_eq!(bc.amount(), Some(dec!(10000)));
        assert_eq!(bc.budget_type(), None); // projects have no budget_type
        if let BudgetConfig::Project {
            start_date,
            end_date,
            ..
        } = &bc
        {
            assert_eq!(*start_date, date(2025, 1, 1));
            assert_eq!(*end_date, Some(date(2025, 6, 30)));
        } else {
            panic!("expected Project variant");
        }
    }

    #[test]
    fn budget_config_from_parts_none_when_all_null() {
        let bc = BudgetConfig::from_parts(None, None, None, None, None);
        assert!(matches!(bc, BudgetConfig::None));
    }

    #[test]
    fn budget_config_from_parts_project_without_start_falls_back_to_none() {
        let bc = BudgetConfig::from_parts(
            Some(BudgetMode::Project),
            None,
            Some(dec!(10000)),
            None,
            Some(date(2025, 6, 30)),
        );
        assert!(matches!(bc, BudgetConfig::None));
    }

    #[test]
    fn budget_config_from_parts_monthly_defaults_amount_to_zero() {
        let bc = BudgetConfig::from_parts(
            Some(BudgetMode::Monthly),
            None,
            None, // no amount
            None,
            None,
        );
        assert_eq!(bc.amount(), Some(Decimal::ZERO));
    }

    // -----------------------------------------------------------------------
    // BudgetConfig serde roundtrip
    // -----------------------------------------------------------------------

    #[test]
    fn budget_config_serde_roundtrip_monthly() {
        let bc = BudgetConfig::Monthly {
            amount: dec!(500),
            budget_type: BudgetType::Variable,
        };
        let json = serde_json::to_string(&bc).expect("serialize");
        let parsed: BudgetConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.mode(), bc.mode());
        assert_eq!(parsed.amount(), bc.amount());
        assert_eq!(parsed.budget_type(), bc.budget_type());
    }

    #[test]
    fn budget_config_serde_roundtrip_project() {
        let bc = BudgetConfig::Project {
            amount: dec!(10000),
            start_date: date(2025, 1, 1),
            end_date: Some(date(2025, 6, 30)),
        };
        let json = serde_json::to_string(&bc).expect("serialize");
        let parsed: BudgetConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.mode(), bc.mode());
        assert_eq!(parsed.amount(), bc.amount());
        if let BudgetConfig::Project {
            start_date,
            end_date,
            ..
        } = &parsed
        {
            assert_eq!(*start_date, date(2025, 1, 1));
            assert_eq!(*end_date, Some(date(2025, 6, 30)));
        } else {
            panic!("expected Project variant after roundtrip");
        }
    }

    #[test]
    fn budget_config_serde_roundtrip_none() {
        let bc = BudgetConfig::None;
        let json = serde_json::to_string(&bc).expect("serialize");
        let parsed: BudgetConfig = serde_json::from_str(&json).expect("deserialize");
        assert!(matches!(parsed, BudgetConfig::None));
    }

    /// Budget months don't align with calendar months — salary typically arrives
    /// mid-to-late month, so a "January" budget month might start on Dec 30.
    /// Annual budget year must include the budget month that *contains* Jan 1,
    /// not just the one whose start_date falls in January.
    #[test]
    fn budget_year_includes_month_containing_january_first() {
        // Real-world scenario: salary arrives late December, so the budget month
        // that covers January starts on Dec 30.
        let bm_dec = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 12, 30),
            end_date: Some(date(2026, 1, 29)),
            salary_transactions_detected: 1,
        };
        let bm_jan = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2026, 1, 30),
            end_date: Some(date(2026, 2, 26)),
            salary_transactions_detected: 1,
        };
        let bm_feb = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2026, 2, 27),
            end_date: None,
            salary_transactions_detected: 1,
        };
        let all = [bm_dec.clone(), bm_jan.clone(), bm_feb.clone()];

        let year = budget_year_months(&all, &bm_feb);

        // The Dec-30-started month covers all of January — it must be included
        assert_eq!(
            year[0].id, bm_dec.id,
            "budget year should start at the month containing Jan 1, not the one starting Jan 30"
        );
        assert_eq!(year.len(), 3);
    }

    /// Annual spending must include transactions in the budget month that
    /// contains Jan 1, even when that month starts in late December.
    #[test]
    fn annual_status_includes_spending_from_month_containing_january() {
        let mut insurance = make_category(200, "Car Insurance", None);
        insurance.budget = BudgetConfig::Annual {
            amount: dec!(3200),
            budget_type: BudgetType::Fixed,
        };
        let categories = vec![insurance.clone()];

        // Budget months mirroring real data: salary arrives late month
        let bm_dec = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 12, 30),
            end_date: Some(date(2026, 1, 29)),
            salary_transactions_detected: 1,
        };
        let bm_jan = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2026, 1, 30),
            end_date: Some(date(2026, 2, 26)),
            salary_transactions_detected: 1,
        };
        let bm_feb = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2026, 2, 27),
            end_date: None,
            salary_transactions_detected: 1,
        };
        let all_months = [bm_dec.clone(), bm_jan.clone(), bm_feb.clone()];

        // Insurance premiums paid in early January (inside bm_dec: Dec 30 → Jan 29)
        let transactions = vec![
            make_txn(
                Categorization::Manual(insurance.id),
                dec!(-1208),
                date(2026, 1, 2),
            ),
            make_txn(
                Categorization::Manual(insurance.id),
                dec!(-70),
                date(2026, 1, 2),
            ),
            make_txn(
                Categorization::Manual(insurance.id),
                dec!(-1392),
                date(2026, 1, 5),
            ),
            make_txn(
                Categorization::Manual(insurance.id),
                dec!(-529),
                date(2026, 1, 15),
            ),
        ];

        let today = date(2026, 3, 4);
        let status = compute_budget_status(
            &insurance,
            &transactions,
            &bm_feb,
            &all_months,
            &categories,
            today,
            None,
        );

        assert_eq!(status.budget_mode, BudgetMode::Annual);
        // All four transactions should be counted: 1208 + 70 + 1392 + 529 = 3199
        assert_eq!(status.spent, dec!(3199));
        assert_eq!(status.remaining, dec!(1)); // 3200 - 3199
    }

    // -----------------------------------------------------------------------
    // Inter-month gap transaction assignment
    // -----------------------------------------------------------------------

    #[test]
    fn gap_transaction_before_salary_belongs_to_previous_month() {
        let categories = vec![salary_category(), food_category()];
        let transactions = vec![
            // Dec salary
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(3000),
                date(2025, 12, 18),
            ),
            // Jan salary
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(3000),
                date(2026, 1, 26),
            ),
            // Feb salary
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(3000),
                date(2026, 2, 26),
            ),
            // Gap transaction: Jan 5 is between calendar month start and Jan 26 salary
            make_txn(
                Categorization::Manual(food_category().id),
                dec!(-50),
                date(2026, 1, 5),
            ),
        ];

        let months = detect_budget_month_boundaries(&transactions, nz(1), &categories)
            .expect("should detect months");

        assert_eq!(months.len(), 3);
        // Jan 5 falls in month 0 (Dec 18 – Jan 25)
        assert!(is_in_budget_month(date(2026, 1, 5), &months[0]));
        assert!(!is_in_budget_month(date(2026, 1, 5), &months[1]));
    }

    #[test]
    fn gap_transaction_day_before_salary_belongs_to_previous_month() {
        let categories = vec![salary_category(), food_category()];
        let transactions = vec![
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(3000),
                date(2025, 12, 18),
            ),
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(3000),
                date(2026, 1, 26),
            ),
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(3000),
                date(2026, 2, 26),
            ),
        ];

        let months = detect_budget_month_boundaries(&transactions, nz(1), &categories)
            .expect("should detect months");

        // Jan 25 is the day before the Jan 26 salary — still in month 0
        assert!(is_in_budget_month(date(2026, 1, 25), &months[0]));
        assert!(!is_in_budget_month(date(2026, 1, 25), &months[1]));
    }

    #[test]
    fn transaction_on_salary_day_belongs_to_new_month() {
        let categories = vec![salary_category(), food_category()];
        let transactions = vec![
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(3000),
                date(2025, 12, 18),
            ),
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(3000),
                date(2026, 1, 26),
            ),
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(3000),
                date(2026, 2, 26),
            ),
        ];

        let months = detect_budget_month_boundaries(&transactions, nz(1), &categories)
            .expect("should detect months");

        // Jan 26 is the salary day — belongs to month 1, not month 0
        assert!(!is_in_budget_month(date(2026, 1, 26), &months[0]));
        assert!(is_in_budget_month(date(2026, 1, 26), &months[1]));
    }

    #[test]
    fn gap_with_skipped_month_extends_previous() {
        let categories = vec![salary_category(), food_category()];
        // Expected 2 salaries/month. Jan has 2, Feb has only 1 (skipped), Mar has 2.
        let transactions = vec![
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(2000),
                date(2026, 1, 10),
            ),
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(2000),
                date(2026, 1, 25),
            ),
            // Feb only has 1 salary — skipped
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(2000),
                date(2026, 2, 10),
            ),
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(2000),
                date(2026, 3, 10),
            ),
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(2000),
                date(2026, 3, 25),
            ),
        ];

        let months = detect_budget_month_boundaries(&transactions, nz(2), &categories)
            .expect("should detect months");

        assert_eq!(months.len(), 2);
        assert_eq!(months[0].start_date, date(2026, 1, 10));
        assert_eq!(months[1].start_date, date(2026, 3, 10));

        // Feb 15 falls in the Jan budget month (which extends to Mar 9)
        assert!(is_in_budget_month(date(2026, 2, 15), &months[0]));
        assert!(!is_in_budget_month(date(2026, 2, 15), &months[1]));
    }

    #[test]
    fn transaction_before_first_budget_month_is_covered() {
        let categories = vec![salary_category(), food_category()];
        let transactions = vec![
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(3000),
                date(2026, 1, 15),
            ),
            // Transaction before the first budget month
            make_txn(
                Categorization::Manual(food_category().id),
                dec!(-30),
                date(2026, 1, 5),
            ),
        ];

        let months = detect_budget_month_boundaries(&transactions, nz(1), &categories)
            .expect("should detect months");

        assert_eq!(months.len(), 1);
        // After the fix, the first budget month should extend back to cover Jan 5
        assert!(
            is_in_budget_month(date(2026, 1, 5), &months[0]),
            "first budget month should extend back to cover earlier transactions"
        );
    }

    #[test]
    fn spending_includes_gap_transactions_in_previous_month() {
        let food = food_category();
        let categories = vec![salary_category(), food.clone()];
        let transactions = vec![
            // Dec salary
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(3000),
                date(2025, 12, 18),
            ),
            // Jan salary
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(3000),
                date(2026, 1, 26),
            ),
            // Gap transaction on Jan 5 — should be in Dec budget month
            make_txn(Categorization::Manual(food.id), dec!(-50), date(2026, 1, 5)),
        ];

        let months = detect_budget_month_boundaries(&transactions, nz(1), &categories)
            .expect("should detect months");

        // Dec month spending should include the Jan 5 gap transaction
        let spending = compute_category_spending(&transactions, food.id, &months[0], &categories);
        assert_eq!(spending, dec!(50));
    }

    #[test]
    fn spending_excludes_gap_transactions_from_new_month() {
        let food = food_category();
        let categories = vec![salary_category(), food.clone()];
        let transactions = vec![
            // Dec salary
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(3000),
                date(2025, 12, 18),
            ),
            // Jan salary
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(3000),
                date(2026, 1, 26),
            ),
            // Feb salary
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(3000),
                date(2026, 2, 26),
            ),
            // Gap transaction on Jan 5 — belongs to Dec, not Jan
            make_txn(Categorization::Manual(food.id), dec!(-50), date(2026, 1, 5)),
            // Transaction after salary on Jan 28 — belongs to Jan
            make_txn(
                Categorization::Manual(food.id),
                dec!(-30),
                date(2026, 1, 28),
            ),
        ];

        let months = detect_budget_month_boundaries(&transactions, nz(1), &categories)
            .expect("should detect months");

        // Jan month spending should NOT include the Jan 5 gap transaction
        let spending = compute_category_spending(&transactions, food.id, &months[1], &categories);
        assert_eq!(spending, dec!(30));
    }

    #[test]
    fn first_budget_month_covers_earlier_transactions() {
        let food = food_category();
        let categories = vec![salary_category(), food.clone()];
        let transactions = vec![
            // First-ever salary on Jan 15
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(3000),
                date(2026, 1, 15),
            ),
            // Transactions before the first salary
            make_txn(Categorization::Manual(food.id), dec!(-20), date(2026, 1, 1)),
            make_txn(Categorization::Manual(food.id), dec!(-30), date(2026, 1, 5)),
            make_txn(
                Categorization::Manual(food.id),
                dec!(-10),
                date(2026, 1, 14),
            ),
        ];

        let months = detect_budget_month_boundaries(&transactions, nz(1), &categories)
            .expect("should detect months");

        assert_eq!(months.len(), 1);

        // All pre-salary transactions should be covered by the first month
        assert!(is_in_budget_month(date(2026, 1, 1), &months[0]));
        assert!(is_in_budget_month(date(2026, 1, 5), &months[0]));
        assert!(is_in_budget_month(date(2026, 1, 14), &months[0]));

        // Spending should include all three pre-salary transactions
        let spending = compute_category_spending(&transactions, food.id, &months[0], &categories);
        assert_eq!(spending, dec!(60)); // 20 + 30 + 10
    }

    #[test]
    fn first_month_not_extended_when_only_salary_transactions_provided() {
        // detect_budget_month_boundaries only extends the first month
        // backward when it can see non-salary transactions. The API's
        // derive_months() passes salary-only data, then compensates
        // with a separate MIN(posted_date) query.
        let categories = vec![salary_category(), food_category()];
        let salary_only = vec![make_txn(
            Categorization::Manual(salary_cat_id()),
            dec!(3000),
            date(2026, 1, 15),
        )];

        let months = detect_budget_month_boundaries(&salary_only, nz(1), &categories)
            .expect("should detect months");

        assert_eq!(months.len(), 1);
        assert_eq!(months[0].start_date, date(2026, 1, 15));
        assert!(!is_in_budget_month(date(2026, 1, 1), &months[0]));
    }

    #[test]
    fn first_month_extended_across_calendar_month_boundary() {
        // Pre-salary transactions in an earlier calendar month should
        // still be covered by backward extension.
        let food = food_category();
        let categories = vec![salary_category(), food.clone()];
        let transactions = vec![
            make_txn(
                Categorization::Manual(food.id),
                dec!(-25),
                date(2025, 12, 20),
            ),
            make_txn(Categorization::Manual(food.id), dec!(-15), date(2026, 1, 3)),
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(3000),
                date(2026, 1, 15),
            ),
        ];

        let months = detect_budget_month_boundaries(&transactions, nz(1), &categories)
            .expect("should detect months");

        assert_eq!(months.len(), 1);
        assert_eq!(months[0].start_date, date(2025, 12, 20));
        assert!(is_in_budget_month(date(2025, 12, 20), &months[0]));
        assert!(is_in_budget_month(date(2026, 1, 3), &months[0]));

        let spending = compute_category_spending(&transactions, food.id, &months[0], &categories);
        assert_eq!(spending, dec!(40)); // 25 + 15
    }

    // -----------------------------------------------------------------------
    // build_daily_cumulative_series tests
    // -----------------------------------------------------------------------

    fn food_cat_id() -> CategoryId {
        CategoryId::from_uuid(uuid::Uuid::from_u128(100))
    }

    #[test]
    fn daily_cumulative_empty_transactions() {
        let month = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 3, 15),
            end_date: Some(date(2025, 4, 13)),
            salary_transactions_detected: 1,
        };
        let txns: Vec<&Transaction> = vec![];
        let subtree = vec![food_cat_id()];

        let points = build_daily_cumulative_series(&txns, &subtree, &month, date(2025, 4, 13));
        assert_eq!(points.len(), 30);
        for p in &points {
            assert_eq!(p.cumulative, Decimal::ZERO);
        }
    }

    #[test]
    fn daily_cumulative_single_transaction_on_day_3() {
        let month = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 3, 15),
            end_date: Some(date(2025, 3, 19)),
            salary_transactions_detected: 1,
        };
        let txn = make_txn(
            Categorization::Manual(food_cat_id()),
            dec!(-50),
            date(2025, 3, 17),
        );
        let txns: Vec<&Transaction> = vec![&txn];
        let subtree = vec![food_cat_id()];

        let points = build_daily_cumulative_series(&txns, &subtree, &month, date(2025, 3, 19));
        assert_eq!(points.len(), 5);
        // Days 1-2: zero
        assert_eq!(points[0].cumulative, Decimal::ZERO);
        assert_eq!(points[1].cumulative, Decimal::ZERO);
        // Day 3 onward: 50
        assert_eq!(points[2].cumulative, dec!(50));
        assert_eq!(points[3].cumulative, dec!(50));
        assert_eq!(points[4].cumulative, dec!(50));
    }

    #[test]
    fn daily_cumulative_multiple_transactions_same_day() {
        let month = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 3, 15),
            end_date: Some(date(2025, 3, 17)),
            salary_transactions_detected: 1,
        };
        let t1 = make_txn(
            Categorization::Manual(food_cat_id()),
            dec!(-30),
            date(2025, 3, 16),
        );
        let t2 = make_txn(
            Categorization::Manual(food_cat_id()),
            dec!(-20),
            date(2025, 3, 16),
        );
        let txns: Vec<&Transaction> = vec![&t1, &t2];
        let subtree = vec![food_cat_id()];

        let points = build_daily_cumulative_series(&txns, &subtree, &month, date(2025, 3, 17));
        assert_eq!(points.len(), 3);
        assert_eq!(points[0].cumulative, Decimal::ZERO);
        assert_eq!(points[1].cumulative, dec!(50));
        assert_eq!(points[2].cumulative, dec!(50));
    }

    #[test]
    fn daily_cumulative_open_month_uses_through_date() {
        let month = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 3, 15),
            end_date: None,
            salary_transactions_detected: 1,
        };
        let txn = make_txn(
            Categorization::Manual(food_cat_id()),
            dec!(-100),
            date(2025, 3, 15),
        );
        let txns: Vec<&Transaction> = vec![&txn];
        let subtree = vec![food_cat_id()];

        let points = build_daily_cumulative_series(&txns, &subtree, &month, date(2025, 3, 17));
        // Should only go through through_date (3 days)
        assert_eq!(points.len(), 3);
        assert_eq!(points[0].cumulative, dec!(100));
        assert_eq!(points[2].cumulative, dec!(100));
    }

    // -----------------------------------------------------------------------
    // predict_salary_arrivals
    // -----------------------------------------------------------------------

    #[test]
    fn salary_prediction_no_salary_categories() {
        let categories = vec![food_category()];
        let transactions = vec![];
        let status = predict_salary_arrivals(&transactions, &categories, date(2025, 3, 16), nz(1));
        assert!(status.sources.is_empty());
        assert!(status.all_arrived);
        assert!(!status.any_late);
    }

    #[test]
    fn salary_prediction_single_source_arrived() {
        let categories = vec![salary_category()];
        let transactions = vec![
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(3000),
                date(2025, 1, 15),
            ),
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(3000),
                date(2025, 2, 14),
            ),
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(3000),
                date(2025, 3, 15),
            ),
        ];
        let status = predict_salary_arrivals(&transactions, &categories, date(2025, 3, 16), nz(1));
        assert_eq!(status.arrived_count, 1);
        assert!(status.all_arrived);
        assert!(!status.any_late);
        assert_eq!(status.sources.len(), 1);
        assert!(status.sources[0].arrived);
        assert_eq!(status.sources[0].arrived_date, Some(date(2025, 3, 15)));
    }

    #[test]
    fn salary_prediction_sufficient_history() {
        let categories = vec![salary_category()];
        let transactions = vec![
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(3000),
                date(2024, 10, 25),
            ),
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(3000),
                date(2024, 11, 25),
            ),
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(3000),
                date(2024, 12, 24),
            ),
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(3000),
                date(2025, 1, 25),
            ),
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(3000),
                date(2025, 2, 25),
            ),
        ];
        // March 10 — salary not yet arrived
        let status = predict_salary_arrivals(&transactions, &categories, date(2025, 3, 10), nz(1));
        assert_eq!(status.arrived_count, 0);
        assert!(!status.all_arrived);
        let src = &status.sources[0];
        assert!(!src.arrived);
        assert!(src.predicted_day.is_some());
        // With consistent ~25th arrivals, prediction should be around 24-26
        let day = src.predicted_day.unwrap();
        assert!(day >= 20 && day <= 31, "predicted day {day} out of range");
    }

    #[test]
    fn salary_prediction_late() {
        let categories = vec![salary_category()];
        let transactions = vec![
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(3000),
                date(2024, 10, 15),
            ),
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(3000),
                date(2024, 11, 15),
            ),
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(3000),
                date(2024, 12, 15),
            ),
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(3000),
                date(2025, 1, 15),
            ),
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(3000),
                date(2025, 2, 14),
            ),
        ];
        // March 28 — well past predicted upper bound (~15-16)
        let status = predict_salary_arrivals(&transactions, &categories, date(2025, 3, 28), nz(1));
        assert!(!status.all_arrived);
        assert!(status.any_late);
        assert!(status.sources[0].late);
    }

    #[test]
    fn salary_prediction_multiple_sources_partial() {
        let salary2 = Category {
            budget: BudgetConfig::Salary,
            ..make_category(2, "Government", None)
        };
        let cat2_id = CategoryId::from_uuid(uuid::Uuid::from_u128(2));
        let categories = vec![salary_category(), salary2];
        let transactions = vec![
            // Source 1 arrived in March
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(3000),
                date(2025, 1, 15),
            ),
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(3000),
                date(2025, 2, 15),
            ),
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(3000),
                date(2025, 3, 14),
            ),
            // Source 2 not arrived in March
            make_txn(
                Categorization::Manual(cat2_id),
                dec!(500),
                date(2025, 1, 20),
            ),
            make_txn(
                Categorization::Manual(cat2_id),
                dec!(500),
                date(2025, 2, 20),
            ),
        ];
        let status = predict_salary_arrivals(&transactions, &categories, date(2025, 3, 16), nz(2));
        assert_eq!(status.expected_count, 2);
        assert_eq!(status.arrived_count, 1);
        assert!(!status.all_arrived);
        assert_eq!(status.sources.len(), 2);
        let arrived: Vec<_> = status.sources.iter().filter(|s| s.arrived).collect();
        let pending: Vec<_> = status.sources.iter().filter(|s| !s.arrived).collect();
        assert_eq!(arrived.len(), 1);
        assert_eq!(pending.len(), 1);
    }

    #[test]
    fn salary_prediction_insufficient_history_fallback() {
        let categories = vec![salary_category()];
        // Only 2 months of history — below ETS threshold of 3
        let transactions = vec![
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(3000),
                date(2025, 1, 20),
            ),
            make_txn(
                Categorization::Manual(salary_cat_id()),
                dec!(3000),
                date(2025, 2, 22),
            ),
        ];
        let status = predict_salary_arrivals(&transactions, &categories, date(2025, 3, 10), nz(1));
        let src = &status.sources[0];
        assert!(!src.arrived);
        // Fallback to median/range
        assert!(src.predicted_day.is_some());
        let day = src.predicted_day.unwrap();
        assert!(day >= 20 && day <= 22, "fallback predicted day {day}");
        assert_eq!(src.predicted_day_lower, Some(20));
        assert_eq!(src.predicted_day_upper, Some(22));
    }

    #[test]
    fn salary_prediction_ets_failure_graceful() {
        let categories = vec![salary_category()];
        // Single month — no ETS possible, should fall back
        let transactions = vec![make_txn(
            Categorization::Manual(salary_cat_id()),
            dec!(3000),
            date(2025, 1, 15),
        )];
        let status = predict_salary_arrivals(&transactions, &categories, date(2025, 2, 10), nz(1));
        let src = &status.sources[0];
        assert!(!src.arrived);
        // Single data point → median fallback
        assert_eq!(src.predicted_day, Some(15));
        assert_eq!(src.predicted_day_lower, Some(15));
        assert_eq!(src.predicted_day_upper, Some(15));
    }
}
