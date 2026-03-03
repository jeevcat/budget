use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use budget_core::budget::{
    budget_year_months, collect_category_subtree, compute_budget_status,
    compute_project_child_breakdowns, detect_budget_month_boundaries, filter_for_budget,
    filter_for_project, is_in_budget_month,
};
use budget_core::models::{
    BudgetMode, BudgetMonth, BudgetStatus, Category, CategoryId, ProjectChildSpending, Transaction,
};

use crate::routes::AppError;
use crate::state::AppState;

#[derive(Deserialize)]
struct StatusQuery {
    month_id: Option<Uuid>,
}

#[derive(Serialize)]
struct ChildCategoryInfo {
    category_id: CategoryId,
    category_name: String,
}

#[derive(Serialize)]
struct StatusEntry {
    #[serde(flatten)]
    status: BudgetStatus,
    children: Vec<ChildCategoryInfo>,
    has_children: bool,
}

#[derive(Serialize)]
struct ProjectStatusEntry {
    #[serde(flatten)]
    status: BudgetStatus,
    children: Vec<ProjectChildSpending>,
    has_children: bool,
}

#[derive(Serialize)]
struct StatusResponse {
    month: BudgetMonth,
    statuses: Vec<StatusEntry>,
    projects: Vec<ProjectStatusEntry>,
    /// Transactions contributing to monthly budgets in the active month.
    monthly_transactions: Vec<Transaction>,
    /// Transactions contributing to annual budgets across the budget year.
    annual_transactions: Vec<Transaction>,
    /// Transactions contributing to project budgets.
    project_transactions: Vec<Transaction>,
    /// Number of uncategorized, uncorrelated transactions in the active month.
    uncategorized_count: u32,
    /// The budget year (calendar year of the January-anchored start).
    budget_year: i32,
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
    categories: &[Category],
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

/// Effective budget mode for a category: its own mode, or inherited from parent.
fn effective_budget_mode(cat: &Category, categories: &[Category]) -> Option<BudgetMode> {
    if let Some(mode) = cat.budget_mode {
        return Some(mode);
    }
    let parent = cat
        .parent_id
        .and_then(|pid| categories.iter().find(|c| c.id == pid));
    parent.and_then(|p| p.budget_mode)
}

/// Pre-classified transactions and auxiliary dashboard data.
struct ClassifiedTransactions {
    monthly: Vec<Transaction>,
    annual: Vec<Transaction>,
    project: Vec<Transaction>,
    uncategorized_count: u32,
    budget_year: i32,
}

/// Classify transactions by budget mode and compute auxiliary dashboard data.
///
/// Returns the actual transaction objects grouped by mode so the frontend
/// can display them directly without any business logic.
fn classify_transactions(
    transactions: &[Transaction],
    categories: &[Category],
    month: &BudgetMonth,
    year_months: &[&BudgetMonth],
) -> ClassifiedTransactions {
    let budget_txns = filter_for_budget(transactions, categories);

    let monthly_cat_ids: std::collections::HashSet<CategoryId> = categories
        .iter()
        .filter(|c| effective_budget_mode(c, categories) == Some(BudgetMode::Monthly))
        .map(|c| c.id)
        .collect();

    let annual_cat_ids: std::collections::HashSet<CategoryId> = categories
        .iter()
        .filter(|c| effective_budget_mode(c, categories) == Some(BudgetMode::Annual))
        .map(|c| c.id)
        .collect();

    let monthly: Vec<Transaction> = budget_txns
        .iter()
        .filter(|t| {
            t.category_id
                .is_some_and(|cid| monthly_cat_ids.contains(&cid))
                && is_in_budget_month(t.posted_date, month)
        })
        .map(|t| (*t).clone())
        .collect();

    let year_start = year_months.first().map(|bm| bm.start_date);
    let year_end = year_months.last().and_then(|bm| bm.end_date);
    let annual: Vec<Transaction> = budget_txns
        .iter()
        .filter(|t| {
            t.category_id
                .is_some_and(|cid| annual_cat_ids.contains(&cid))
                && year_start.is_none_or(|ys| t.posted_date >= ys)
                && year_end.is_none_or(|ye| t.posted_date <= ye)
        })
        .map(|t| (*t).clone())
        .collect();

    let project: Vec<Transaction> = filter_for_project(transactions, categories)
        .into_iter()
        .cloned()
        .collect();

    let uncategorized_count: u32 = transactions
        .iter()
        .filter(|t| {
            is_in_budget_month(t.posted_date, month)
                && t.category_id.is_none()
                && t.correlation_type.is_none()
        })
        .count()
        .try_into()
        .unwrap_or(u32::MAX);

    let budget_year = year_months.first().map_or_else(
        || chrono::Datelike::year(&month.start_date),
        |bm| chrono::Datelike::year(&bm.start_date),
    );

    ClassifiedTransactions {
        monthly,
        annual,
        project,
        uncategorized_count,
        budget_year,
    }
}

/// Compute project statuses with per-child spending breakdowns.
fn compute_projects(
    categories: &[Category],
    transactions: &[Transaction],
    month: &BudgetMonth,
    budget_months: &[BudgetMonth],
    reference_date: chrono::NaiveDate,
) -> Vec<ProjectStatusEntry> {
    let project_txns = filter_for_project(transactions, categories);

    categories
        .iter()
        .filter(|c| c.budget_mode == Some(BudgetMode::Project))
        .map(|cat| {
            let status = compute_budget_status(
                cat,
                transactions,
                month,
                budget_months,
                categories,
                reference_date,
            );
            let has_children = categories.iter().any(|c| c.parent_id == Some(cat.id));
            let children = if has_children {
                let subtree_ids: std::collections::HashSet<CategoryId> =
                    collect_category_subtree(cat.id, categories)
                        .into_iter()
                        .collect();
                let subtree_txns: Vec<&Transaction> = project_txns
                    .iter()
                    .filter(|t| t.category_id.is_some_and(|cid| subtree_ids.contains(&cid)))
                    .copied()
                    .collect();
                compute_project_child_breakdowns(cat, &subtree_txns, categories)
            } else {
                Vec::new()
            };
            ProjectStatusEntry {
                status,
                children,
                has_children,
            }
        })
        .collect()
}

/// Compute budget status for every budgeted category in a given month.
///
/// Returns all data needed to render the dashboard: budget statuses, pre-filtered
/// transactions grouped by mode, uncategorized count, budget year, and project
/// child breakdowns. The frontend is a pure display layer — no business logic.
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

