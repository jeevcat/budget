//! Budget recompute job handler: detects salary-anchored budget month
//! boundaries and assigns every transaction to the appropriate month.

use apalis::prelude::*;
use chrono::NaiveDate;
use sqlx::SqlitePool;

use budget_core::budget::detect_budget_month_boundaries;
use budget_core::models::{BudgetMonth, BudgetMonthId};
use budget_core::{db, load_config};

use super::BudgetRecomputeJob;

/// Find the budget month a transaction date belongs to, if any.
///
/// Iterates budget months and returns the first whose date range contains
/// `date`. Returns `None` when the date falls outside all known months.
fn find_budget_month_for_date(date: NaiveDate, months: &[BudgetMonth]) -> Option<BudgetMonthId> {
    for month in months {
        if date < month.start_date {
            continue;
        }
        match month.end_date {
            Some(end) if date <= end => return Some(month.id),
            None => return Some(month.id), // open-ended current month
            _ => {}
        }
    }
    None
}

/// Recompute budget month boundaries from salary transactions, replace them
/// in the database, and assign each transaction to its budget month.
///
/// This is the shared implementation used by both the standalone recompute
/// handler and the pipeline step. Purely functional over the current
/// transaction set: re-derives all budget months from scratch so that
/// late-arriving or recategorized transactions are handled correctly.
///
/// # Errors
///
/// Returns an error if:
/// - The application config cannot be loaded.
/// - The "Salary" category does not exist.
/// - Budget month detection fails (e.g. no salary category configured).
/// - Any database read or write fails.
pub(crate) async fn recompute_budgets(pool: &SqlitePool) -> Result<(), BoxDynError> {
    let config = load_config().map_err(|e| format!("config error: {e}"))?;

    let transactions = db::list_transactions(pool).await?;
    let categories = db::list_categories(pool).await?;

    // Resolve the salary category by well-known name
    let salary_category = db::get_category_by_name(pool, "Salary").await?;
    let salary_category_id = salary_category.map(|c| c.id);

    let budget_months = detect_budget_month_boundaries(
        &transactions,
        config.expected_salary_count,
        salary_category_id,
        &categories,
    )?;

    // Atomically replace all budget months
    db::replace_budget_months(pool, &budget_months).await?;

    // Assign each transaction to its budget month based on posted_date
    let mut assigned: u32 = 0;
    let mut unassigned: u32 = 0;

    for txn in &transactions {
        if let Some(month_id) = find_budget_month_for_date(txn.posted_date, &budget_months) {
            db::update_transaction_budget_month(pool, txn.id, month_id).await?;
            assigned += 1;
        } else {
            unassigned += 1;
        }
    }

    tracing::info!(
        budget_months = budget_months.len(),
        transactions_assigned = assigned,
        transactions_unassigned = unassigned,
        "budget recompute job completed"
    );

    Ok(())
}

/// Apalis handler that delegates to [`recompute_budgets`].
///
/// # Errors
///
/// Returns an error if budget recomputation fails.
pub async fn handle_recompute_job(
    _job: BudgetRecomputeJob,
    pool: Data<SqlitePool>,
) -> Result<(), BoxDynError> {
    recompute_budgets(&pool).await
}
