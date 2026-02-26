use chrono::{Datelike, NaiveDate};
use rust_decimal::Decimal;

use crate::error::Error;
use crate::models::{
    BudgetMode, BudgetMonth, BudgetMonthId, BudgetStatus, Category, CategoryId, PaceIndicator,
    Transaction,
};

/// Collect all descendant category IDs for a given category (including itself).
fn collect_category_subtree(category_id: CategoryId, categories: &[Category]) -> Vec<CategoryId> {
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
fn project_category_ids(categories: &[Category]) -> std::collections::HashSet<CategoryId> {
    let project_roots: Vec<CategoryId> = categories
        .iter()
        .filter(|c| c.budget_mode == Some(BudgetMode::Project))
        .map(|c| c.id)
        .collect();

    let mut ids = std::collections::HashSet::new();
    for root in project_roots {
        for id in collect_category_subtree(root, categories) {
            ids.insert(id);
        }
    }
    ids
}

/// Filter transactions to only include those relevant to regular budget math.
///
/// Excludes:
/// - Transactions in project-mode categories (project isolation)
/// - Correlated transfers (net to zero, not an expense)
/// - Correlated reimbursements on the reimbursing side (the original expense
///   is also excluded since the reimbursement nets it out)
fn filter_for_budget<'a>(
    transactions: &'a [Transaction],
    categories: &[Category],
) -> Vec<&'a Transaction> {
    let project_cats = project_category_ids(categories);

    // Collect IDs of transactions that are reimbursed (have a correlation partner
    // with type "reimbursement")
    let reimbursed_ids: std::collections::HashSet<_> = transactions
        .iter()
        .filter(|t| {
            t.correlation_type
                .as_ref()
                .is_some_and(|ct| *ct == crate::models::CorrelationType::Reimbursement)
        })
        .filter_map(|t| t.correlation_id)
        .collect();

    transactions
        .iter()
        .filter(|t| {
            // Exclude transactions in project-mode categories
            if t.category_id.is_some_and(|cid| project_cats.contains(&cid)) {
                return false;
            }
            // Exclude transfers
            if t.correlation_type
                .as_ref()
                .is_some_and(|ct| *ct == crate::models::CorrelationType::Transfer)
            {
                return false;
            }
            // Exclude reimbursements themselves
            if t.correlation_type
                .as_ref()
                .is_some_and(|ct| *ct == crate::models::CorrelationType::Reimbursement)
            {
                return false;
            }
            // Exclude the original expense that was reimbursed
            if reimbursed_ids.contains(&t.id) {
                return false;
            }
            true
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
/// Returns `Error::NoSalaryCategory` if `salary_category_id` is `None`.
pub fn detect_budget_month_boundaries(
    transactions: &[Transaction],
    expected_salary_count: u32,
    salary_category_id: Option<CategoryId>,
    categories: &[Category],
) -> Result<Vec<BudgetMonth>, Error> {
    let salary_cat_id = salary_category_id.ok_or(Error::NoSalaryCategory)?;

    // Collect salary category subtree (e.g. Salary:Employer A, Salary:Employer B)
    let salary_cat_ids = collect_category_subtree(salary_cat_id, categories);

    // Find salary transactions (positive amounts in salary categories)
    let mut salary_txns: Vec<&Transaction> = transactions
        .iter()
        .filter(|t| {
            t.category_id
                .is_some_and(|cid| salary_cat_ids.contains(&cid))
                && t.amount > Decimal::ZERO
        })
        .collect();

    salary_txns.sort_by_key(|t| t.posted_date);

    // Group salary transactions by calendar month (year, month)
    let mut by_month: std::collections::BTreeMap<(i32, u32), Vec<NaiveDate>> =
        std::collections::BTreeMap::new();

    for txn in &salary_txns {
        let key = (txn.posted_date.year(), txn.posted_date.month());
        by_month.entry(key).or_default().push(txn.posted_date);
    }

    // For each calendar month that has >= expected_salary_count deposits,
    // the budget month starts on the last salary date
    let mut budget_months: Vec<BudgetMonth> = Vec::new();

    for dates in by_month.values() {
        if dates.len() >= expected_salary_count as usize {
            // Budget month starts on the last salary deposit date
            if let Some(&last_salary_date) = dates.iter().max() {
                let detected: i32 = dates.len().try_into().unwrap_or(i32::MAX);
                budget_months.push(BudgetMonth {
                    id: BudgetMonthId::new(),
                    start_date: last_salary_date,
                    end_date: None,
                    salary_transactions_detected: detected,
                });
            }
        }
    }

    budget_months.sort_by_key(|bm| bm.start_date);

    // Set end dates: each month ends the day before the next one starts
    for i in 0..budget_months.len().saturating_sub(1) {
        let next_start = budget_months[i + 1].start_date;
        budget_months[i].end_date = next_start.pred_opt();
    }

    Ok(budget_months)
}

/// Sum spending in a category (including children) for a budget month.
///
/// Respects category hierarchy: parent includes all children's spending.
/// Excludes project-mode, transfer, and reimbursed transactions.
#[must_use]
pub fn compute_category_spending(
    transactions: &[Transaction],
    category_id: CategoryId,
    budget_month: &BudgetMonth,
    categories: &[Category],
) -> Decimal {
    let subtree = collect_category_subtree(category_id, categories);
    let budget_txns = filter_for_budget(transactions, categories);

    -budget_txns
        .iter()
        .filter(|t| {
            // Must be in the category subtree
            t.category_id.is_some_and(|cid| subtree.contains(&cid))
            // Must fall within the budget month
            && is_in_budget_month(t.posted_date, budget_month)
        })
        .fold(Decimal::ZERO, |acc, t| acc + t.amount)
}

/// Check if a date falls within a budget month's boundaries.
fn is_in_budget_month(date: NaiveDate, budget_month: &BudgetMonth) -> bool {
    if date < budget_month.start_date {
        return false;
    }
    match budget_month.end_date {
        Some(end) => date <= end,
        None => true, // Open-ended month (current month)
    }
}

/// Compare actual spending to a pro-rata linear budget over elapsed/total periods.
fn compute_pace(spent: Decimal, budget: Decimal, elapsed: i64, total: i64) -> PaceIndicator {
    if total <= 0 || budget == Decimal::ZERO {
        if spent > budget {
            PaceIndicator::OverBudget
        } else {
            PaceIndicator::OnTrack
        }
    } else {
        let fraction = Decimal::from(elapsed) / Decimal::from(total);
        let expected_spend = budget * fraction;
        if spent > expected_spend {
            PaceIndicator::OverBudget
        } else {
            PaceIndicator::UnderBudget
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

    // Walk backwards to find the January-anchored start
    let mut year_start_idx = ref_idx;
    for i in (0..=ref_idx).rev() {
        year_start_idx = i;
        if all_months[i].start_date.month() == 1 {
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
    all_months: &[BudgetMonth],
    categories: &[Category],
    today: NaiveDate,
) -> BudgetStatus {
    let spent = compute_category_spending(transactions, category.id, current_month, categories);
    let budget_amount = category.budget_amount.unwrap_or(Decimal::ZERO);

    // Rollover from prior months only (exclude the month we're computing status for)
    let prior_months: Vec<BudgetMonth> = all_months
        .iter()
        .filter(|bm| bm.id != current_month.id)
        .cloned()
        .collect();
    let rollover = compute_rollover(category, &prior_months, transactions, categories);
    let effective_budget = budget_amount + rollover;
    let remaining = effective_budget - spent;

    let end_date = current_month
        .end_date
        .unwrap_or(current_month.start_date + chrono::Days::new(30));

    let time_left = (end_date - today).num_days().max(0);

    let total_days = (end_date - current_month.start_date).num_days();
    let elapsed_days = (today - current_month.start_date).num_days().max(0);
    let pace = compute_pace(spent, effective_budget, elapsed_days, total_days);

    BudgetStatus {
        category_id: category.id,
        category_name: category.name.clone(),
        budget_amount,
        spent,
        remaining,
        time_left,
        pace,
        budget_mode: BudgetMode::Monthly,
        rollover,
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

    // Sum spending across all months in the budget year
    let spent = year_months
        .iter()
        .map(|bm| compute_category_spending(transactions, category.id, bm, categories))
        .sum::<Decimal>();

    let budget_amount = category.budget_amount.unwrap_or(Decimal::ZERO);
    let remaining = budget_amount - spent;

    let total_year_months = i64::try_from(year_months.len()).unwrap_or(i64::MAX);
    // Elapsed months = months up to and including the reference month
    let elapsed_months = year_months
        .iter()
        .position(|bm| bm.id == current_month.id)
        .map_or(total_year_months, |idx| {
            i64::try_from(idx).unwrap_or(i64::MAX) + 1
        });
    let time_left = (total_year_months - elapsed_months).max(0);

    let pace = compute_pace(spent, budget_amount, elapsed_months, total_year_months);

    BudgetStatus {
        category_id: category.id,
        category_name: category.name.clone(),
        budget_amount,
        spent,
        remaining,
        time_left,
        pace,
        budget_mode: BudgetMode::Annual,
        rollover: Decimal::ZERO,
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

    // Collect reimbursed transaction IDs (same logic as filter_for_budget)
    let reimbursed_ids: std::collections::HashSet<_> = transactions
        .iter()
        .filter(|t| {
            t.correlation_type
                .as_ref()
                .is_some_and(|ct| *ct == crate::models::CorrelationType::Reimbursement)
        })
        .filter_map(|t| t.correlation_id)
        .collect();

    let start = category.project_start_date;
    let end = category.project_end_date;

    // Filter transactions within the project date range, excluding transfers/reimbursements
    let spent: Decimal = -transactions
        .iter()
        .filter(|t| {
            let in_subtree = t.category_id.is_some_and(|cid| subtree.contains(&cid));
            if !in_subtree {
                return false;
            }
            // Exclude transfers
            if t.correlation_type
                .as_ref()
                .is_some_and(|ct| *ct == crate::models::CorrelationType::Transfer)
            {
                return false;
            }
            // Exclude reimbursements
            if t.correlation_type
                .as_ref()
                .is_some_and(|ct| *ct == crate::models::CorrelationType::Reimbursement)
            {
                return false;
            }
            // Exclude reimbursed originals
            if reimbursed_ids.contains(&t.id) {
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

    let budget_amount = category.budget_amount.unwrap_or(Decimal::ZERO);
    let remaining = budget_amount - spent;

    let (time_left, pace) = match (start, end) {
        (Some(s), Some(e)) if budget_amount > Decimal::ZERO => {
            let total_days = (e - s).num_days();
            let elapsed_days = (today - s).num_days().max(0);
            let tl = (e - today).num_days().max(0);
            (
                tl,
                compute_pace(spent, budget_amount, elapsed_days, total_days),
            )
        }
        // Open-ended or no budget: can't compute pace
        _ => (-1, PaceIndicator::OnTrack),
    };

    BudgetStatus {
        category_id: category.id,
        category_name: category.name.clone(),
        budget_amount,
        spent,
        remaining,
        time_left,
        pace,
        budget_mode: BudgetMode::Project,
        rollover: Decimal::ZERO,
    }
}

/// Compute the full budget status for a category.
///
/// Dispatches to mode-specific logic based on the category's `budget_mode`.
#[must_use]
pub fn compute_budget_status(
    category: &Category,
    transactions: &[Transaction],
    current_month: &BudgetMonth,
    all_months: &[BudgetMonth],
    categories: &[Category],
    today: NaiveDate,
) -> BudgetStatus {
    match category.budget_mode {
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
        // Monthly (default)
        _ => compute_monthly_status(
            category,
            transactions,
            current_month,
            all_months,
            categories,
            today,
        ),
    }
}

/// Compute cumulative rollover for a monthly category across budget months.
///
/// Rollover = sum of (budget - spent) for each completed budget month.
/// Only applies to monthly budget categories. Annual/project categories do not
/// roll over.
#[must_use]
pub fn compute_rollover(
    category: &Category,
    budget_months: &[BudgetMonth],
    transactions: &[Transaction],
    categories: &[Category],
) -> Decimal {
    if category.budget_mode != Some(BudgetMode::Monthly) {
        return Decimal::ZERO;
    }

    let budget_amount = category.budget_amount.unwrap_or(Decimal::ZERO);

    // Only closed budget months contribute to rollover
    budget_months
        .iter()
        .filter(|bm| bm.end_date.is_some())
        .fold(Decimal::ZERO, |rollover, bm| {
            let spent = compute_category_spending(transactions, category.id, bm, categories);
            let surplus = budget_amount - spent;
            rollover + surplus
        })
}

/// Get all transactions assigned to a specific budget month, excluding project
/// transactions. Useful for computing overall totals.
#[must_use]
pub fn transactions_for_budget_month<'a>(
    transactions: &'a [Transaction],
    budget_month_id: BudgetMonthId,
    categories: &[Category],
) -> Vec<&'a Transaction> {
    let filtered = filter_for_budget(transactions, categories);
    filtered
        .into_iter()
        .filter(|t| t.budget_month_id == Some(budget_month_id))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AccountId, BudgetMonthId, CategoryId, TransactionId};
    use chrono::NaiveDate;
    use rust_decimal_macros::dec;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).expect("valid test date")
    }

    fn make_txn(
        category_id: Option<CategoryId>,
        amount: Decimal,
        posted_date: NaiveDate,
    ) -> Transaction {
        Transaction {
            id: TransactionId::new(),
            account_id: AccountId::new(),
            category_id,
            amount,
            original_amount: None,
            original_currency: None,
            merchant_name: "Test".to_owned(),
            description: "Test transaction".to_owned(),
            posted_date,
            budget_month_id: None,
            correlation_id: None,
            correlation_type: None,
            category_method: None,
            suggested_category: None,
            counterparty_name: None,
            counterparty_iban: None,
            counterparty_bic: None,
            bank_transaction_code: None,
        }
    }

    fn make_category(id: u128, name: &str, parent_id: Option<u128>) -> Category {
        Category {
            id: CategoryId::from_uuid(uuid::Uuid::from_u128(id)),
            name: name.to_owned(),
            parent_id: parent_id.map(|p| CategoryId::from_uuid(uuid::Uuid::from_u128(p))),
            budget_mode: None,
            budget_amount: None,
            project_start_date: None,
            project_end_date: None,
        }
    }

    fn salary_category() -> Category {
        make_category(1, "Salary", None)
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
        Category {
            budget_mode: Some(mode),
            budget_amount: Some(amount),
            ..food_category()
        }
    }

    #[test]
    fn detect_single_salary_budget_months() {
        let categories = vec![salary_category()];
        let transactions = vec![
            make_txn(Some(salary_cat_id()), dec!(3000), date(2025, 1, 15)),
            make_txn(Some(salary_cat_id()), dec!(3000), date(2025, 2, 14)),
            make_txn(Some(salary_cat_id()), dec!(3000), date(2025, 3, 15)),
        ];

        let months =
            detect_budget_month_boundaries(&transactions, 1, Some(salary_cat_id()), &categories)
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
            make_txn(Some(salary_cat_id()), dec!(2000), date(2025, 1, 10)),
            make_txn(Some(salary_cat_id()), dec!(2000), date(2025, 1, 25)),
            make_txn(Some(salary_cat_id()), dec!(2000), date(2025, 2, 10)),
            make_txn(Some(salary_cat_id()), dec!(2000), date(2025, 2, 24)),
        ];

        let months =
            detect_budget_month_boundaries(&transactions, 2, Some(salary_cat_id()), &categories)
                .expect("should detect months");

        assert_eq!(months.len(), 2);
        // Budget month starts on last salary of the calendar month
        assert_eq!(months[0].start_date, date(2025, 1, 25));
        assert_eq!(months[1].start_date, date(2025, 2, 24));
    }

    #[test]
    fn incomplete_salary_month_skipped() {
        let categories = vec![salary_category()];
        // Only 1 salary in February when 2 expected
        let transactions = vec![
            make_txn(Some(salary_cat_id()), dec!(2000), date(2025, 1, 10)),
            make_txn(Some(salary_cat_id()), dec!(2000), date(2025, 1, 25)),
            make_txn(Some(salary_cat_id()), dec!(2000), date(2025, 2, 10)),
            // Missing second salary in Feb
            make_txn(Some(salary_cat_id()), dec!(2000), date(2025, 3, 10)),
            make_txn(Some(salary_cat_id()), dec!(2000), date(2025, 3, 25)),
        ];

        let months =
            detect_budget_month_boundaries(&transactions, 2, Some(salary_cat_id()), &categories)
                .expect("should detect months");

        assert_eq!(months.len(), 2);
        assert_eq!(months[0].start_date, date(2025, 1, 25));
        assert_eq!(months[1].start_date, date(2025, 3, 25));
    }

    #[test]
    fn no_salary_category_returns_error() {
        let result = detect_budget_month_boundaries(&[], 1, None, &[]);
        assert!(result.is_err());
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
            make_txn(Some(groceries.id), dec!(-50), date(2025, 1, 20)),
            make_txn(Some(restaurants.id), dec!(-30), date(2025, 1, 22)),
            make_txn(Some(food.id), dec!(-10), date(2025, 1, 25)),
            // Outside budget month — should be excluded
            make_txn(Some(groceries.id), dec!(-100), date(2025, 2, 14)),
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
    fn project_transactions_excluded_from_budget() {
        let food = food_category();
        // A project category whose transactions should be excluded
        let mut project_cat = make_category(200, "Renovation", None);
        project_cat.budget_mode = Some(BudgetMode::Project);
        let categories = vec![food.clone(), project_cat.clone()];

        let bm = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 1, 15),
            end_date: Some(date(2025, 2, 13)),
            salary_transactions_detected: 1,
        };

        let transactions = vec![
            make_txn(Some(food.id), dec!(-50), date(2025, 1, 20)),
            // This transaction is in a project category — excluded from budget
            make_txn(Some(project_cat.id), dec!(-500), date(2025, 1, 20)),
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
    fn transfer_transactions_excluded() {
        let food = food_category();
        let categories = vec![food.clone()];

        let bm = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 1, 15),
            end_date: Some(date(2025, 2, 13)),
            salary_transactions_detected: 1,
        };

        let mut transfer_txn = make_txn(Some(food.id), dec!(-200), date(2025, 1, 20));
        transfer_txn.correlation_type = Some(crate::models::CorrelationType::Transfer);

        let transactions = vec![
            make_txn(Some(food.id), dec!(-50), date(2025, 1, 20)),
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
        let original_txn = make_txn(Some(food.id), dec!(-200), date(2025, 1, 20));
        let original_id = original_txn.id;

        // Reimbursement linked to the original (positive: money coming back)
        let mut reimbursement = make_txn(Some(food.id), dec!(200), date(2025, 1, 25));
        reimbursement.correlation_id = Some(original_id);
        reimbursement.correlation_type = Some(crate::models::CorrelationType::Reimbursement);

        let transactions = vec![
            make_txn(Some(food.id), dec!(-50), date(2025, 1, 20)),
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

        let transactions = vec![make_txn(Some(food.id), dec!(-100), date(2025, 1, 20))];

        let today = date(2025, 1, 25);
        let status =
            compute_budget_status(&food, &transactions, &bm, &all_months, &categories, today);

        assert_eq!(status.spent, dec!(100));
        assert_eq!(status.remaining, dec!(400));
        assert_eq!(status.budget_amount, dec!(500));
        assert_eq!(status.pace, PaceIndicator::UnderBudget);
        assert!(status.time_left > 0);
        assert_eq!(status.budget_mode, BudgetMode::Monthly);
        assert_eq!(status.rollover, Decimal::ZERO);
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

        let transactions = vec![make_txn(Some(food.id), dec!(-250), date(2025, 1, 20))];

        let today = date(2025, 1, 25);
        let status =
            compute_budget_status(&food, &transactions, &bm, &all_months, &categories, today);

        assert_eq!(status.spent, dec!(250));
        assert_eq!(status.remaining, dec!(-50));
        assert_eq!(status.pace, PaceIndicator::OverBudget);
        assert_eq!(status.budget_mode, BudgetMode::Monthly);
    }

    #[test]
    fn rollover_accumulates_surplus() {
        let food = food_with_budget(BudgetMode::Monthly, dec!(500));
        let categories = vec![food.clone()];

        let bm1 = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 1, 15),
            end_date: Some(date(2025, 2, 13)),
            salary_transactions_detected: 1,
        };

        let bm2 = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 2, 14),
            end_date: Some(date(2025, 3, 14)),
            salary_transactions_detected: 1,
        };

        let transactions = vec![
            // Month 1: spent 300, surplus 200
            make_txn(Some(food.id), dec!(-300), date(2025, 1, 20)),
            // Month 2: spent 400, surplus 100
            make_txn(Some(food.id), dec!(-400), date(2025, 2, 20)),
        ];

        let rollover = compute_rollover(&food, &[bm1, bm2], &transactions, &categories);

        // Month 1: 500 - 300 = 200 surplus
        // Month 2: 500 - 400 = 100 surplus
        // Total rollover: 300
        assert_eq!(rollover, dec!(300));
    }

    #[test]
    fn rollover_accumulates_deficit() {
        let food = food_with_budget(BudgetMode::Monthly, dec!(500));
        let categories = vec![food.clone()];

        let bm1 = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 1, 15),
            end_date: Some(date(2025, 2, 13)),
            salary_transactions_detected: 1,
        };

        let transactions = vec![make_txn(Some(food.id), dec!(-700), date(2025, 1, 20))];

        let rollover = compute_rollover(&food, &[bm1], &transactions, &categories);

        // 500 - 700 = -200 deficit carried forward
        assert_eq!(rollover, dec!(-200));
    }

    #[test]
    fn rollover_zero_for_annual() {
        let food = food_with_budget(BudgetMode::Annual, dec!(6000));
        let categories = vec![food.clone()];

        let bm1 = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 1, 15),
            end_date: Some(date(2025, 2, 13)),
            salary_transactions_detected: 1,
        };

        let transactions = vec![make_txn(Some(food.id), dec!(-300), date(2025, 1, 20))];

        let rollover = compute_rollover(&food, &[bm1], &transactions, &categories);

        // Annual budgets don't roll over
        assert_eq!(rollover, Decimal::ZERO);
    }

    #[test]
    fn open_budget_month_not_included_in_rollover() {
        let food = food_with_budget(BudgetMode::Monthly, dec!(500));
        let categories = vec![food.clone()];

        let bm_closed = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 1, 15),
            end_date: Some(date(2025, 2, 13)),
            salary_transactions_detected: 1,
        };

        let bm_open = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 2, 14),
            end_date: None, // Still open
            salary_transactions_detected: 1,
        };

        let transactions = vec![
            make_txn(Some(food.id), dec!(-300), date(2025, 1, 20)),
            make_txn(Some(food.id), dec!(-100), date(2025, 2, 20)),
        ];

        let rollover = compute_rollover(&food, &[bm_closed, bm_open], &transactions, &categories);

        // Only closed month counts: 500 - 300 = 200
        assert_eq!(rollover, dec!(200));
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
        // 14 months starting from November (no January anchor found before it)
        let months: Vec<BudgetMonth> = (0..14)
            .map(|i| {
                let y = 2024 + (10 + i) / 12;
                let m = ((10 + i) % 12) + 1;
                BudgetMonth {
                    id: BudgetMonthId::new(),
                    start_date: date(y as i32, m, 15),
                    end_date: Some(date(y as i32, m, 28)),
                    salary_transactions_detected: 1,
                }
            })
            .collect();

        // Reference = month index 5 (March 2025 — first January is at index 2)
        let year = budget_year_months(&months, &months[5]);
        // Should start at index 2 (Jan 2025) and take up to 12 months
        assert_eq!(year[0].start_date.month(), 1);
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
            make_txn(Some(food.id), dec!(-400), date(2025, 1, 20)),
            make_txn(Some(food.id), dec!(-600), date(2025, 2, 20)),
            make_txn(Some(food.id), dec!(-200), date(2025, 3, 20)),
        ];

        let today = date(2025, 3, 25);
        let status = compute_budget_status(
            &food,
            &transactions,
            &bm_mar,
            &all_months,
            &categories,
            today,
        );

        assert_eq!(status.budget_mode, BudgetMode::Annual);
        // 400 + 600 + 200 = 1200 across three months
        assert_eq!(status.spent, dec!(1200));
        assert_eq!(status.remaining, dec!(4800));
        assert_eq!(status.rollover, Decimal::ZERO);
        // 3 months total, reference is month 3 → 0 months left
        assert_eq!(status.time_left, 0);
    }

    #[test]
    fn project_status_filters_by_date_range() {
        let mut project = make_category(300, "Renovation", None);
        project.budget_mode = Some(BudgetMode::Project);
        project.budget_amount = Some(dec!(10000));
        project.project_start_date = Some(date(2025, 1, 1));
        project.project_end_date = Some(date(2025, 6, 30));
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
            make_txn(Some(project.id), dec!(-2000), date(2025, 2, 15)),
            make_txn(Some(project.id), dec!(-3000), date(2025, 4, 10)),
            // Outside project range — excluded
            make_txn(Some(project.id), dec!(-500), date(2024, 12, 20)),
            make_txn(Some(project.id), dec!(-500), date(2025, 7, 1)),
        ];

        let today = date(2025, 4, 15);
        let status = compute_budget_status(
            &project,
            &transactions,
            &bm,
            &all_months,
            &categories,
            today,
        );

        assert_eq!(status.budget_mode, BudgetMode::Project);
        assert_eq!(status.spent, dec!(5000));
        assert_eq!(status.remaining, dec!(5000));
        // Days left: June 30 - April 15 = 76 days
        assert_eq!(status.time_left, 76);
    }

    #[test]
    fn project_status_open_ended() {
        let mut project = make_category(301, "Ongoing", None);
        project.budget_mode = Some(BudgetMode::Project);
        project.budget_amount = Some(dec!(5000));
        project.project_start_date = Some(date(2025, 1, 1));
        // No end date
        let categories = vec![project.clone()];

        let bm = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 3, 15),
            end_date: None,
            salary_transactions_detected: 1,
        };
        let all_months = [bm.clone()];

        let transactions = vec![make_txn(Some(project.id), dec!(-1000), date(2025, 2, 1))];

        let today = date(2025, 4, 1);
        let status = compute_budget_status(
            &project,
            &transactions,
            &bm,
            &all_months,
            &categories,
            today,
        );

        assert_eq!(status.budget_mode, BudgetMode::Project);
        assert_eq!(status.spent, dec!(1000));
        assert_eq!(status.time_left, -1);
        assert_eq!(status.pace, PaceIndicator::OnTrack);
    }

    #[test]
    fn monthly_status_includes_rollover() {
        let food = food_with_budget(BudgetMode::Monthly, dec!(500));
        let categories = vec![food.clone()];

        let bm1 = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 1, 15),
            end_date: Some(date(2025, 2, 13)),
            salary_transactions_detected: 1,
        };
        let bm2 = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 2, 14),
            end_date: None,
            salary_transactions_detected: 1,
        };
        let all_months = [bm1.clone(), bm2.clone()];

        let transactions = vec![
            // Month 1: spent 300, surplus 200
            make_txn(Some(food.id), dec!(-300), date(2025, 1, 20)),
            // Month 2 (current): spent 100
            make_txn(Some(food.id), dec!(-100), date(2025, 2, 20)),
        ];

        let today = date(2025, 2, 25);
        let status =
            compute_budget_status(&food, &transactions, &bm2, &all_months, &categories, today);

        assert_eq!(status.budget_mode, BudgetMode::Monthly);
        assert_eq!(status.rollover, dec!(200));
        assert_eq!(status.spent, dec!(100));
        // remaining = budget(500) + rollover(200) - spent(100) = 600
        assert_eq!(status.remaining, dec!(600));
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
            make_txn(Some(food.id), dec!(-80), date(2025, 1, 20)),
            make_txn(Some(food.id), dec!(-45), date(2025, 1, 22)),
            // Refund credited back (positive amount from the bank)
            make_txn(Some(food.id), dec!(25), date(2025, 1, 24)),
        ];

        let spending = compute_category_spending(&transactions, food.id, &bm, &categories);
        // Net outflow: -80 + -45 + 25 = -100, negated → 100
        assert_eq!(spending, dec!(100));
    }

    #[test]
    fn mixed_monthly_and_annual_categories() {
        let food = food_with_budget(BudgetMode::Monthly, dec!(500));
        let mut insurance = make_category(200, "Insurance", None);
        insurance.budget_mode = Some(BudgetMode::Annual);
        insurance.budget_amount = Some(dec!(2400));
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
            make_txn(Some(food.id), dec!(-300), date(2025, 1, 20)),
            make_txn(Some(food.id), dec!(-200), date(2025, 2, 20)),
            // Insurance in both months
            make_txn(Some(insurance.id), dec!(-100), date(2025, 1, 25)),
            make_txn(Some(insurance.id), dec!(-150), date(2025, 2, 18)),
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
        );
        assert_eq!(food_status.budget_mode, BudgetMode::Monthly);
        assert_eq!(food_status.spent, dec!(200));
        // Rollover from Jan: 500 - 300 = 200
        assert_eq!(food_status.rollover, dec!(200));
        // remaining = 500 + 200 - 200 = 500
        assert_eq!(food_status.remaining, dec!(500));

        // Annual insurance status (sums all months in the year)
        let ins_status = compute_budget_status(
            &insurance,
            &transactions,
            &bm_feb,
            &all_months,
            &categories,
            today,
        );
        assert_eq!(ins_status.budget_mode, BudgetMode::Annual);
        // 100 + 150 = 250 across two months
        assert_eq!(ins_status.spent, dec!(250));
        assert_eq!(ins_status.remaining, dec!(2150));
        assert_eq!(ins_status.rollover, Decimal::ZERO);
    }

    #[test]
    fn annual_transactions_do_not_affect_monthly_budget() {
        let food = food_with_budget(BudgetMode::Monthly, dec!(500));
        let mut insurance = make_category(200, "Insurance", None);
        insurance.budget_mode = Some(BudgetMode::Annual);
        insurance.budget_amount = Some(dec!(2400));
        let categories = vec![food.clone(), insurance.clone()];

        let bm = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 1, 15),
            end_date: Some(date(2025, 2, 13)),
            salary_transactions_detected: 1,
        };
        let all_months = [bm.clone()];

        let transactions = vec![
            make_txn(Some(food.id), dec!(-120), date(2025, 1, 20)),
            // Large annual insurance payment in the same month
            make_txn(Some(insurance.id), dec!(-1200), date(2025, 1, 18)),
        ];

        let today = date(2025, 1, 25);

        // Monthly food spending must only reflect food transactions
        let food_spending = compute_category_spending(&transactions, food.id, &bm, &categories);
        assert_eq!(food_spending, dec!(120));

        let food_status =
            compute_budget_status(&food, &transactions, &bm, &all_months, &categories, today);
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
        );
        assert_eq!(ins_status.spent, dec!(1200));
        assert_eq!(ins_status.remaining, dec!(1200));
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
        let status =
            compute_budget_status(&food, &transactions, &bm, &all_months, &categories, today);

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
            make_txn(Some(food.id), dec!(-100), date(2025, 1, 20)),
            make_txn(Some(food.id), dec!(300), date(2025, 1, 22)),
        ];

        let today = date(2025, 1, 25);
        let status =
            compute_budget_status(&food, &transactions, &bm, &all_months, &categories, today);

        // Net: -100 + 300 = +200, negated → -200 (negative spending = net income)
        assert_eq!(status.spent, dec!(-200));
        // remaining = 500 - (-200) = 700
        assert_eq!(status.remaining, dec!(700));
    }

    #[test]
    fn project_with_mixed_expenses_and_refunds() {
        let mut project = make_category(300, "Renovation", None);
        project.budget_mode = Some(BudgetMode::Project);
        project.budget_amount = Some(dec!(10000));
        project.project_start_date = Some(date(2025, 1, 1));
        project.project_end_date = Some(date(2025, 6, 30));
        let categories = vec![project.clone()];

        let bm = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 3, 15),
            end_date: None,
            salary_transactions_detected: 1,
        };
        let all_months = [bm.clone()];

        let transactions = vec![
            make_txn(Some(project.id), dec!(-5000), date(2025, 2, 15)),
            make_txn(Some(project.id), dec!(-3000), date(2025, 4, 10)),
            // Partial refund on materials
            make_txn(Some(project.id), dec!(800), date(2025, 4, 20)),
        ];

        let today = date(2025, 4, 25);
        let status = compute_budget_status(
            &project,
            &transactions,
            &bm,
            &all_months,
            &categories,
            today,
        );

        assert_eq!(status.budget_mode, BudgetMode::Project);
        // Net: -5000 + -3000 + 800 = -7200, negated → 7200
        assert_eq!(status.spent, dec!(7200));
        assert_eq!(status.remaining, dec!(2800));
    }

    #[test]
    fn rollover_with_mixed_expenses_and_refunds() {
        let food = food_with_budget(BudgetMode::Monthly, dec!(500));
        let categories = vec![food.clone()];

        let bm1 = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 1, 15),
            end_date: Some(date(2025, 2, 13)),
            salary_transactions_detected: 1,
        };

        let bm2 = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: date(2025, 2, 14),
            end_date: None,
            salary_transactions_detected: 1,
        };
        let all_months = [bm1.clone(), bm2.clone()];

        let transactions = vec![
            // Month 1: -400 expense + 50 refund = -350 net → 350 spent → 150 surplus
            make_txn(Some(food.id), dec!(-400), date(2025, 1, 20)),
            make_txn(Some(food.id), dec!(50), date(2025, 1, 28)),
            // Month 2 (current): -200 expense
            make_txn(Some(food.id), dec!(-200), date(2025, 2, 20)),
        ];

        let today = date(2025, 2, 25);
        let status =
            compute_budget_status(&food, &transactions, &bm2, &all_months, &categories, today);

        assert_eq!(status.spent, dec!(200));
        // Rollover: 500 - 350 = 150
        assert_eq!(status.rollover, dec!(150));
        // remaining = 500 + 150 - 200 = 450
        assert_eq!(status.remaining, dec!(450));
    }
}