    let year_months = budget_year_months(&budget_months, month);
    let mut earliest = month.start_date;
    if let Some(first) = year_months.first() {
        earliest = earliest.min(first.start_date);
    }
    for cat in &categories {
        if cat.budget_mode == Some(BudgetMode::Project)
            && let Some(start) = cat.project_start_date
        {
            earliest = earliest.min(start);
        }
    }

    let transactions = state.db.list_transactions_since(earliest).await?;
    let reference_date = month.end_date.unwrap_or_else(|| Utc::now().date_naive());

    let statuses: Vec<StatusEntry> = categories
        .iter()
        .filter(|c| {
            matches!(
                c.budget_mode,
                Some(BudgetMode::Monthly | BudgetMode::Annual)
            )
        })
        .map(|cat| {
            let status = compute_budget_status(
                cat,
                &transactions,
                month,
                &budget_months,
                &categories,
                reference_date,
            );
            let direct_children: Vec<ChildCategoryInfo> = categories
                .iter()
                .filter(|c| c.parent_id == Some(cat.id))
                .map(|c| ChildCategoryInfo {
                    category_id: c.id,
                    category_name: c.name.to_string(),
                })
                .collect();
            let has_children = !direct_children.is_empty();
            StatusEntry {
                status,
                children: direct_children,
                has_children,
            }
        })
        .collect();

    let classified = classify_transactions(&transactions, &categories, month, &year_months);

    let projects = compute_projects(
        &categories,
        &transactions,
        month,
        &budget_months,
        reference_date,
    );

    Ok(Json(StatusResponse {
        month: month.clone(),
        statuses,
        projects,
        monthly_transactions: classified.monthly,
        annual_transactions: classified.annual,
        project_transactions: classified.project,
        uncategorized_count: classified.uncategorized_count,
        budget_year: classified.budget_year,
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
