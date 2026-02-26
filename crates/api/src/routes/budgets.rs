use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use chrono::Utc;

use budget_core::budget::compute_budget_status;
use budget_core::models::{BudgetMode, BudgetMonth, BudgetStatus};

use crate::routes::AppError;
use crate::state::AppState;

/// Build the budgets sub-router.
///
/// Mounts:
/// - `GET /status` -- compute budget status for the current month
/// - `GET /months` -- list all budget months
///
/// # Errors
///
/// Individual handlers return `AppError` on failure.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/status", get(status))
        .route("/months", get(list_months))
}

/// Compute budget status for every budgeted category in the current month.
///
/// Finds the most recent budget month with no end date and evaluates each
/// category that has a `budget_mode` of Monthly or Annual against the
/// transactions in that month.
///
/// # Errors
///
/// Returns 404 if no current budget month exists.
/// Returns `AppError` if any database query fails.
async fn status(State(state): State<AppState>) -> Result<Json<Vec<BudgetStatus>>, AppError> {
    let transactions = state.db.list_transactions().await?;
    let categories = state.db.list_categories().await?;
    let budget_months = state.db.list_budget_months().await?;

    // The current month is the one with no end date
    let current_month = budget_months
        .iter()
        .find(|bm| bm.end_date.is_none())
        .ok_or_else(|| AppError(StatusCode::NOT_FOUND, "no current budget month".to_owned()))?;

    let today = Utc::now().date_naive();

    let statuses: Vec<BudgetStatus> = categories
        .iter()
        .filter(|c| {
            matches!(
                c.budget_mode,
                Some(BudgetMode::Monthly | BudgetMode::Annual)
            )
        })
        .map(|cat| compute_budget_status(cat, &transactions, current_month, &categories, today))
        .collect();

    Ok(Json(statuses))
}

/// List all budget months.
///
/// # Errors
///
/// Returns `AppError` if the database query fails.
async fn list_months(State(state): State<AppState>) -> Result<Json<Vec<BudgetMonth>>, AppError> {
    let months = state.db.list_budget_months().await?;
    Ok(Json(months))
}
