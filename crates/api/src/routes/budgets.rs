use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use chrono::Utc;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use budget_core::budget::{
    budget_year_months, collect_category_subtree, compute_budget_status,
    compute_project_child_breakdowns, detect_budget_month_boundaries, effective_budget_mode,
    filter_for_budget, filter_for_project, is_in_budget_month,
};
use budget_core::models::{
    BudgetConfig, BudgetMode, BudgetMonth, BudgetStatus, Category, CategoryId, PaceIndicator,
    ProjectChildSpending, Transaction,
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
struct BudgetGroupSummary {
    total_budget: Decimal,
    total_spent: Decimal,
    remaining: Decimal,
    over_budget_count: usize,
    bar_max: Decimal,
}

#[derive(Serialize)]
struct StatusResponse {
    month: BudgetMonth,
    statuses: Vec<StatusEntry>,
    projects: Vec<ProjectStatusEntry>,
    monthly_summary: BudgetGroupSummary,
    annual_summary: BudgetGroupSummary,
    project_summary: BudgetGroupSummary,
    /// Transactions contributing to monthly budgets in the active month.
    monthly_transactions: Vec<Transaction>,
    /// Transactions contributing to annual budgets across the budget year.
    annual_transactions: Vec<Transaction>,
    /// Transactions contributing to project budgets.
    project_transactions: Vec<Transaction>,
    /// Transactions in categories with no budget config or with no category at all.
    unbudgeted_transactions: Vec<Transaction>,
    /// Total spending on unbudgeted/uncategorized transactions (positive = money spent).
    unbudgeted_spent: Decimal,
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
    // Find salary categories by budget mode
    let salary_roots: Vec<&Category> = categories
        .iter()
        .filter(|c| c.budget.mode() == Some(BudgetMode::Salary))
        .collect();

    let salary_txns = if salary_roots.is_empty() {
        Vec::new()
    } else {
        let mut all_ids = Vec::new();
        for root in &salary_roots {
            all_ids.extend(collect_category_subtree(root.id, categories));
        }
        state.db.list_transactions_by_category_ids(&all_ids).await?
    };

    match detect_budget_month_boundaries(&salary_txns, state.expected_salary_count, categories) {
        Ok(mut months) => {
            months.sort_by_key(|bm| bm.start_date);
            Ok(months)
        }
        Err(budget_core::error::Error::NoSalaryCategory) => Ok(Vec::new()),
        Err(e) => Err(AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

/// Pre-classified transactions and auxiliary dashboard data.
struct ClassifiedTransactions {
    monthly: Vec<Transaction>,
    annual: Vec<Transaction>,
    project: Vec<Transaction>,
    unbudgeted: Vec<Transaction>,
    unbudgeted_spent: Decimal,
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

    let none_cat_ids: std::collections::HashSet<CategoryId> = categories
        .iter()
        .filter(|c| effective_budget_mode(c, categories).is_none())
        .map(|c| c.id)
        .collect();

    let uncategorized: Vec<Transaction> = transactions
        .iter()
        .filter(|t| {
            is_in_budget_month(t.posted_date, month)
                && t.category_id.is_none()
                && t.correlation.is_none()
        })
        .cloned()
        .collect();

    let mut unbudgeted: Vec<Transaction> = budget_txns
        .iter()
        .filter(|t| {
            t.category_id.is_some_and(|cid| none_cat_ids.contains(&cid))
                && is_in_budget_month(t.posted_date, month)
        })
        .map(|t| (*t).clone())
        .collect();
    unbudgeted.extend(uncategorized);

    let unbudgeted_spent = -unbudgeted
        .iter()
        .fold(Decimal::ZERO, |acc, t| acc + t.amount);

    let project: Vec<Transaction> = filter_for_project(transactions, categories)
        .into_iter()
        .cloned()
        .collect();

    let budget_year = year_months.first().map_or_else(
        || chrono::Datelike::year(&month.start_date),
        |bm| chrono::Datelike::year(&bm.start_date),
    );

    ClassifiedTransactions {
        monthly,
        annual,
        project,
        unbudgeted,
        unbudgeted_spent,
        budget_year,
    }
}

/// Compute deduplicated summary totals for a group of budget entries.
///
/// Excludes entries that are children of another entry in the list from the
/// budget/spent totals (they're already reflected in their parent's figures).
/// Over-budget count and bar scale include all entries.
fn compute_group_summary(entries: &[&StatusEntry]) -> BudgetGroupSummary {
    let child_ids: std::collections::HashSet<CategoryId> = entries
        .iter()
        .flat_map(|e| e.children.iter().map(|c| c.category_id))
        .collect();

    let mut total_budget = Decimal::ZERO;
    let mut total_spent = Decimal::ZERO;
    let mut over_budget_count = 0;
    let mut bar_max = Decimal::ZERO;

    for entry in entries {
        let s = &entry.status;

        if !child_ids.contains(&s.category_id) {
            total_budget += s.budget_amount;
            total_spent += s.spent;
        }

        if s.pace == PaceIndicator::OverBudget {
            over_budget_count += 1;
        }
        bar_max = bar_max.max(s.spent.abs()).max(s.budget_amount);
    }

    BudgetGroupSummary {
        total_budget,
        total_spent,
        remaining: total_budget - total_spent,
        over_budget_count,
        bar_max,
    }
}

/// Compute deduplicated summary totals for project entries.
fn compute_project_group_summary(entries: &[ProjectStatusEntry]) -> BudgetGroupSummary {
    let mut total_budget = Decimal::ZERO;
    let mut total_spent = Decimal::ZERO;
    let mut over_budget_count = 0;
    let mut bar_max = Decimal::ZERO;

    for entry in entries {
        let s = &entry.status;
        total_budget += s.budget_amount;
        total_spent += s.spent;
        if s.pace == PaceIndicator::OverBudget {
            over_budget_count += 1;
        }
        bar_max = bar_max.max(s.spent.abs()).max(s.budget_amount);
    }

    BudgetGroupSummary {
        total_budget,
        total_spent,
        remaining: total_budget - total_spent,
        over_budget_count,
        bar_max,
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
        .filter(|c| c.budget.mode() == Some(BudgetMode::Project))
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
        if let BudgetConfig::Project { start_date, .. } = &cat.budget {
            earliest = earliest.min(*start_date);
        }
    }

    let transactions = state.db.list_transactions_since(earliest).await?;
    let reference_date = month.end_date.unwrap_or_else(|| Utc::now().date_naive());

    let statuses: Vec<StatusEntry> = categories
        .iter()
        .filter(|c| {
            matches!(
                c.budget.mode(),
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

    let monthly_entries: Vec<&StatusEntry> = statuses
        .iter()
        .filter(|e| e.status.budget_mode == BudgetMode::Monthly)
        .collect();
    let annual_entries: Vec<&StatusEntry> = statuses
        .iter()
        .filter(|e| e.status.budget_mode == BudgetMode::Annual)
        .collect();
    let monthly_summary = compute_group_summary(&monthly_entries);
    let annual_summary = compute_group_summary(&annual_entries);
    let project_summary = compute_project_group_summary(&projects);

    Ok(Json(StatusResponse {
        month: month.clone(),
        statuses,
        projects,
        monthly_summary,
        annual_summary,
        project_summary,
        monthly_transactions: classified.monthly,
        annual_transactions: classified.annual,
        project_transactions: classified.project,
        unbudgeted_transactions: classified.unbudgeted,
        unbudgeted_spent: classified.unbudgeted_spent,
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

#[cfg(test)]
mod tests {
    use super::*;
    fn dec(v: i64) -> Decimal {
        Decimal::from(v)
    }

    fn make_status(
        id: u128,
        name: &str,
        budget: Decimal,
        spent: Decimal,
        pace: PaceIndicator,
    ) -> BudgetStatus {
        BudgetStatus {
            category_id: CategoryId::from_uuid(uuid::Uuid::from_u128(id)),
            category_name: name.to_owned(),
            budget_amount: budget,
            spent,
            remaining: budget - spent,
            time_left: Some(10),
            pace,
            pace_delta: Decimal::ZERO,
            budget_mode: BudgetMode::Monthly,
        }
    }

    fn make_entry(status: BudgetStatus, children: Vec<ChildCategoryInfo>) -> StatusEntry {
        let has_children = !children.is_empty();
        StatusEntry {
            status,
            children,
            has_children,
        }
    }

    fn child_info(id: u128, name: &str) -> ChildCategoryInfo {
        ChildCategoryInfo {
            category_id: CategoryId::from_uuid(uuid::Uuid::from_u128(id)),
            category_name: name.to_owned(),
        }
    }

    #[test]
    fn summary_excludes_child_budget_and_spent() {
        // House ($5000 budget, $0 direct spent) has children Mortgage and Electricity.
        // Mortgage ($4032 budget, $4032 spent) and Electricity ($350, $27) are separate entries.
        // Summary should count House OR its children, not both.
        let house = make_entry(
            make_status(1, "House", dec(5000), dec(0), PaceIndicator::OnTrack),
            vec![child_info(2, "Mortgage"), child_info(3, "Electricity")],
        );
        let mortgage = make_entry(
            make_status(2, "Mortgage", dec(4032), dec(4032), PaceIndicator::OnTrack),
            vec![],
        );
        let electricity = make_entry(
            make_status(
                3,
                "Electricity",
                dec(350),
                dec(27),
                PaceIndicator::UnderBudget,
            ),
            vec![],
        );

        let entries: Vec<&StatusEntry> = vec![&house, &mortgage, &electricity];
        let summary = compute_group_summary(&entries);

        // Only House (root) counted: children excluded from budget/spent
        assert_eq!(summary.total_budget, dec(5000));
        assert_eq!(summary.total_spent, dec(0));
        assert_eq!(summary.remaining, dec(5000));
    }

    #[test]
    fn summary_counts_independent_roots() {
        let food = make_entry(
            make_status(1, "Food", dec(1000), dec(366), PaceIndicator::UnderBudget),
            vec![],
        );
        let savings = make_entry(
            make_status(2, "Savings", dec(1000), dec(0), PaceIndicator::OnTrack),
            vec![],
        );

        let entries: Vec<&StatusEntry> = vec![&food, &savings];
        let summary = compute_group_summary(&entries);

        assert_eq!(summary.total_budget, dec(2000));
        assert_eq!(summary.total_spent, dec(366));
        assert_eq!(summary.remaining, dec(1634));
    }

    #[test]
    fn summary_over_budget_count_includes_all_entries() {
        // Over-budget count should include children (they're separate budget targets)
        let parent = make_entry(
            make_status(1, "Food", dec(500), dec(600), PaceIndicator::OverBudget),
            vec![child_info(2, "Dining")],
        );
        let child = make_entry(
            make_status(2, "Dining", dec(100), dec(150), PaceIndicator::OverBudget),
            vec![],
        );

        let entries: Vec<&StatusEntry> = vec![&parent, &child];
        let summary = compute_group_summary(&entries);

        assert_eq!(summary.over_budget_count, 2);
    }

    #[test]
    fn summary_bar_max_considers_all_entries() {
        let parent = make_entry(
            make_status(1, "House", dec(5000), dec(0), PaceIndicator::OnTrack),
            vec![child_info(2, "Mortgage")],
        );
        let child = make_entry(
            make_status(2, "Mortgage", dec(4032), dec(4032), PaceIndicator::OnTrack),
            vec![],
        );

        let entries: Vec<&StatusEntry> = vec![&parent, &child];
        let summary = compute_group_summary(&entries);

        // bar_max should be 5000 (House budget) even though it's a parent
        assert_eq!(summary.bar_max, dec(5000));
    }

    #[test]
    fn summary_empty_entries() {
        let entries: Vec<&StatusEntry> = vec![];
        let summary = compute_group_summary(&entries);

        assert_eq!(summary.total_budget, dec(0));
        assert_eq!(summary.total_spent, dec(0));
        assert_eq!(summary.remaining, dec(0));
        assert_eq!(summary.over_budget_count, 0);
        assert_eq!(summary.bar_max, dec(0));
    }
}
