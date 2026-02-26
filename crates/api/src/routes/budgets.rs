use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use budget_core::budget::compute_budget_status;
use budget_core::models::{BudgetMode, BudgetMonth, BudgetStatus};

use crate::routes::AppError;
use crate::state::AppState;

#[derive(Deserialize)]
struct StatusQuery {
    month_id: Option<Uuid>,
}

#[derive(Serialize)]
struct StatusResponse {
    month: BudgetMonth,
    statuses: Vec<BudgetStatus>,
}

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

/// Compute budget status for every budgeted category in a given month.
///
/// If `month_id` is provided, looks up that specific month; otherwise finds the
/// current open month (the one with `end_date IS NULL`). For historical months,
/// pace is calculated against the month's end date rather than today.
///
/// # Errors
///
/// Returns 404 if the requested budget month does not exist.
/// Returns `AppError` if any database query fails.
async fn status(
    State(state): State<AppState>,
    Query(query): Query<StatusQuery>,
) -> Result<Json<StatusResponse>, AppError> {
    let transactions = state.db.list_transactions().await?;
    let categories = state.db.list_categories().await?;
    let budget_months = state.db.list_budget_months().await?;

    let month = if let Some(id) = query.month_id {
        budget_months
            .iter()
            .find(|bm| *bm.id.as_uuid() == id)
            .ok_or_else(|| AppError(StatusCode::NOT_FOUND, "budget month not found".to_owned()))?
    } else {
        budget_months
            .iter()
            .find(|bm| bm.end_date.is_none())
            .ok_or_else(|| AppError(StatusCode::NOT_FOUND, "no current budget month".to_owned()))?
    };

    // For historical months use end_date as the reference; for the current month use today
    let reference_date = month.end_date.unwrap_or_else(|| Utc::now().date_naive());

    let statuses: Vec<BudgetStatus> = categories
        .iter()
        .filter(|c| {
            matches!(
                c.budget_mode,
                Some(BudgetMode::Monthly | BudgetMode::Annual)
            )
        })
        .map(|cat| compute_budget_status(cat, &transactions, month, &categories, reference_date))
        .collect();

    Ok(Json(StatusResponse {
        month: month.clone(),
        statuses,
    }))
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
