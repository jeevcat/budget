use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use rust_decimal::Decimal;
use serde::Deserialize;
use uuid::Uuid;

use budget_core::budget::compute_budget_status;
use budget_core::db;
use budget_core::models::{
    BudgetMonth, BudgetPeriod, BudgetPeriodId, BudgetStatus, CategoryId, PeriodType,
};

use crate::routes::AppError;
use crate::state::AppState;

/// Request body for creating or updating a budget period.
#[derive(Deserialize)]
pub struct CreateBudgetPeriod {
    /// Category UUID this budget applies to.
    pub category_id: String,
    /// Period type: "monthly" or "annual".
    pub period_type: String,
    /// Budget amount as a decimal string (e.g. "500.00").
    pub amount: String,
}

/// Build the budgets sub-router.
///
/// Mounts:
/// - `GET /status` -- compute budget status for the current month
/// - `POST /periods` -- create a new budget period
/// - `PUT /periods/{id}` -- update a budget period
/// - `DELETE /periods/{id}` -- delete a budget period
/// - `GET /months` -- list all budget months
///
/// # Errors
///
/// Individual handlers return `AppError` on failure.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/status", get(status))
        .route("/periods", post(create_period))
        .route(
            "/periods/{id}",
            axum::routing::put(update_period).delete(delete_period),
        )
        .route("/months", get(list_months))
}

/// Compute budget status for every budget period in the current (open-ended) month.
///
/// Finds the most recent budget month with no end date and evaluates each
/// budget period against the transactions in that month.
///
/// # Errors
///
/// Returns 404 if no current budget month exists.
/// Returns `AppError` if any database query fails.
async fn status(State(state): State<AppState>) -> Result<Json<Vec<BudgetStatus>>, AppError> {
    let transactions = db::list_transactions(&state.pool).await?;
    let categories = db::list_categories(&state.pool).await?;
    let budget_periods = db::list_budget_periods(&state.pool).await?;
    let budget_months = db::list_budget_months(&state.pool).await?;

    // The current month is the one with no end date
    let current_month = budget_months
        .iter()
        .find(|bm| bm.end_date.is_none())
        .ok_or_else(|| AppError(StatusCode::NOT_FOUND, "no current budget month".to_owned()))?;

    let today = Utc::now().date_naive();

    let statuses: Vec<BudgetStatus> = budget_periods
        .iter()
        .map(|bp| compute_budget_status(bp, &transactions, current_month, &categories, today))
        .collect();

    Ok(Json(statuses))
}

/// Parse a `CreateBudgetPeriod` body into a domain `BudgetPeriod`.
fn parse_budget_period_body(
    body: &CreateBudgetPeriod,
    id: BudgetPeriodId,
) -> Result<BudgetPeriod, AppError> {
    let category_uuid = Uuid::parse_str(&body.category_id)
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, e.to_string()))?;

    let period_type: PeriodType = body
        .period_type
        .parse()
        .map_err(|e: budget_core::error::Error| AppError(StatusCode::BAD_REQUEST, e.to_string()))?;

    let amount: Decimal = body
        .amount
        .parse()
        .map_err(|e: rust_decimal::Error| AppError(StatusCode::BAD_REQUEST, e.to_string()))?;

    Ok(BudgetPeriod {
        id,
        category_id: CategoryId::from_uuid(category_uuid),
        period_type,
        amount,
    })
}

/// Create a new budget period.
///
/// # Errors
///
/// Returns 400 if any field value is invalid.
/// Returns `AppError` if the database insert fails.
async fn create_period(
    State(state): State<AppState>,
    Json(body): Json<CreateBudgetPeriod>,
) -> Result<(StatusCode, Json<BudgetPeriod>), AppError> {
    let bp = parse_budget_period_body(&body, BudgetPeriodId::new())?;
    db::insert_budget_period(&state.pool, &bp).await?;
    Ok((StatusCode::CREATED, Json(bp)))
}

/// Update an existing budget period by ID.
///
/// # Errors
///
/// Returns 400 if the ID or any field value is invalid.
/// Returns `AppError` if the database update fails.
async fn update_period(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<CreateBudgetPeriod>,
) -> Result<Json<BudgetPeriod>, AppError> {
    let uuid =
        Uuid::parse_str(&id).map_err(|e| AppError(StatusCode::BAD_REQUEST, e.to_string()))?;
    let bp = parse_budget_period_body(&body, BudgetPeriodId::from_uuid(uuid))?;
    db::update_budget_period(&state.pool, &bp).await?;
    Ok(Json(bp))
}

/// Delete a budget period by its UUID path parameter.
///
/// # Errors
///
/// Returns 400 if the ID is not a valid UUID.
/// Returns `AppError` if the database delete fails.
async fn delete_period(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let uuid =
        Uuid::parse_str(&id).map_err(|e| AppError(StatusCode::BAD_REQUEST, e.to_string()))?;
    db::delete_budget_period(&state.pool, BudgetPeriodId::from_uuid(uuid)).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// List all budget months.
///
/// # Errors
///
/// Returns `AppError` if the database query fails.
async fn list_months(State(state): State<AppState>) -> Result<Json<Vec<BudgetMonth>>, AppError> {
    let months = db::list_budget_months(&state.pool).await?;
    Ok(Json(months))
}
