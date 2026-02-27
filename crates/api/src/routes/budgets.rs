use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use budget_core::budget::{
    budget_year_months, collect_category_subtree, compute_budget_status,
    detect_budget_month_boundaries,
};
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
    projects: Vec<BudgetStatus>,
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

/// Derive budget months by fetching only salary-category transactions.
///
/// Returns an empty list when no Salary category exists (instead of erroring),
/// since the user may not have set one up yet.
async fn derive_months(
    state: &AppState,
    categories: &[budget_core::models::Category],
) -> Result<Vec<BudgetMonth>, AppError> {
    let salary_cat_id = state.db.get_category_by_name("Salary").await?.map(|c| c.id);

    let salary_txns = match salary_cat_id {
        Some(sid) => {
            let subtree = collect_category_subtree(sid, categories);
            state.db.list_transactions_by_category_ids(&subtree).await?
        }
        None => Vec::new(),
    };

    match detect_budget_month_boundaries(
        &salary_txns,
        state.expected_salary_count,
        salary_cat_id,
        categories,
    ) {
        Ok(mut months) => {
            months.sort_by_key(|bm| bm.start_date);
            Ok(months)
        }
        Err(budget_core::error::Error::NoSalaryCategory) => Ok(Vec::new()),
        Err(e) => Err(AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
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
    let categories = state.db.list_categories().await?;

    // Phase 1: Detect budget month boundaries using only salary transactions
    let mut budget_months = derive_months(&state, &categories).await?;
    budget_months.sort_by_key(|bm| bm.start_date);

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

    // Phase 2: Determine the earliest date needed and fetch only that range.
    // Budget year months cover rollover (monthly) and annual spending.
    let year_months = budget_year_months(&budget_months, month);
    let mut earliest = month.start_date;
    if let Some(first) = year_months.first() {
        earliest = earliest.min(first.start_date);
    }
    // Include project date ranges so project spending is accurate
    for cat in &categories {
        if cat.budget_mode == Some(BudgetMode::Project)
            && let Some(start) = cat.project_start_date
        {
            earliest = earliest.min(start);
        }
    }

    let transactions = state.db.list_transactions_since(earliest).await?;

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
        .map(|cat| {
            compute_budget_status(
                cat,
                &transactions,
                month,
                &budget_months,
                &categories,
                reference_date,
            )
        })
        .collect();

    let projects: Vec<BudgetStatus> = categories
        .iter()
        .filter(|c| c.budget_mode == Some(BudgetMode::Project))
        .map(|cat| {
            compute_budget_status(
                cat,
                &transactions,
                month,
                &budget_months,
                &categories,
                reference_date,
            )
        })
        .collect();

    Ok(Json(StatusResponse {
        month: month.clone(),
        statuses,
        projects,
    }))
}

/// List all budget months, derived on the fly from transactions.
///
/// # Errors
///
/// Returns `AppError` if the database query fails.
async fn list_months(State(state): State<AppState>) -> Result<Json<Vec<BudgetMonth>>, AppError> {
    let categories = state.db.list_categories().await?;
    let months = derive_months(&state, &categories).await?;
    Ok(Json(months))
}
