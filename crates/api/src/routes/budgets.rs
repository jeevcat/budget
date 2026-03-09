use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use chrono::{NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use budget_core::budget::{
    budget_year_months, collect_category_subtree, compute_budget_status,
    compute_project_child_breakdowns, detect_budget_month_boundaries, effective_budget_mode,
    filter_for_budget, filter_for_project, is_in_budget_month, salary_category_ids,
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
    project_start_date: NaiveDate,
    project_end_date: Option<NaiveDate>,
    finished: bool,
}

#[derive(Serialize)]
struct BudgetGroupSummary {
    total_budget: Decimal,
    total_spent: Decimal,
    remaining: Decimal,
    over_budget_count: usize,
    bar_max: Decimal,
}

/// A single line item in a cash-flow section, grouped by category.
#[derive(Serialize)]
struct CashFlowItem {
    category_id: Option<CategoryId>,
    label: String,
    amount: Decimal,
    transaction_count: usize,
    transactions: Vec<Transaction>,
}

/// One side of the cash-flow ledger (e.g. "salary income" or "unbudgeted spending").
#[derive(Serialize)]
struct CashFlowSection {
    total: Decimal,
    items: Vec<CashFlowItem>,
}

/// Full cash-flow breakdown for a budget period (month or year).
#[derive(Serialize)]
struct CashFlowSummary {
    /// Budget health metrics.
    #[serde(flatten)]
    budget: BudgetGroupSummary,

    /// Salary / child-of-salary income.
    income: CashFlowSection,
    /// Positive unbudgeted transactions (RSUs, asset sales, etc.).
    other_income: CashFlowSection,
    /// Sum of budgeted category spending (monthly + project-in-month).
    budgeted_spending: CashFlowSection,
    /// Negative unbudgeted / uncategorized transactions.
    unbudgeted_spending: CashFlowSection,

    /// `income.total + other_income.total`
    total_in: Decimal,
    /// `budgeted_spending.total + unbudgeted_spending.total`
    total_out: Decimal,
    /// `total_in - total_out`
    net_cashflow: Decimal,
    /// `income.total - budgeted_spending.total - unbudgeted_spending.total`
    saved: Decimal,
}

#[derive(Serialize)]
struct StatusResponse {
    month: BudgetMonth,
    statuses: Vec<StatusEntry>,
    projects: Vec<ProjectStatusEntry>,
    monthly_cashflow: CashFlowSummary,
    annual_cashflow: CashFlowSummary,
    project_summary: BudgetGroupSummary,
    /// Transactions contributing to monthly budgets in the active month.
    monthly_transactions: Vec<Transaction>,
    /// Transactions contributing to annual budgets across the budget year.
    annual_transactions: Vec<Transaction>,
    /// Transactions contributing to project budgets.
    project_transactions: Vec<Transaction>,
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
    /// Unbudgeted/uncategorized transactions in the current budget month.
    unbudgeted: Vec<Transaction>,
    /// Unbudgeted/uncategorized transactions across the budget year.
    unbudgeted_annual: Vec<Transaction>,
    budget_year: i32,
    income: Vec<Transaction>,
    annual_income: Vec<Transaction>,
}

/// Collect positive salary-category transactions within a date range.
fn collect_income_transactions(
    transactions: &[Transaction],
    salary_cats: &std::collections::HashSet<CategoryId>,
    start: Option<chrono::NaiveDate>,
    end: Option<chrono::NaiveDate>,
) -> Vec<Transaction> {
    transactions
        .iter()
        .filter(|t| {
            t.categorization
                .category_id()
                .is_some_and(|cid| salary_cats.contains(&cid))
                && t.amount > Decimal::ZERO
                && start.is_none_or(|s| t.posted_date >= s)
                && end.is_none_or(|e| t.posted_date <= e)
        })
        .cloned()
        .collect()
}

/// Classify transactions by budget mode and compute auxiliary dashboard data.
///
/// Returns the actual transaction objects grouped by mode so the frontend
/// can display them directly without any business logic.
fn is_in_date_range(
    date: chrono::NaiveDate,
    start: Option<chrono::NaiveDate>,
    end: Option<chrono::NaiveDate>,
) -> bool {
    start.is_none_or(|s| date >= s) && end.is_none_or(|e| date <= e)
}

fn collect_unbudgeted(
    all_txns: &[Transaction],
    budget_txns: &[&Transaction],
    none_cat_ids: &std::collections::HashSet<CategoryId>,
    start: Option<chrono::NaiveDate>,
    end: Option<chrono::NaiveDate>,
) -> Vec<Transaction> {
    let mut result: Vec<Transaction> = budget_txns
        .iter()
        .filter(|t| {
            t.categorization
                .category_id()
                .is_some_and(|cid| none_cat_ids.contains(&cid))
                && is_in_date_range(t.posted_date, start, end)
        })
        .map(|t| (*t).clone())
        .collect();
    let uncategorized: Vec<Transaction> = all_txns
        .iter()
        .filter(|t| {
            is_in_date_range(t.posted_date, start, end)
                && !t.categorization.is_categorized()
                && t.correlation.is_none()
        })
        .cloned()
        .collect();
    result.extend(uncategorized);
    result
}

fn classify_transactions(
    transactions: &[Transaction],
    categories: &[Category],
    month: &BudgetMonth,
    year_months: &[&BudgetMonth],
) -> ClassifiedTransactions {
    let budget_txns = filter_for_budget(transactions, categories);

    let cat_ids_for_mode = |mode: BudgetMode| -> std::collections::HashSet<CategoryId> {
        categories
            .iter()
            .filter(|c| effective_budget_mode(c, categories) == Some(mode))
            .map(|c| c.id)
            .collect()
    };

    let monthly_cat_ids = cat_ids_for_mode(BudgetMode::Monthly);
    let annual_cat_ids = cat_ids_for_mode(BudgetMode::Annual);

    let monthly: Vec<Transaction> = budget_txns
        .iter()
        .filter(|t| {
            t.categorization
                .category_id()
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
            t.categorization
                .category_id()
                .is_some_and(|cid| annual_cat_ids.contains(&cid))
                && is_in_date_range(t.posted_date, year_start, year_end)
        })
        .map(|t| (*t).clone())
        .collect();

    let none_cat_ids: std::collections::HashSet<CategoryId> = categories
        .iter()
        .filter(|c| effective_budget_mode(c, categories).is_none())
        .map(|c| c.id)
        .collect();

    let unbudgeted = collect_unbudgeted(
        transactions,
        &budget_txns,
        &none_cat_ids,
        Some(month.start_date),
        month.end_date,
    );
    let unbudgeted_annual = collect_unbudgeted(
        transactions,
        &budget_txns,
        &none_cat_ids,
        year_start,
        year_end,
    );

    let project: Vec<Transaction> = filter_for_project(transactions, categories)
        .into_iter()
        .cloned()
        .collect();

    let budget_year = year_months.first().map_or_else(
        || chrono::Datelike::year(&month.start_date),
        |bm| chrono::Datelike::year(&bm.start_date),
    );

    let salary_cats = salary_category_ids(categories);
    let income = collect_income_transactions(
        transactions,
        &salary_cats,
        Some(month.start_date),
        month.end_date,
    );
    let annual_income =
        collect_income_transactions(transactions, &salary_cats, year_start, year_end);

    ClassifiedTransactions {
        monthly,
        annual,
        project,
        unbudgeted,
        unbudgeted_annual,
        budget_year,
        income,
        annual_income,
    }
}

/// Negated sum of transaction amounts (positive = money spent).
fn negate_sum(transactions: &[Transaction]) -> Decimal {
    -transactions
        .iter()
        .fold(Decimal::ZERO, |acc, t| acc + t.amount)
}

/// Compute budget-health totals for a group of budget entries.
///
/// - `total_budget`: sum of every entry's `budget_amount` (each entry
///   is an independent budget target — parent budgets do NOT envelope
///   their children).
/// - `total_spent`: negated sum of `transactions` — simple, hierarchy-free.
fn compute_group_summary(
    entries: &[&StatusEntry],
    transactions: &[Transaction],
) -> BudgetGroupSummary {
    let mut total_budget = Decimal::ZERO;
    let mut over_budget_count = 0;
    let mut bar_max = Decimal::ZERO;

    for entry in entries {
        let s = &entry.status;
        total_budget += s.budget_amount;

        if s.pace == PaceIndicator::OverBudget {
            over_budget_count += 1;
        }
        bar_max = bar_max.max(s.spent.abs()).max(s.budget_amount);
    }

    let total_spent = negate_sum(transactions);

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

/// Group transactions by category into [`CashFlowItem`]s.
///
/// Each distinct category becomes one item; uncategorized transactions
/// are gathered under a single "Uncategorized" label. Amounts are
/// stored as absolute values (always positive).
fn group_by_category(transactions: &[Transaction], categories: &[Category]) -> Vec<CashFlowItem> {
    let cat_map: std::collections::HashMap<CategoryId, &Category> =
        categories.iter().map(|c| (c.id, c)).collect();

    let mut by_cat: std::collections::HashMap<Option<CategoryId>, Vec<&Transaction>> =
        std::collections::HashMap::new();
    for t in transactions {
        let cid = t.categorization.category_id();
        by_cat.entry(cid).or_default().push(t);
    }

    let mut items: Vec<CashFlowItem> = by_cat
        .into_iter()
        .map(|(cid, txns)| {
            let label = cid
                .and_then(|id| cat_map.get(&id))
                .map_or_else(|| "Uncategorized".to_owned(), |c| c.name.to_string());
            let amount = txns
                .iter()
                .fold(Decimal::ZERO, |acc, t| acc + t.amount)
                .abs();
            CashFlowItem {
                category_id: cid,
                label,
                amount,
                transaction_count: txns.len(),
                transactions: txns.into_iter().cloned().collect(),
            }
        })
        .collect();
    items.sort_by(|a, b| b.amount.cmp(&a.amount));
    items
}

/// Build a [`CashFlowSection`] from a slice of transactions, grouping by category.
fn build_cashflow_section(
    transactions: &[Transaction],
    categories: &[Category],
) -> CashFlowSection {
    let total = transactions
        .iter()
        .fold(Decimal::ZERO, |acc, t| acc + t.amount)
        .abs();
    let items = if transactions.is_empty() {
        Vec::new()
    } else {
        group_by_category(transactions, categories)
    };
    CashFlowSection { total, items }
}

/// Build all summaries: monthly + annual cash-flow, and project budget summary.
fn build_summaries(
    statuses: &[StatusEntry],
    classified: &ClassifiedTransactions,
    categories: &[Category],
    month: &BudgetMonth,
    projects: &[ProjectStatusEntry],
) -> (CashFlowSummary, CashFlowSummary, BudgetGroupSummary) {
    let monthly_entries: Vec<&StatusEntry> = statuses
        .iter()
        .filter(|e| e.status.budget_mode == BudgetMode::Monthly)
        .collect();
    let annual_entries: Vec<&StatusEntry> = statuses
        .iter()
        .filter(|e| e.status.budget_mode == BudgetMode::Annual)
        .collect();

    // Project transactions in the current budget month (counted as monthly spending)
    let project_month_txns: Vec<Transaction> = classified
        .project
        .iter()
        .filter(|t| is_in_budget_month(t.posted_date, month))
        .cloned()
        .collect();

    // Monthly cash flow
    let monthly_budget = compute_group_summary(&monthly_entries, &classified.monthly);
    let monthly_budgeted_spent = negate_sum(&classified.monthly) + negate_sum(&project_month_txns);
    let monthly_cashflow = build_cashflow(
        monthly_budget,
        &classified.income,
        &classified.unbudgeted,
        monthly_budgeted_spent,
        categories,
    );

    // Annual cash flow
    let annual_budget = compute_group_summary(&annual_entries, &classified.annual);
    let annual_budgeted_spent = negate_sum(&classified.annual);
    let annual_cashflow = build_cashflow(
        annual_budget,
        &classified.annual_income,
        &classified.unbudgeted_annual,
        annual_budgeted_spent,
        categories,
    );

    let project_summary = compute_project_group_summary(projects);

    (monthly_cashflow, annual_cashflow, project_summary)
}

/// Assemble a [`CashFlowSummary`] from its constituent parts.
fn build_cashflow(
    budget: BudgetGroupSummary,
    income_txns: &[Transaction],
    unbudgeted_txns: &[Transaction],
    budgeted_spent: Decimal,
    categories: &[Category],
) -> CashFlowSummary {
    let income = build_cashflow_section(income_txns, categories);

    // Split unbudgeted by sign
    let positive_unbudgeted: Vec<Transaction> = unbudgeted_txns
        .iter()
        .filter(|t| t.amount > Decimal::ZERO)
        .cloned()
        .collect();
    let negative_unbudgeted: Vec<Transaction> = unbudgeted_txns
        .iter()
        .filter(|t| t.amount < Decimal::ZERO)
        .cloned()
        .collect();

    let other_income = build_cashflow_section(&positive_unbudgeted, categories);
    let unbudgeted_spending = build_cashflow_section(&negative_unbudgeted, categories);

    let budgeted_spending = CashFlowSection {
        total: budgeted_spent,
        items: Vec::new(),
    };

    let total_in = income.total + other_income.total;
    let total_out = budgeted_spending.total + unbudgeted_spending.total;
    let net_cashflow = total_in - total_out;
    let saved = income.total - budgeted_spending.total - unbudgeted_spending.total;

    CashFlowSummary {
        budget,
        income,
        other_income,
        budgeted_spending,
        unbudgeted_spending,
        total_in,
        total_out,
        net_cashflow,
        saved,
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
                    .filter(|t| {
                        t.categorization
                            .category_id()
                            .is_some_and(|cid| subtree_ids.contains(&cid))
                    })
                    .copied()
                    .collect();
                compute_project_child_breakdowns(cat, &subtree_txns, categories)
            } else {
                Vec::new()
            };
            let (project_start_date, project_end_date) = match &cat.budget {
                BudgetConfig::Project {
                    start_date,
                    end_date,
                    ..
                } => (*start_date, *end_date),
                _ => unreachable!("filtered to projects above"),
            };
            let finished = project_end_date.is_some_and(|end| end < reference_date);
            ProjectStatusEntry {
                status,
                children,
                has_children,
                project_start_date,
                project_end_date,
                finished,
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

    let (monthly_cashflow, annual_cashflow, project_summary) =
        build_summaries(&statuses, &classified, &categories, month, &projects);

    Ok(Json(StatusResponse {
        month: month.clone(),
        statuses,
        projects,
        monthly_cashflow,
        annual_cashflow,
        project_summary,
        monthly_transactions: classified.monthly,
        annual_transactions: classified.annual,
        project_transactions: classified.project,
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
    use budget_core::models::Categorization;

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

    /// Build a minimal transaction list whose `-sum(amount)` equals `total_spent`.
    fn txns_for_spent(total_spent: Decimal) -> Vec<Transaction> {
        vec![Transaction {
            amount: -total_spent,
            merchant_name: "Test".to_owned(),
            ..Default::default()
        }]
    }

    #[test]
    fn summary_budget_sums_all_entries() {
        // Parent budget is extra on top of children, not an envelope.
        // House ($70 buffer) + Mortgage ($4032) + Electricity ($350) = $4452
        let house = make_entry(
            make_status(1, "House", dec(70), dec(0), PaceIndicator::OnTrack),
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
        let txns = txns_for_spent(dec(4059));
        let summary = compute_group_summary(&entries, &txns);

        assert_eq!(summary.total_budget, dec(4452));
        assert_eq!(summary.total_spent, dec(4059));
        assert_eq!(summary.remaining, dec(393));
    }

    #[test]
    fn summary_spent_from_transactions_not_entries() {
        // Spent is the negated sum of transactions, independent of entry.spent
        let food = make_entry(
            make_status(1, "Food", dec(1000), dec(999), PaceIndicator::UnderBudget),
            vec![],
        );

        let entries: Vec<&StatusEntry> = vec![&food];
        let txns = txns_for_spent(dec(366));
        let summary = compute_group_summary(&entries, &txns);

        // entry.spent ($999) is irrelevant — only transactions matter
        assert_eq!(summary.total_spent, dec(366));
        assert_eq!(summary.remaining, dec(634));
    }

    #[test]
    fn summary_independent_roots() {
        let food = make_entry(
            make_status(1, "Food", dec(1000), dec(366), PaceIndicator::UnderBudget),
            vec![],
        );
        let savings = make_entry(
            make_status(2, "Savings", dec(1000), dec(0), PaceIndicator::OnTrack),
            vec![],
        );

        let entries: Vec<&StatusEntry> = vec![&food, &savings];
        let txns = txns_for_spent(dec(366));
        let summary = compute_group_summary(&entries, &txns);

        assert_eq!(summary.total_budget, dec(2000));
        assert_eq!(summary.total_spent, dec(366));
        assert_eq!(summary.remaining, dec(1634));
    }

    #[test]
    fn summary_over_budget_count() {
        let parent = make_entry(
            make_status(1, "Food", dec(500), dec(600), PaceIndicator::OverBudget),
            vec![child_info(2, "Dining")],
        );
        let child = make_entry(
            make_status(2, "Dining", dec(100), dec(150), PaceIndicator::OverBudget),
            vec![],
        );

        let entries: Vec<&StatusEntry> = vec![&parent, &child];
        let txns = txns_for_spent(dec(750));
        let summary = compute_group_summary(&entries, &txns);

        assert_eq!(summary.over_budget_count, 2);
    }

    #[test]
    fn summary_bar_max() {
        let parent = make_entry(
            make_status(1, "House", dec(5000), dec(0), PaceIndicator::OnTrack),
            vec![child_info(2, "Mortgage")],
        );
        let child = make_entry(
            make_status(2, "Mortgage", dec(4032), dec(4032), PaceIndicator::OnTrack),
            vec![],
        );

        let entries: Vec<&StatusEntry> = vec![&parent, &child];
        let txns = txns_for_spent(dec(4032));
        let summary = compute_group_summary(&entries, &txns);

        assert_eq!(summary.bar_max, dec(5000));
    }

    #[test]
    fn summary_empty() {
        let entries: Vec<&StatusEntry> = vec![];
        let summary = compute_group_summary(&entries, &[]);

        assert_eq!(summary.total_budget, dec(0));
        assert_eq!(summary.total_spent, dec(0));
        assert_eq!(summary.remaining, dec(0));
        assert_eq!(summary.over_budget_count, 0);
        assert_eq!(summary.bar_max, dec(0));
    }

    #[test]
    fn classify_includes_annual_txns_in_late_december_budget_month() {
        use budget_core::models::{
            BudgetConfig, BudgetMonth, BudgetMonthId, BudgetType, Category, CategoryName,
            Transaction,
        };
        use chrono::NaiveDate;

        let cat_id = CategoryId::from_uuid(uuid::Uuid::from_u128(1));
        let insurance = Category {
            id: cat_id,
            name: CategoryName::new("Car Insurance").unwrap(),
            parent_id: None,
            budget: BudgetConfig::Annual {
                amount: Decimal::from(3200),
                budget_type: BudgetType::Fixed,
            },
        };
        let categories = vec![insurance];

        // Budget month starting Dec 30 covers all of January
        let bm_dec = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: NaiveDate::from_ymd_opt(2025, 12, 30).unwrap(),
            end_date: Some(NaiveDate::from_ymd_opt(2026, 1, 29).unwrap()),
            salary_transactions_detected: 1,
        };
        let bm_jan = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: NaiveDate::from_ymd_opt(2026, 1, 30).unwrap(),
            end_date: Some(NaiveDate::from_ymd_opt(2026, 2, 26).unwrap()),
            salary_transactions_detected: 1,
        };
        let bm_feb = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: NaiveDate::from_ymd_opt(2026, 2, 27).unwrap(),
            end_date: None,
            salary_transactions_detected: 1,
        };
        let all_months = [bm_dec.clone(), bm_jan.clone(), bm_feb.clone()];
        let year_months = budget_core::budget::budget_year_months(&all_months, &bm_feb);

        let txn = |d: (i32, u32, u32), amt: i64| -> Transaction {
            Transaction {
                categorization: Categorization::Manual(cat_id),
                amount: Decimal::from(amt),
                merchant_name: "Insurer".to_owned(),
                posted_date: NaiveDate::from_ymd_opt(d.0, d.1, d.2).unwrap(),
                ..Default::default()
            }
        };

        let transactions = vec![
            txn((2026, 1, 2), -1208),
            txn((2026, 1, 5), -1392),
            txn((2026, 1, 15), -529),
        ];

        let classified = classify_transactions(&transactions, &categories, &bm_feb, &year_months);

        assert_eq!(
            classified.annual.len(),
            3,
            "all January transactions should appear in annual list even when their budget month starts in December"
        );
    }

    #[test]
    fn classify_includes_income_transactions() {
        use budget_core::models::{
            BudgetConfig, BudgetMonth, BudgetMonthId, BudgetType, Category, CategoryName,
            Transaction,
        };
        use chrono::NaiveDate;

        let salary_root_id = CategoryId::from_uuid(uuid::Uuid::from_u128(10));
        let salary_child_id = CategoryId::from_uuid(uuid::Uuid::from_u128(11));
        let food_id = CategoryId::from_uuid(uuid::Uuid::from_u128(20));

        let categories = vec![
            Category {
                id: salary_root_id,
                name: CategoryName::new("Salary").unwrap(),
                parent_id: None,
                budget: BudgetConfig::Salary,
            },
            Category {
                id: salary_child_id,
                name: CategoryName::new("Bonus").unwrap(),
                parent_id: Some(salary_root_id),
                budget: BudgetConfig::None,
            },
            Category {
                id: food_id,
                name: CategoryName::new("Food").unwrap(),
                parent_id: None,
                budget: BudgetConfig::Monthly {
                    amount: Decimal::from(500),
                    budget_type: BudgetType::Variable,
                },
            },
        ];

        let month = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: NaiveDate::from_ymd_opt(2026, 2, 1).unwrap(),
            end_date: Some(NaiveDate::from_ymd_opt(2026, 2, 28).unwrap()),
            salary_transactions_detected: 1,
        };
        let year_months: Vec<&BudgetMonth> = vec![&month];

        let txn = |cat: CategoryId, d: (i32, u32, u32), amt: i64| -> Transaction {
            Transaction {
                categorization: Categorization::Manual(cat),
                amount: Decimal::from(amt),
                merchant_name: "Test".to_owned(),
                posted_date: NaiveDate::from_ymd_opt(d.0, d.1, d.2).unwrap(),
                ..Default::default()
            }
        };

        let transactions = vec![
            // Salary root in-month (positive = income) → included
            txn(salary_root_id, (2026, 2, 5), 3000),
            // Salary subcategory in-month (positive) → included
            txn(salary_child_id, (2026, 2, 10), 500),
            // Negative salary transaction (refund) → excluded
            txn(salary_root_id, (2026, 2, 15), -200),
            // Salary outside budget month → excluded
            txn(salary_root_id, (2026, 1, 15), 3000),
            // Non-salary positive transaction → excluded
            txn(food_id, (2026, 2, 10), 50),
        ];

        let classified = classify_transactions(&transactions, &categories, &month, &year_months);

        assert_eq!(
            classified.income.len(),
            2,
            "should include root and subcategory salary txns"
        );
        let income_sum: Decimal = classified.income.iter().map(|t| t.amount).sum();
        assert_eq!(income_sum, dec(3500), "income should be 3000 + 500");
    }

    #[test]
    fn classify_includes_annual_income_transactions() {
        use budget_core::models::{
            BudgetConfig, BudgetMonth, BudgetMonthId, BudgetType, Category, CategoryName,
            Transaction,
        };
        use chrono::NaiveDate;

        let salary_id = CategoryId::from_uuid(uuid::Uuid::from_u128(10));
        let food_id = CategoryId::from_uuid(uuid::Uuid::from_u128(20));

        let categories = vec![
            Category {
                id: salary_id,
                name: CategoryName::new("Salary").unwrap(),
                parent_id: None,
                budget: BudgetConfig::Salary,
            },
            Category {
                id: food_id,
                name: CategoryName::new("Food").unwrap(),
                parent_id: None,
                budget: BudgetConfig::Monthly {
                    amount: Decimal::from(500),
                    budget_type: BudgetType::Variable,
                },
            },
        ];

        let bm1 = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            end_date: Some(NaiveDate::from_ymd_opt(2026, 1, 31).unwrap()),
            salary_transactions_detected: 1,
        };
        let bm2 = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: NaiveDate::from_ymd_opt(2026, 2, 1).unwrap(),
            end_date: Some(NaiveDate::from_ymd_opt(2026, 2, 28).unwrap()),
            salary_transactions_detected: 1,
        };
        let bm3 = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            end_date: None,
            salary_transactions_detected: 1,
        };
        let year_months: Vec<&BudgetMonth> = vec![&bm1, &bm2, &bm3];

        let txn = |cat: CategoryId, d: (i32, u32, u32), amt: i64| -> Transaction {
            Transaction {
                categorization: Categorization::Manual(cat),
                amount: Decimal::from(amt),
                merchant_name: "Test".to_owned(),
                posted_date: NaiveDate::from_ymd_opt(d.0, d.1, d.2).unwrap(),
                ..Default::default()
            }
        };

        let transactions = vec![
            // Salary across different months in the year
            txn(salary_id, (2026, 1, 15), 3000),
            txn(salary_id, (2026, 2, 15), 3000),
            txn(salary_id, (2026, 3, 5), 3000),
            // Outside year range (before first month)
            txn(salary_id, (2025, 12, 15), 3000),
            // Negative salary → excluded
            txn(salary_id, (2026, 2, 20), -100),
            // Non-salary → excluded
            txn(food_id, (2026, 2, 10), 50),
        ];

        let classified = classify_transactions(&transactions, &categories, &bm3, &year_months);

        assert_eq!(
            classified.annual_income.len(),
            3,
            "should include salary txns from all months in the year"
        );
        let annual_income_sum: Decimal = classified.annual_income.iter().map(|t| t.amount).sum();
        assert_eq!(
            annual_income_sum,
            dec(9000),
            "annual income should be 3000 * 3 months"
        );
    }

    // ── Shared test helpers for cash-flow tests ──────────────────────

    use budget_core::models::{
        BudgetConfig, BudgetMonth, BudgetMonthId, BudgetType, Category, CategoryName, Correlation,
        CorrelationType, TransactionId,
    };
    use chrono::NaiveDate;

    fn cat_id(n: u128) -> CategoryId {
        CategoryId::from_uuid(uuid::Uuid::from_u128(n))
    }

    fn make_category(id: u128, name: &str, config: BudgetConfig) -> Category {
        Category {
            id: cat_id(id),
            name: CategoryName::new(name).unwrap(),
            parent_id: None,
            budget: config,
        }
    }

    fn txn(cat: CategoryId, amt: i64) -> Transaction {
        Transaction {
            categorization: Categorization::Manual(cat),
            amount: Decimal::from(amt),
            merchant_name: "Test".to_owned(),
            posted_date: NaiveDate::from_ymd_opt(2026, 2, 15).unwrap(),
            ..Default::default()
        }
    }

    fn txn_on(cat: CategoryId, amt: i64, date: NaiveDate) -> Transaction {
        Transaction {
            categorization: Categorization::Manual(cat),
            amount: Decimal::from(amt),
            merchant_name: "Test".to_owned(),
            posted_date: date,
            ..Default::default()
        }
    }

    fn uncategorized_txn(amt: i64) -> Transaction {
        Transaction {
            amount: Decimal::from(amt),
            merchant_name: "Unknown".to_owned(),
            posted_date: NaiveDate::from_ymd_opt(2026, 2, 15).unwrap(),
            ..Default::default()
        }
    }

    fn uncategorized_txn_on(amt: i64, date: NaiveDate) -> Transaction {
        Transaction {
            amount: Decimal::from(amt),
            merchant_name: "Unknown".to_owned(),
            posted_date: date,
            ..Default::default()
        }
    }

    fn correlated_txn(amt: i64) -> Transaction {
        Transaction {
            amount: Decimal::from(amt),
            merchant_name: "Transfer".to_owned(),
            posted_date: NaiveDate::from_ymd_opt(2026, 2, 15).unwrap(),
            correlation: Some(Correlation {
                partner_id: TransactionId::new(),
                correlation_type: CorrelationType::Transfer,
            }),
            ..Default::default()
        }
    }

    // ── group_by_category tests ─────────────────────────────────────

    #[test]
    fn group_by_category_multiple_categories() {
        let food_id = cat_id(1);
        let transport_id = cat_id(2);
        let categories = vec![
            make_category(1, "Food", BudgetConfig::None),
            make_category(2, "Transport", BudgetConfig::None),
        ];

        let transactions = vec![
            txn(food_id, -100),
            txn(food_id, -50),
            txn(transport_id, -200),
        ];

        let items = group_by_category(&transactions, &categories);

        assert_eq!(items.len(), 2);
        // Sorted by amount descending
        assert_eq!(items[0].label, "Transport");
        assert_eq!(items[0].amount, dec(200));
        assert_eq!(items[0].transaction_count, 1);
        assert_eq!(items[1].label, "Food");
        assert_eq!(items[1].amount, dec(150));
        assert_eq!(items[1].transaction_count, 2);
    }

    #[test]
    fn group_by_category_uncategorized_transactions() {
        let categories = vec![make_category(1, "Food", BudgetConfig::None)];

        let transactions = vec![uncategorized_txn(-75), uncategorized_txn(-25)];

        let items = group_by_category(&transactions, &categories);

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "Uncategorized");
        assert_eq!(items[0].category_id, None);
        assert_eq!(items[0].amount, dec(100));
        assert_eq!(items[0].transaction_count, 2);
    }

    #[test]
    fn group_by_category_mixed_categorized_and_uncategorized() {
        let food_id = cat_id(1);
        let categories = vec![make_category(1, "Food", BudgetConfig::None)];

        let transactions = vec![txn(food_id, -300), uncategorized_txn(-50)];

        let items = group_by_category(&transactions, &categories);

        assert_eq!(items.len(), 2);
        assert_eq!(items[0].label, "Food");
        assert_eq!(items[0].amount, dec(300));
        assert_eq!(items[1].label, "Uncategorized");
        assert_eq!(items[1].amount, dec(50));
    }

    #[test]
    fn group_by_category_amounts_are_absolute() {
        let food_id = cat_id(1);
        let categories = vec![make_category(1, "Food", BudgetConfig::None)];

        // Positive transactions (income-like)
        let transactions = vec![txn(food_id, 500)];

        let items = group_by_category(&transactions, &categories);

        assert_eq!(items[0].amount, dec(500));
    }

    #[test]
    fn group_by_category_empty_input() {
        let categories = vec![make_category(1, "Food", BudgetConfig::None)];
        let items = group_by_category(&[], &categories);
        assert!(items.is_empty());
    }

    #[test]
    fn group_by_category_preserves_transactions() {
        let food_id = cat_id(1);
        let categories = vec![make_category(1, "Food", BudgetConfig::None)];

        let transactions = vec![txn(food_id, -100), txn(food_id, -50)];

        let items = group_by_category(&transactions, &categories);

        assert_eq!(items[0].transactions.len(), 2);
    }

    // ── build_cashflow_section tests ────────────────────────────────

    #[test]
    fn cashflow_section_total_is_absolute_value() {
        let food_id = cat_id(1);
        let categories = vec![make_category(1, "Food", BudgetConfig::None)];

        // Negative transactions (spending)
        let transactions = vec![txn(food_id, -300), txn(food_id, -200)];

        let section = build_cashflow_section(&transactions, &categories);

        assert_eq!(section.total, dec(500));
        assert_eq!(section.items.len(), 1);
    }

    #[test]
    fn cashflow_section_positive_transactions() {
        let salary_id = cat_id(10);
        let categories = vec![make_category(10, "Salary", BudgetConfig::Salary)];

        let transactions = vec![txn(salary_id, 3000), txn(salary_id, 500)];

        let section = build_cashflow_section(&transactions, &categories);

        assert_eq!(section.total, dec(3500));
    }

    #[test]
    fn cashflow_section_empty() {
        let categories = vec![make_category(1, "Food", BudgetConfig::None)];

        let section = build_cashflow_section(&[], &categories);

        assert_eq!(section.total, dec(0));
        assert!(section.items.is_empty());
    }

    // ── build_cashflow tests ────────────────────────────────────────

    #[test]
    fn cashflow_splits_unbudgeted_by_sign() {
        let salary_id = cat_id(10);
        let rsu_id = cat_id(20);
        let tax_id = cat_id(30);
        let categories = vec![
            make_category(10, "Salary", BudgetConfig::Salary),
            make_category(20, "RSU", BudgetConfig::None),
            make_category(30, "Tax", BudgetConfig::None),
        ];

        let income_txns = vec![txn(salary_id, 5000)];
        let unbudgeted_txns = vec![
            txn(rsu_id, 10000), // positive → other_income
            txn(tax_id, -2000), // negative → unbudgeted_spending
        ];

        let budget = BudgetGroupSummary {
            total_budget: dec(3000),
            total_spent: dec(2500),
            remaining: dec(500),
            over_budget_count: 0,
            bar_max: dec(3000),
        };

        let cf = build_cashflow(
            budget,
            &income_txns,
            &unbudgeted_txns,
            dec(2500),
            &categories,
        );

        assert_eq!(cf.income.total, dec(5000));
        assert_eq!(cf.other_income.total, dec(10000));
        assert_eq!(cf.budgeted_spending.total, dec(2500));
        assert_eq!(cf.unbudgeted_spending.total, dec(2000));
    }

    #[test]
    fn cashflow_computed_totals() {
        let salary_id = cat_id(10);
        let rsu_id = cat_id(20);
        let misc_id = cat_id(30);
        let categories = vec![
            make_category(10, "Salary", BudgetConfig::Salary),
            make_category(20, "RSU", BudgetConfig::None),
            make_category(30, "Misc", BudgetConfig::None),
        ];

        let income_txns = vec![txn(salary_id, 5000)];
        let unbudgeted_txns = vec![
            txn(rsu_id, 2000),  // positive → other_income
            txn(misc_id, -800), // negative → unbudgeted_spending
        ];
        let budgeted_spent = dec(3000);

        let budget = BudgetGroupSummary {
            total_budget: dec(4000),
            total_spent: budgeted_spent,
            remaining: dec(1000),
            over_budget_count: 0,
            bar_max: dec(4000),
        };

        let cf = build_cashflow(
            budget,
            &income_txns,
            &unbudgeted_txns,
            budgeted_spent,
            &categories,
        );

        // total_in = income + other_income = 5000 + 2000
        assert_eq!(cf.total_in, dec(7000));
        // total_out = budgeted + unbudgeted = 3000 + 800
        assert_eq!(cf.total_out, dec(3800));
        // net = total_in - total_out = 7000 - 3800
        assert_eq!(cf.net_cashflow, dec(3200));
        // saved = income - budgeted - unbudgeted = 5000 - 3000 - 800
        assert_eq!(cf.saved, dec(1200));
    }

    #[test]
    fn cashflow_saved_excludes_other_income() {
        let salary_id = cat_id(10);
        let windfall_id = cat_id(20);
        let categories = vec![
            make_category(10, "Salary", BudgetConfig::Salary),
            make_category(20, "Windfall", BudgetConfig::None),
        ];

        let income_txns = vec![txn(salary_id, 5000)];
        // Large windfall should NOT inflate saved
        let unbudgeted_txns = vec![txn(windfall_id, 50000)];

        let budget = BudgetGroupSummary {
            total_budget: dec(4000),
            total_spent: dec(4000),
            remaining: dec(0),
            over_budget_count: 0,
            bar_max: dec(4000),
        };

        let cf = build_cashflow(
            budget,
            &income_txns,
            &unbudgeted_txns,
            dec(4000),
            &categories,
        );

        // Saved = salary - budgeted - unbudgeted_spending = 5000 - 4000 - 0 = 1000
        // The 50k windfall does NOT affect saved
        assert_eq!(cf.saved, dec(1000));
        // But net_cashflow includes it: 55000 - 4000 = 51000
        assert_eq!(cf.net_cashflow, dec(51000));
    }

    #[test]
    fn cashflow_income_only() {
        let salary_id = cat_id(10);
        let categories = vec![make_category(10, "Salary", BudgetConfig::Salary)];

        let income_txns = vec![txn(salary_id, 5000)];

        let budget = BudgetGroupSummary {
            total_budget: dec(0),
            total_spent: dec(0),
            remaining: dec(0),
            over_budget_count: 0,
            bar_max: dec(0),
        };

        let cf = build_cashflow(budget, &income_txns, &[], dec(0), &categories);

        assert_eq!(cf.total_in, dec(5000));
        assert_eq!(cf.total_out, dec(0));
        assert_eq!(cf.net_cashflow, dec(5000));
        assert_eq!(cf.saved, dec(5000));
    }

    #[test]
    fn cashflow_spending_only() {
        let misc_id = cat_id(30);
        let categories = vec![make_category(30, "Misc", BudgetConfig::None)];

        let unbudgeted_txns = vec![txn(misc_id, -1500)];

        let budget = BudgetGroupSummary {
            total_budget: dec(2000),
            total_spent: dec(2000),
            remaining: dec(0),
            over_budget_count: 0,
            bar_max: dec(2000),
        };

        let cf = build_cashflow(budget, &[], &unbudgeted_txns, dec(2000), &categories);

        assert_eq!(cf.total_in, dec(0));
        assert_eq!(cf.total_out, dec(3500));
        assert_eq!(cf.net_cashflow, dec(-3500));
        assert_eq!(cf.saved, dec(-3500));
    }

    #[test]
    fn cashflow_zero_amount_txns_excluded_from_split() {
        let misc_id = cat_id(30);
        let categories = vec![make_category(30, "Misc", BudgetConfig::None)];

        // Zero-amount transaction → neither positive nor negative
        let unbudgeted_txns = vec![txn(misc_id, 0)];

        let budget = BudgetGroupSummary {
            total_budget: dec(0),
            total_spent: dec(0),
            remaining: dec(0),
            over_budget_count: 0,
            bar_max: dec(0),
        };

        let cf = build_cashflow(budget, &[], &unbudgeted_txns, dec(0), &categories);

        assert_eq!(cf.other_income.total, dec(0));
        assert!(cf.other_income.items.is_empty());
        assert_eq!(cf.unbudgeted_spending.total, dec(0));
        assert!(cf.unbudgeted_spending.items.is_empty());
    }

    #[test]
    fn cashflow_budgeted_spending_has_no_items() {
        let categories = vec![make_category(10, "Salary", BudgetConfig::Salary)];

        let budget = BudgetGroupSummary {
            total_budget: dec(3000),
            total_spent: dec(2500),
            remaining: dec(500),
            over_budget_count: 0,
            bar_max: dec(3000),
        };

        let cf = build_cashflow(budget, &[], &[], dec(2500), &categories);

        // budgeted_spending is a total-only section
        assert_eq!(cf.budgeted_spending.total, dec(2500));
        assert!(cf.budgeted_spending.items.is_empty());
    }

    #[test]
    fn cashflow_budget_health_passed_through() {
        let budget = BudgetGroupSummary {
            total_budget: dec(5000),
            total_spent: dec(3000),
            remaining: dec(2000),
            over_budget_count: 1,
            bar_max: dec(5000),
        };

        let cf = build_cashflow(budget, &[], &[], dec(3000), &[]);

        assert_eq!(cf.budget.total_budget, dec(5000));
        assert_eq!(cf.budget.total_spent, dec(3000));
        assert_eq!(cf.budget.remaining, dec(2000));
        assert_eq!(cf.budget.over_budget_count, 1);
        assert_eq!(cf.budget.bar_max, dec(5000));
    }

    // ── collect_unbudgeted tests ────────────────────────────────────

    #[test]
    fn collect_unbudgeted_includes_none_mode_budget_txns() {
        let food_id = cat_id(1);
        let misc_id = cat_id(2);

        let food = make_category(
            1,
            "Food",
            BudgetConfig::Monthly {
                amount: dec(500),
                budget_type: BudgetType::Variable,
            },
        );
        let misc = make_category(2, "Misc", BudgetConfig::None);
        let categories = vec![food, misc.clone()];

        let budget_txns_raw = vec![txn(food_id, -100), txn(misc_id, -50)];
        let budget_txns: Vec<&Transaction> = budget_txns_raw.iter().collect();
        let all_txns = budget_txns_raw.clone();

        let none_cat_ids: std::collections::HashSet<CategoryId> = categories
            .iter()
            .filter(|c| effective_budget_mode(c, &categories).is_none())
            .map(|c| c.id)
            .collect();

        let result = collect_unbudgeted(&all_txns, &budget_txns, &none_cat_ids, None, None);

        assert_eq!(result.len(), 1, "only misc (None-mode) should be included");
        assert_eq!(result[0].amount, dec(-50));
    }

    #[test]
    fn collect_unbudgeted_includes_uncategorized_txns() {
        let food_id = cat_id(1);
        let budget_txns_raw = [txn(food_id, -100)];
        let budget_txns: Vec<&Transaction> = budget_txns_raw.iter().collect();

        let all_txns = vec![txn(food_id, -100), uncategorized_txn(-75)];

        let none_cat_ids = std::collections::HashSet::new();

        let result = collect_unbudgeted(&all_txns, &budget_txns, &none_cat_ids, None, None);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].amount, dec(-75));
    }

    #[test]
    fn collect_unbudgeted_excludes_correlated_uncategorized() {
        let all_txns = vec![uncategorized_txn(-100), correlated_txn(-200)];
        let budget_txns: Vec<&Transaction> = vec![];
        let none_cat_ids = std::collections::HashSet::new();

        let result = collect_unbudgeted(&all_txns, &budget_txns, &none_cat_ids, None, None);

        assert_eq!(result.len(), 1, "correlated txn should be excluded");
        assert_eq!(result[0].amount, dec(-100));
    }

    #[test]
    fn collect_unbudgeted_respects_date_range() {
        let misc_id = cat_id(2);
        let in_range = NaiveDate::from_ymd_opt(2026, 2, 15).unwrap();
        let before_range = NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();
        let after_range = NaiveDate::from_ymd_opt(2026, 3, 15).unwrap();

        let budget_txns_raw = [
            txn_on(misc_id, -100, in_range),
            txn_on(misc_id, -200, before_range),
            txn_on(misc_id, -300, after_range),
        ];
        let budget_txns: Vec<&Transaction> = budget_txns_raw.iter().collect();

        let all_txns = vec![
            uncategorized_txn_on(-50, in_range),
            uncategorized_txn_on(-75, before_range),
        ];

        let none_cat_ids: std::collections::HashSet<CategoryId> = [misc_id].into_iter().collect();

        let start = NaiveDate::from_ymd_opt(2026, 2, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2026, 2, 28).unwrap();

        let result = collect_unbudgeted(
            &all_txns,
            &budget_txns,
            &none_cat_ids,
            Some(start),
            Some(end),
        );

        assert_eq!(result.len(), 2, "in-range misc + in-range uncategorized");
        let amounts: Vec<Decimal> = result.iter().map(|t| t.amount).collect();
        assert!(amounts.contains(&dec(-100)));
        assert!(amounts.contains(&dec(-50)));
    }

    // ── build_summaries integration test ────────────────────────────

    #[test]
    fn build_summaries_full_scenario() {
        let salary_id = cat_id(10);
        let food_id = cat_id(20);
        let insurance_id = cat_id(30);
        let rsu_id = cat_id(40);
        let tax_id = cat_id(50);

        let categories = vec![
            make_category(10, "Salary", BudgetConfig::Salary),
            make_category(
                20,
                "Food",
                BudgetConfig::Monthly {
                    amount: dec(1000),
                    budget_type: BudgetType::Variable,
                },
            ),
            make_category(
                30,
                "Insurance",
                BudgetConfig::Annual {
                    amount: dec(3000),
                    budget_type: BudgetType::Fixed,
                },
            ),
            make_category(40, "RSU", BudgetConfig::None),
            make_category(50, "Tax", BudgetConfig::None),
        ];

        let month = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: NaiveDate::from_ymd_opt(2026, 2, 1).unwrap(),
            end_date: Some(NaiveDate::from_ymd_opt(2026, 2, 28).unwrap()),
            salary_transactions_detected: 1,
        };

        // Statuses: food is monthly, insurance is annual
        let food_status = make_status(20, "Food", dec(1000), dec(600), PaceIndicator::OnTrack);
        let mut food_entry = make_entry(food_status, vec![]);
        food_entry.status.budget_mode = BudgetMode::Monthly;

        let insurance_status = make_status(
            30,
            "Insurance",
            dec(3000),
            dec(1200),
            PaceIndicator::UnderBudget,
        );
        let mut insurance_entry = make_entry(insurance_status, vec![]);
        insurance_entry.status.budget_mode = BudgetMode::Annual;

        let statuses = vec![food_entry, insurance_entry];

        // Classified transactions
        let classified = ClassifiedTransactions {
            monthly: vec![txn(food_id, -600)],
            annual: vec![txn(insurance_id, -1200)],
            project: vec![],
            unbudgeted: vec![
                txn(rsu_id, 10000), // positive → other_income
                txn(tax_id, -500),  // negative → unbudgeted_spending
            ],
            unbudgeted_annual: vec![
                txn(rsu_id, 10000),
                txn(rsu_id, 20000),
                txn(tax_id, -500),
                txn(tax_id, -1000),
            ],
            budget_year: 2026,
            income: vec![txn(salary_id, 5000)],
            annual_income: vec![txn(salary_id, 5000), txn(salary_id, 5000)],
        };

        let projects: Vec<ProjectStatusEntry> = vec![];

        let (monthly_cf, annual_cf, project_sum) =
            build_summaries(&statuses, &classified, &categories, &month, &projects);

        // Monthly cash flow
        assert_eq!(monthly_cf.income.total, dec(5000));
        assert_eq!(monthly_cf.other_income.total, dec(10000));
        assert_eq!(monthly_cf.budgeted_spending.total, dec(600)); // negate_sum of monthly txns
        assert_eq!(monthly_cf.unbudgeted_spending.total, dec(500));
        assert_eq!(monthly_cf.total_in, dec(15000));
        assert_eq!(monthly_cf.total_out, dec(1100));
        assert_eq!(monthly_cf.net_cashflow, dec(13900));
        assert_eq!(monthly_cf.saved, dec(3900)); // 5000 - 600 - 500

        // Annual cash flow
        assert_eq!(annual_cf.income.total, dec(10000));
        assert_eq!(annual_cf.other_income.total, dec(30000));
        assert_eq!(annual_cf.budgeted_spending.total, dec(1200));
        assert_eq!(annual_cf.unbudgeted_spending.total, dec(1500));
        assert_eq!(annual_cf.total_in, dec(40000));
        assert_eq!(annual_cf.total_out, dec(2700));
        assert_eq!(annual_cf.net_cashflow, dec(37300));
        assert_eq!(annual_cf.saved, dec(7300)); // 10000 - 1200 - 1500

        // Project summary
        assert_eq!(project_sum.total_budget, dec(0));
        assert_eq!(project_sum.total_spent, dec(0));
    }

    #[test]
    fn build_summaries_includes_project_txns_in_monthly_budgeted() {
        let salary_id = cat_id(10);
        let food_id = cat_id(20);
        let reno_id = cat_id(60);

        let categories = vec![
            make_category(10, "Salary", BudgetConfig::Salary),
            make_category(
                20,
                "Food",
                BudgetConfig::Monthly {
                    amount: dec(1000),
                    budget_type: BudgetType::Variable,
                },
            ),
            make_category(
                60,
                "Renovation",
                BudgetConfig::Project {
                    amount: dec(5000),
                    start_date: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
                    end_date: None,
                },
            ),
        ];

        let month = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: NaiveDate::from_ymd_opt(2026, 2, 1).unwrap(),
            end_date: Some(NaiveDate::from_ymd_opt(2026, 2, 28).unwrap()),
            salary_transactions_detected: 1,
        };

        let food_status = make_status(20, "Food", dec(1000), dec(400), PaceIndicator::OnTrack);
        let mut food_entry = make_entry(food_status, vec![]);
        food_entry.status.budget_mode = BudgetMode::Monthly;
        let statuses = vec![food_entry];

        // Project transaction in the current month counts towards monthly budgeted spending
        let project_txn = txn_on(reno_id, -800, NaiveDate::from_ymd_opt(2026, 2, 10).unwrap());

        let classified = ClassifiedTransactions {
            monthly: vec![txn(food_id, -400)],
            annual: vec![],
            project: vec![project_txn],
            unbudgeted: vec![],
            unbudgeted_annual: vec![],
            budget_year: 2026,
            income: vec![txn(salary_id, 5000)],
            annual_income: vec![txn(salary_id, 5000)],
        };

        let projects: Vec<ProjectStatusEntry> = vec![];

        let (monthly_cf, _, _) =
            build_summaries(&statuses, &classified, &categories, &month, &projects);

        // Monthly budgeted_spending = food (400) + project-in-month (800) = 1200
        assert_eq!(monthly_cf.budgeted_spending.total, dec(1200));
        assert_eq!(monthly_cf.saved, dec(3800)); // 5000 - 1200 - 0
    }

    #[test]
    fn build_summaries_empty_month() {
        let categories: Vec<Category> = vec![];

        let month = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: NaiveDate::from_ymd_opt(2026, 2, 1).unwrap(),
            end_date: Some(NaiveDate::from_ymd_opt(2026, 2, 28).unwrap()),
            salary_transactions_detected: 1,
        };

        let statuses: Vec<StatusEntry> = vec![];
        let classified = ClassifiedTransactions {
            monthly: vec![],
            annual: vec![],
            project: vec![],
            unbudgeted: vec![],
            unbudgeted_annual: vec![],
            budget_year: 2026,
            income: vec![],
            annual_income: vec![],
        };
        let projects: Vec<ProjectStatusEntry> = vec![];

        let (monthly_cf, annual_cf, project_sum) =
            build_summaries(&statuses, &classified, &categories, &month, &projects);

        assert_eq!(monthly_cf.total_in, dec(0));
        assert_eq!(monthly_cf.total_out, dec(0));
        assert_eq!(monthly_cf.net_cashflow, dec(0));
        assert_eq!(monthly_cf.saved, dec(0));
        assert_eq!(annual_cf.total_in, dec(0));
        assert_eq!(annual_cf.total_out, dec(0));
        assert_eq!(project_sum.total_budget, dec(0));
    }

    // ── classify_transactions + cashflow end-to-end ─────────────────

    #[test]
    fn classify_unbudgeted_split_correctly_for_cashflow() {
        let salary_id = cat_id(10);
        let rsu_id = cat_id(20);
        let tax_id = cat_id(30);
        let food_id = cat_id(40);

        let categories = vec![
            make_category(10, "Salary", BudgetConfig::Salary),
            make_category(20, "RSU", BudgetConfig::None),
            make_category(30, "Tax", BudgetConfig::None),
            make_category(
                40,
                "Food",
                BudgetConfig::Monthly {
                    amount: dec(500),
                    budget_type: BudgetType::Variable,
                },
            ),
        ];

        let month = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: NaiveDate::from_ymd_opt(2026, 2, 1).unwrap(),
            end_date: Some(NaiveDate::from_ymd_opt(2026, 2, 28).unwrap()),
            salary_transactions_detected: 1,
        };
        let year_months: Vec<&BudgetMonth> = vec![&month];

        let transactions = vec![
            txn_on(
                salary_id,
                5000,
                NaiveDate::from_ymd_opt(2026, 2, 5).unwrap(),
            ),
            txn_on(food_id, -300, NaiveDate::from_ymd_opt(2026, 2, 10).unwrap()),
            // RSU is None-mode, positive → should end up in unbudgeted as positive
            txn_on(rsu_id, 15000, NaiveDate::from_ymd_opt(2026, 2, 12).unwrap()),
            // Tax is None-mode, negative → should end up in unbudgeted as negative
            txn_on(tax_id, -2000, NaiveDate::from_ymd_opt(2026, 2, 20).unwrap()),
            // Uncategorized negative → unbudgeted
            uncategorized_txn_on(-500, NaiveDate::from_ymd_opt(2026, 2, 22).unwrap()),
        ];

        let classified = classify_transactions(&transactions, &categories, &month, &year_months);

        // Unbudgeted should contain: RSU (+15000), Tax (-2000), uncategorized (-500)
        assert_eq!(classified.unbudgeted.len(), 3);

        let positive: Vec<&Transaction> = classified
            .unbudgeted
            .iter()
            .filter(|t| t.amount > Decimal::ZERO)
            .collect();
        let negative: Vec<&Transaction> = classified
            .unbudgeted
            .iter()
            .filter(|t| t.amount < Decimal::ZERO)
            .collect();

        assert_eq!(positive.len(), 1);
        assert_eq!(positive[0].amount, dec(15000));
        assert_eq!(negative.len(), 2);

        // When fed to build_cashflow, the split happens correctly
        let food_status = make_status(40, "Food", dec(500), dec(300), PaceIndicator::OnTrack);
        let mut food_entry = make_entry(food_status, vec![]);
        food_entry.status.budget_mode = BudgetMode::Monthly;

        let statuses = vec![food_entry];
        let projects: Vec<ProjectStatusEntry> = vec![];

        let (monthly_cf, _, _) =
            build_summaries(&statuses, &classified, &categories, &month, &projects);

        assert_eq!(monthly_cf.income.total, dec(5000));
        assert_eq!(monthly_cf.other_income.total, dec(15000));
        assert_eq!(monthly_cf.unbudgeted_spending.total, dec(2500)); // 2000 + 500
    }

    #[test]
    fn cashflow_other_income_groups_by_category() {
        let rsu_id = cat_id(20);
        let car_id = cat_id(21);
        let categories = vec![
            make_category(20, "RSU", BudgetConfig::None),
            make_category(21, "Car Sale", BudgetConfig::None),
        ];

        let unbudgeted_txns = vec![txn(rsu_id, 59000), txn(car_id, 15000)];

        let budget = BudgetGroupSummary {
            total_budget: dec(0),
            total_spent: dec(0),
            remaining: dec(0),
            over_budget_count: 0,
            bar_max: dec(0),
        };

        let cf = build_cashflow(budget, &[], &unbudgeted_txns, dec(0), &categories);

        assert_eq!(cf.other_income.items.len(), 2);
        // Sorted by amount descending
        assert_eq!(cf.other_income.items[0].label, "RSU");
        assert_eq!(cf.other_income.items[0].amount, dec(59000));
        assert_eq!(cf.other_income.items[1].label, "Car Sale");
        assert_eq!(cf.other_income.items[1].amount, dec(15000));
        assert_eq!(cf.other_income.total, dec(74000));
    }

    #[test]
    fn cashflow_unbudgeted_spending_groups_by_category() {
        let tax_id = cat_id(30);
        let categories = vec![make_category(30, "Tax", BudgetConfig::None)];

        let unbudgeted_txns = vec![
            txn(tax_id, -2000),
            txn(tax_id, -500),
            uncategorized_txn(-800),
        ];

        let budget = BudgetGroupSummary {
            total_budget: dec(0),
            total_spent: dec(0),
            remaining: dec(0),
            over_budget_count: 0,
            bar_max: dec(0),
        };

        let cf = build_cashflow(budget, &[], &unbudgeted_txns, dec(0), &categories);

        assert_eq!(cf.unbudgeted_spending.items.len(), 2);
        // Tax: |-2000 + -500| = 2500 (sorted first)
        assert_eq!(cf.unbudgeted_spending.items[0].label, "Tax");
        assert_eq!(cf.unbudgeted_spending.items[0].amount, dec(2500));
        assert_eq!(cf.unbudgeted_spending.items[0].transaction_count, 2);
        // Uncategorized: 800
        assert_eq!(cf.unbudgeted_spending.items[1].label, "Uncategorized");
        assert_eq!(cf.unbudgeted_spending.items[1].amount, dec(800));
        assert_eq!(cf.unbudgeted_spending.total, dec(3300));
    }

    // ── negate_sum tests ──────────────────────────────────────────────

    #[test]
    fn negate_sum_positive_transactions_give_negative() {
        let txns = vec![txn(cat_id(1), 100), txn(cat_id(1), 200)];
        assert_eq!(negate_sum(&txns), dec(-300));
    }

    #[test]
    fn negate_sum_negative_transactions_give_positive() {
        let txns = vec![txn(cat_id(1), -100), txn(cat_id(1), -200)];
        assert_eq!(negate_sum(&txns), dec(300));
    }

    #[test]
    fn negate_sum_empty() {
        assert_eq!(negate_sum(&[]), dec(0));
    }

    #[test]
    fn negate_sum_mixed_signs() {
        let txns = vec![txn(cat_id(1), 100), txn(cat_id(1), -300)];
        // sum = 100 + (-300) = -200, negated = 200
        assert_eq!(negate_sum(&txns), dec(200));
    }

    // ── is_in_date_range tests ────────────────────────────────────────

    #[test]
    fn date_range_both_bounds() {
        let start = NaiveDate::from_ymd_opt(2026, 2, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2026, 2, 28).unwrap();

        assert!(is_in_date_range(start, Some(start), Some(end)));
        assert!(is_in_date_range(end, Some(start), Some(end)));
        assert!(is_in_date_range(
            NaiveDate::from_ymd_opt(2026, 2, 15).unwrap(),
            Some(start),
            Some(end),
        ));
        assert!(!is_in_date_range(
            NaiveDate::from_ymd_opt(2026, 1, 31).unwrap(),
            Some(start),
            Some(end),
        ));
        assert!(!is_in_date_range(
            NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            Some(start),
            Some(end),
        ));
    }

    #[test]
    fn date_range_no_bounds() {
        let any_date = NaiveDate::from_ymd_opt(2020, 6, 15).unwrap();
        assert!(is_in_date_range(any_date, None, None));
    }

    #[test]
    fn date_range_only_start() {
        let start = NaiveDate::from_ymd_opt(2026, 2, 1).unwrap();
        assert!(is_in_date_range(start, Some(start), None));
        assert!(is_in_date_range(
            NaiveDate::from_ymd_opt(2099, 12, 31).unwrap(),
            Some(start),
            None,
        ));
        assert!(!is_in_date_range(
            NaiveDate::from_ymd_opt(2026, 1, 31).unwrap(),
            Some(start),
            None,
        ));
    }

    #[test]
    fn date_range_only_end() {
        let end = NaiveDate::from_ymd_opt(2026, 2, 28).unwrap();
        assert!(is_in_date_range(end, None, Some(end)));
        assert!(is_in_date_range(
            NaiveDate::from_ymd_opt(2000, 1, 1).unwrap(),
            None,
            Some(end),
        ));
        assert!(!is_in_date_range(
            NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            None,
            Some(end),
        ));
    }

    // ── collect_income_transactions tests ─────────────────────────────

    #[test]
    fn collect_income_filters_by_salary_category() {
        let salary_id = cat_id(10);
        let food_id = cat_id(20);
        let salary_cats: std::collections::HashSet<CategoryId> = [salary_id].into_iter().collect();

        let transactions = vec![txn(salary_id, 5000), txn(food_id, 50), txn(salary_id, 500)];

        let result = collect_income_transactions(&transactions, &salary_cats, None, None);
        assert_eq!(result.len(), 2);
        let total: Decimal = result.iter().map(|t| t.amount).sum();
        assert_eq!(total, dec(5500));
    }

    #[test]
    fn collect_income_excludes_negative_salary() {
        let salary_id = cat_id(10);
        let salary_cats: std::collections::HashSet<CategoryId> = [salary_id].into_iter().collect();

        let transactions = vec![
            txn(salary_id, 5000),
            txn(salary_id, -200), // refund — excluded
        ];

        let result = collect_income_transactions(&transactions, &salary_cats, None, None);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].amount, dec(5000));
    }

    #[test]
    fn collect_income_respects_date_range() {
        let salary_id = cat_id(10);
        let salary_cats: std::collections::HashSet<CategoryId> = [salary_id].into_iter().collect();

        let start = NaiveDate::from_ymd_opt(2026, 2, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2026, 2, 28).unwrap();

        let transactions = vec![
            txn_on(
                salary_id,
                5000,
                NaiveDate::from_ymd_opt(2026, 2, 15).unwrap(),
            ),
            txn_on(
                salary_id,
                5000,
                NaiveDate::from_ymd_opt(2026, 1, 15).unwrap(),
            ), // before
            txn_on(
                salary_id,
                5000,
                NaiveDate::from_ymd_opt(2026, 3, 15).unwrap(),
            ), // after
        ];

        let result =
            collect_income_transactions(&transactions, &salary_cats, Some(start), Some(end));
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn collect_income_empty_salary_cats() {
        let salary_cats = std::collections::HashSet::new();
        let transactions = vec![txn(cat_id(10), 5000)];

        let result = collect_income_transactions(&transactions, &salary_cats, None, None);
        assert!(result.is_empty());
    }

    // ── group_by_category edge cases ──────────────────────────────────

    #[test]
    fn group_by_category_unknown_category_uses_id() {
        // Transaction references a category not in the categories list
        let unknown_id = cat_id(999);
        let categories = vec![make_category(1, "Food", BudgetConfig::None)];

        let transactions = vec![txn(unknown_id, -100)];

        let items = group_by_category(&transactions, &categories);

        assert_eq!(items.len(), 1);
        // category_id should still be set even if we can't look up the name
        assert_eq!(items[0].category_id, Some(unknown_id));
        assert_eq!(items[0].amount, dec(100));
    }

    #[test]
    fn group_by_category_single_transaction_per_category() {
        let a = cat_id(1);
        let b = cat_id(2);
        let c = cat_id(3);
        let categories = vec![
            make_category(1, "Alpha", BudgetConfig::None),
            make_category(2, "Beta", BudgetConfig::None),
            make_category(3, "Gamma", BudgetConfig::None),
        ];

        let transactions = vec![txn(a, -100), txn(b, -200), txn(c, -50)];

        let items = group_by_category(&transactions, &categories);

        assert_eq!(items.len(), 3);
        // Sorted by amount descending
        assert_eq!(items[0].label, "Beta");
        assert_eq!(items[0].amount, dec(200));
        assert_eq!(items[1].label, "Alpha");
        assert_eq!(items[1].amount, dec(100));
        assert_eq!(items[2].label, "Gamma");
        assert_eq!(items[2].amount, dec(50));
    }

    // ── build_cashflow edge cases ─────────────────────────────────────

    #[test]
    fn cashflow_multiple_income_categories() {
        let salary_id = cat_id(10);
        let bonus_id = cat_id(11);
        let categories = vec![
            make_category(10, "Salary", BudgetConfig::Salary),
            make_category(11, "Bonus", BudgetConfig::None),
        ];

        // Both are income transactions (positive salary-category)
        let income_txns = vec![txn(salary_id, 5000), txn(bonus_id, 1000)];

        let budget = BudgetGroupSummary {
            total_budget: dec(0),
            total_spent: dec(0),
            remaining: dec(0),
            over_budget_count: 0,
            bar_max: dec(0),
        };

        let cf = build_cashflow(budget, &income_txns, &[], dec(0), &categories);

        assert_eq!(cf.income.total, dec(6000));
        assert_eq!(cf.income.items.len(), 2);
        // Sorted by amount descending
        assert_eq!(cf.income.items[0].label, "Salary");
        assert_eq!(cf.income.items[0].amount, dec(5000));
        assert_eq!(cf.income.items[1].label, "Bonus");
        assert_eq!(cf.income.items[1].amount, dec(1000));
    }

    #[test]
    fn cashflow_all_sections_populated() {
        let salary_id = cat_id(10);
        let rsu_id = cat_id(20);
        let tax_id = cat_id(30);
        let categories = vec![
            make_category(10, "Salary", BudgetConfig::Salary),
            make_category(20, "RSU", BudgetConfig::None),
            make_category(30, "Tax", BudgetConfig::None),
        ];

        let income_txns = vec![txn(salary_id, 8000)];
        let unbudgeted_txns = vec![
            txn(rsu_id, 20000),
            txn(tax_id, -3000),
            uncategorized_txn(-1000),
        ];

        let budget = BudgetGroupSummary {
            total_budget: dec(5000),
            total_spent: dec(4500),
            remaining: dec(500),
            over_budget_count: 0,
            bar_max: dec(5000),
        };

        let cf = build_cashflow(
            budget,
            &income_txns,
            &unbudgeted_txns,
            dec(4500),
            &categories,
        );

        // Income
        assert_eq!(cf.income.total, dec(8000));
        assert_eq!(cf.income.items.len(), 1);

        // Other income (positive unbudgeted)
        assert_eq!(cf.other_income.total, dec(20000));
        assert_eq!(cf.other_income.items.len(), 1);
        assert_eq!(cf.other_income.items[0].label, "RSU");

        // Budgeted spending (total-only, no items)
        assert_eq!(cf.budgeted_spending.total, dec(4500));
        assert!(cf.budgeted_spending.items.is_empty());

        // Unbudgeted spending (negative unbudgeted)
        assert_eq!(cf.unbudgeted_spending.total, dec(4000));
        assert_eq!(cf.unbudgeted_spending.items.len(), 2);

        // Totals
        assert_eq!(cf.total_in, dec(28000));
        assert_eq!(cf.total_out, dec(8500));
        assert_eq!(cf.net_cashflow, dec(19500));
        assert_eq!(cf.saved, dec(-500)); // 8000 - 4500 - 4000 = -500
    }

    #[test]
    fn cashflow_negative_saved_when_overspent() {
        let salary_id = cat_id(10);
        let categories = vec![make_category(10, "Salary", BudgetConfig::Salary)];

        let income_txns = vec![txn(salary_id, 3000)];

        let budget = BudgetGroupSummary {
            total_budget: dec(4000),
            total_spent: dec(4000),
            remaining: dec(0),
            over_budget_count: 0,
            bar_max: dec(4000),
        };

        let cf = build_cashflow(budget, &income_txns, &[], dec(4000), &categories);

        // saved = income - budgeted_spending - unbudgeted_spending = 3000 - 4000 - 0 = -1000
        assert_eq!(cf.saved, dec(-1000));
        // net also negative
        assert_eq!(cf.net_cashflow, dec(-1000));
    }

    // ── build_summaries edge cases ────────────────────────────────────

    #[test]
    fn build_summaries_project_txns_outside_month_not_counted() {
        let salary_id = cat_id(10);
        let food_id = cat_id(20);
        let reno_id = cat_id(60);

        let categories = vec![
            make_category(10, "Salary", BudgetConfig::Salary),
            make_category(
                20,
                "Food",
                BudgetConfig::Monthly {
                    amount: dec(1000),
                    budget_type: BudgetType::Variable,
                },
            ),
            make_category(
                60,
                "Renovation",
                BudgetConfig::Project {
                    amount: dec(5000),
                    start_date: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
                    end_date: None,
                },
            ),
        ];

        let month = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: NaiveDate::from_ymd_opt(2026, 2, 1).unwrap(),
            end_date: Some(NaiveDate::from_ymd_opt(2026, 2, 28).unwrap()),
            salary_transactions_detected: 1,
        };

        let food_status = make_status(20, "Food", dec(1000), dec(400), PaceIndicator::OnTrack);
        let mut food_entry = make_entry(food_status, vec![]);
        food_entry.status.budget_mode = BudgetMode::Monthly;
        let statuses = vec![food_entry];

        // Project transaction OUTSIDE current month — should NOT count in monthly budgeted
        let project_txn_outside =
            txn_on(reno_id, -800, NaiveDate::from_ymd_opt(2026, 1, 15).unwrap());

        let classified = ClassifiedTransactions {
            monthly: vec![txn(food_id, -400)],
            annual: vec![],
            project: vec![project_txn_outside],
            unbudgeted: vec![],
            unbudgeted_annual: vec![],
            budget_year: 2026,
            income: vec![txn(salary_id, 5000)],
            annual_income: vec![txn(salary_id, 5000)],
        };

        let projects: Vec<ProjectStatusEntry> = vec![];

        let (monthly_cf, _, _) =
            build_summaries(&statuses, &classified, &categories, &month, &projects);

        // Only food spending (400), project txn is outside month
        assert_eq!(monthly_cf.budgeted_spending.total, dec(400));
        assert_eq!(monthly_cf.saved, dec(4600)); // 5000 - 400
    }

    #[test]
    fn build_summaries_multiple_monthly_and_annual_categories() {
        let salary_id = cat_id(10);
        let food_id = cat_id(20);
        let transport_id = cat_id(21);
        let insurance_id = cat_id(30);
        let subscription_id = cat_id(31);

        let categories = vec![
            make_category(10, "Salary", BudgetConfig::Salary),
            make_category(
                20,
                "Food",
                BudgetConfig::Monthly {
                    amount: dec(1000),
                    budget_type: BudgetType::Variable,
                },
            ),
            make_category(
                21,
                "Transport",
                BudgetConfig::Monthly {
                    amount: dec(200),
                    budget_type: BudgetType::Variable,
                },
            ),
            make_category(
                30,
                "Insurance",
                BudgetConfig::Annual {
                    amount: dec(3000),
                    budget_type: BudgetType::Fixed,
                },
            ),
            make_category(
                31,
                "Subscriptions",
                BudgetConfig::Annual {
                    amount: dec(600),
                    budget_type: BudgetType::Fixed,
                },
            ),
        ];

        let month = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: NaiveDate::from_ymd_opt(2026, 2, 1).unwrap(),
            end_date: Some(NaiveDate::from_ymd_opt(2026, 2, 28).unwrap()),
            salary_transactions_detected: 1,
        };

        let food_status = make_status(20, "Food", dec(1000), dec(800), PaceIndicator::OnTrack);
        let mut food_entry = make_entry(food_status, vec![]);
        food_entry.status.budget_mode = BudgetMode::Monthly;

        let transport_status =
            make_status(21, "Transport", dec(200), dec(150), PaceIndicator::OnTrack);
        let mut transport_entry = make_entry(transport_status, vec![]);
        transport_entry.status.budget_mode = BudgetMode::Monthly;

        let insurance_status = make_status(
            30,
            "Insurance",
            dec(3000),
            dec(1200),
            PaceIndicator::UnderBudget,
        );
        let mut insurance_entry = make_entry(insurance_status, vec![]);
        insurance_entry.status.budget_mode = BudgetMode::Annual;

        let sub_status = make_status(
            31,
            "Subscriptions",
            dec(600),
            dec(100),
            PaceIndicator::UnderBudget,
        );
        let mut sub_entry = make_entry(sub_status, vec![]);
        sub_entry.status.budget_mode = BudgetMode::Annual;

        let statuses = vec![food_entry, transport_entry, insurance_entry, sub_entry];

        let classified = ClassifiedTransactions {
            monthly: vec![txn(food_id, -800), txn(transport_id, -150)],
            annual: vec![txn(insurance_id, -1200), txn(subscription_id, -100)],
            project: vec![],
            unbudgeted: vec![],
            unbudgeted_annual: vec![],
            budget_year: 2026,
            income: vec![txn(salary_id, 5000)],
            annual_income: vec![txn(salary_id, 5000), txn(salary_id, 5000)],
        };

        let projects: Vec<ProjectStatusEntry> = vec![];

        let (monthly_cf, annual_cf, _) =
            build_summaries(&statuses, &classified, &categories, &month, &projects);

        // Monthly: budget health covers food + transport
        assert_eq!(monthly_cf.budget.total_budget, dec(1200));
        assert_eq!(monthly_cf.budgeted_spending.total, dec(950)); // 800 + 150

        // Annual: budget health covers insurance + subscriptions
        assert_eq!(annual_cf.budget.total_budget, dec(3600));
        assert_eq!(annual_cf.budgeted_spending.total, dec(1300)); // 1200 + 100
    }

    // ── classify_transactions edge cases ──────────────────────────────

    #[test]
    fn classify_salary_children_not_in_unbudgeted() {
        let salary_root_id = cat_id(10);
        let salary_child_id = cat_id(11);

        let salary_root = Category {
            id: salary_root_id,
            name: CategoryName::new("Salary").unwrap(),
            parent_id: None,
            budget: BudgetConfig::Salary,
        };
        let salary_child = Category {
            id: salary_child_id,
            name: CategoryName::new("Bonus").unwrap(),
            parent_id: Some(salary_root_id),
            budget: BudgetConfig::None,
        };
        let categories = vec![salary_root, salary_child];

        let month = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: NaiveDate::from_ymd_opt(2026, 2, 1).unwrap(),
            end_date: Some(NaiveDate::from_ymd_opt(2026, 2, 28).unwrap()),
            salary_transactions_detected: 1,
        };
        let year_months: Vec<&BudgetMonth> = vec![&month];

        let transactions = vec![
            txn_on(
                salary_root_id,
                5000,
                NaiveDate::from_ymd_opt(2026, 2, 5).unwrap(),
            ),
            txn_on(
                salary_child_id,
                1000,
                NaiveDate::from_ymd_opt(2026, 2, 10).unwrap(),
            ),
        ];

        let classified = classify_transactions(&transactions, &categories, &month, &year_months);

        // Salary child has BudgetConfig::None but is a child of Salary,
        // so it should be categorized as salary (via filter_for_budget),
        // NOT as unbudgeted.
        // Income should capture both salary root and child.
        assert_eq!(classified.income.len(), 2);
    }

    #[test]
    fn classify_uncategorized_positive_goes_to_unbudgeted() {
        let food_id = cat_id(20);
        let categories = vec![make_category(
            20,
            "Food",
            BudgetConfig::Monthly {
                amount: dec(500),
                budget_type: BudgetType::Variable,
            },
        )];

        let month = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: NaiveDate::from_ymd_opt(2026, 2, 1).unwrap(),
            end_date: Some(NaiveDate::from_ymd_opt(2026, 2, 28).unwrap()),
            salary_transactions_detected: 1,
        };
        let year_months: Vec<&BudgetMonth> = vec![&month];

        let transactions = vec![
            txn_on(food_id, -300, NaiveDate::from_ymd_opt(2026, 2, 10).unwrap()),
            // Positive uncategorized → should be in unbudgeted (then split to other_income)
            uncategorized_txn_on(500, NaiveDate::from_ymd_opt(2026, 2, 15).unwrap()),
            // Negative uncategorized → should be in unbudgeted (then split to unbudgeted_spending)
            uncategorized_txn_on(-200, NaiveDate::from_ymd_opt(2026, 2, 20).unwrap()),
        ];

        let classified = classify_transactions(&transactions, &categories, &month, &year_months);

        assert_eq!(classified.unbudgeted.len(), 2);
        let positive: Vec<&Transaction> = classified
            .unbudgeted
            .iter()
            .filter(|t| t.amount > Decimal::ZERO)
            .collect();
        let negative: Vec<&Transaction> = classified
            .unbudgeted
            .iter()
            .filter(|t| t.amount < Decimal::ZERO)
            .collect();
        assert_eq!(positive.len(), 1);
        assert_eq!(positive[0].amount, dec(500));
        assert_eq!(negative.len(), 1);
        assert_eq!(negative[0].amount, dec(-200));
    }

    #[test]
    fn classify_correlated_uncategorized_excluded_from_unbudgeted() {
        let categories: Vec<Category> = vec![];

        let month = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: NaiveDate::from_ymd_opt(2026, 2, 1).unwrap(),
            end_date: Some(NaiveDate::from_ymd_opt(2026, 2, 28).unwrap()),
            salary_transactions_detected: 1,
        };
        let year_months: Vec<&BudgetMonth> = vec![&month];

        let transactions = vec![
            uncategorized_txn(-100),
            correlated_txn(-500), // transfer — should be excluded
        ];

        let classified = classify_transactions(&transactions, &categories, &month, &year_months);

        assert_eq!(classified.unbudgeted.len(), 1);
        assert_eq!(classified.unbudgeted[0].amount, dec(-100));
    }

    // ── collect_unbudgeted edge cases ──────────────────────────────────

    #[test]
    fn collect_unbudgeted_both_none_mode_and_uncategorized_in_range() {
        let misc_id = cat_id(2);
        let in_range = NaiveDate::from_ymd_opt(2026, 2, 15).unwrap();

        let budget_txns_raw = [txn_on(misc_id, -100, in_range)];
        let budget_txns: Vec<&Transaction> = budget_txns_raw.iter().collect();

        let all_txns = vec![
            txn_on(misc_id, -100, in_range),
            uncategorized_txn_on(-75, in_range),
        ];

        let none_cat_ids: std::collections::HashSet<CategoryId> = [misc_id].into_iter().collect();

        let start = NaiveDate::from_ymd_opt(2026, 2, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2026, 2, 28).unwrap();

        let result = collect_unbudgeted(
            &all_txns,
            &budget_txns,
            &none_cat_ids,
            Some(start),
            Some(end),
        );

        // Should include both: None-mode budget txn + uncategorized
        assert_eq!(result.len(), 2);
        let amounts: Vec<Decimal> = result.iter().map(|t| t.amount).collect();
        assert!(amounts.contains(&dec(-100)));
        assert!(amounts.contains(&dec(-75)));
    }

    #[test]
    fn collect_unbudgeted_boundary_dates_included() {
        let misc_id = cat_id(2);
        let start = NaiveDate::from_ymd_opt(2026, 2, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2026, 2, 28).unwrap();

        // Transactions exactly on boundary dates
        let budget_txns_raw = [txn_on(misc_id, -100, start), txn_on(misc_id, -200, end)];
        let budget_txns: Vec<&Transaction> = budget_txns_raw.iter().collect();

        let all_txns = vec![
            uncategorized_txn_on(-50, start),
            uncategorized_txn_on(-75, end),
        ];

        let none_cat_ids: std::collections::HashSet<CategoryId> = [misc_id].into_iter().collect();

        let result = collect_unbudgeted(
            &all_txns,
            &budget_txns,
            &none_cat_ids,
            Some(start),
            Some(end),
        );

        assert_eq!(result.len(), 4, "boundary dates should be inclusive");
    }

    // ── end-to-end integration: classify → build_summaries ───────────

    #[test]
    fn end_to_end_classify_and_cashflow_with_all_transaction_types() {
        let salary_id = cat_id(10);
        let food_id = cat_id(20);
        let rsu_id = cat_id(40);
        let tax_id = cat_id(50);

        let categories = vec![
            make_category(10, "Salary", BudgetConfig::Salary),
            make_category(
                20,
                "Food",
                BudgetConfig::Monthly {
                    amount: dec(1000),
                    budget_type: BudgetType::Variable,
                },
            ),
            make_category(40, "RSU", BudgetConfig::None),
            make_category(50, "Tax", BudgetConfig::None),
        ];

        let month = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: NaiveDate::from_ymd_opt(2026, 2, 1).unwrap(),
            end_date: Some(NaiveDate::from_ymd_opt(2026, 2, 28).unwrap()),
            salary_transactions_detected: 1,
        };
        let year_months: Vec<&BudgetMonth> = vec![&month];

        let transactions = vec![
            // Salary income
            txn_on(
                salary_id,
                5000,
                NaiveDate::from_ymd_opt(2026, 2, 5).unwrap(),
            ),
            // Budgeted food spending
            txn_on(food_id, -700, NaiveDate::from_ymd_opt(2026, 2, 10).unwrap()),
            // Positive unbudgeted (RSU vest) → other_income
            txn_on(rsu_id, 25000, NaiveDate::from_ymd_opt(2026, 2, 12).unwrap()),
            // Negative unbudgeted (tax) → unbudgeted_spending
            txn_on(tax_id, -3000, NaiveDate::from_ymd_opt(2026, 2, 15).unwrap()),
            // Positive uncategorized → other_income
            uncategorized_txn_on(200, NaiveDate::from_ymd_opt(2026, 2, 18).unwrap()),
            // Negative uncategorized → unbudgeted_spending
            uncategorized_txn_on(-150, NaiveDate::from_ymd_opt(2026, 2, 20).unwrap()),
        ];

        let classified = classify_transactions(&transactions, &categories, &month, &year_months);

        // Verify classification
        assert_eq!(classified.monthly.len(), 1); // food
        assert_eq!(classified.income.len(), 1); // salary
        // Unbudgeted: RSU, Tax, +uncategorized, -uncategorized
        assert_eq!(classified.unbudgeted.len(), 4);

        // Build statuses
        let food_status = make_status(20, "Food", dec(1000), dec(700), PaceIndicator::OnTrack);
        let mut food_entry = make_entry(food_status, vec![]);
        food_entry.status.budget_mode = BudgetMode::Monthly;
        let statuses = vec![food_entry];

        let projects: Vec<ProjectStatusEntry> = vec![];

        let (monthly_cf, _, _) =
            build_summaries(&statuses, &classified, &categories, &month, &projects);

        // Income: salary only
        assert_eq!(monthly_cf.income.total, dec(5000));
        assert_eq!(monthly_cf.income.items.len(), 1);

        // Other income: RSU (25000) + uncategorized positive (200) = 25200
        assert_eq!(monthly_cf.other_income.total, dec(25200));
        assert_eq!(monthly_cf.other_income.items.len(), 2);

        // Budgeted spending: food = 700
        assert_eq!(monthly_cf.budgeted_spending.total, dec(700));

        // Unbudgeted spending: tax (3000) + uncategorized negative (150) = 3150
        assert_eq!(monthly_cf.unbudgeted_spending.total, dec(3150));
        assert_eq!(monthly_cf.unbudgeted_spending.items.len(), 2);

        // Computed totals
        assert_eq!(monthly_cf.total_in, dec(30200)); // 5000 + 25200
        assert_eq!(monthly_cf.total_out, dec(3850)); // 700 + 3150
        assert_eq!(monthly_cf.net_cashflow, dec(26350)); // 30200 - 3850
        assert_eq!(monthly_cf.saved, dec(1150)); // 5000 - 700 - 3150
    }

    #[test]
    fn end_to_end_annual_cashflow_spans_multiple_months() {
        let salary_id = cat_id(10);
        let insurance_id = cat_id(30);
        let rsu_id = cat_id(40);

        let categories = vec![
            make_category(10, "Salary", BudgetConfig::Salary),
            make_category(
                30,
                "Insurance",
                BudgetConfig::Annual {
                    amount: dec(3000),
                    budget_type: BudgetType::Fixed,
                },
            ),
            make_category(40, "RSU", BudgetConfig::None),
        ];

        let bm1 = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            end_date: Some(NaiveDate::from_ymd_opt(2026, 1, 31).unwrap()),
            salary_transactions_detected: 1,
        };
        let bm2 = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: NaiveDate::from_ymd_opt(2026, 2, 1).unwrap(),
            end_date: Some(NaiveDate::from_ymd_opt(2026, 2, 28).unwrap()),
            salary_transactions_detected: 1,
        };
        let bm3 = BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            end_date: None,
            salary_transactions_detected: 1,
        };
        let all_months = [bm1.clone(), bm2.clone(), bm3.clone()];
        let year_months = budget_core::budget::budget_year_months(&all_months, &bm3);

        let transactions = vec![
            // Salary each month
            txn_on(
                salary_id,
                5000,
                NaiveDate::from_ymd_opt(2026, 1, 15).unwrap(),
            ),
            txn_on(
                salary_id,
                5000,
                NaiveDate::from_ymd_opt(2026, 2, 15).unwrap(),
            ),
            txn_on(
                salary_id,
                5000,
                NaiveDate::from_ymd_opt(2026, 3, 5).unwrap(),
            ),
            // Annual insurance payments
            txn_on(
                insurance_id,
                -1000,
                NaiveDate::from_ymd_opt(2026, 1, 10).unwrap(),
            ),
            txn_on(
                insurance_id,
                -500,
                NaiveDate::from_ymd_opt(2026, 2, 10).unwrap(),
            ),
            // RSU in Jan (unbudgeted positive)
            txn_on(rsu_id, 30000, NaiveDate::from_ymd_opt(2026, 1, 20).unwrap()),
        ];

        let classified = classify_transactions(&transactions, &categories, &bm3, &year_months);

        // Annual income across all 3 months
        assert_eq!(classified.annual_income.len(), 3);
        let annual_income_sum: Decimal = classified.annual_income.iter().map(|t| t.amount).sum();
        assert_eq!(annual_income_sum, dec(15000));

        // Annual insurance txns
        assert_eq!(classified.annual.len(), 2);

        // Annual unbudgeted includes RSU
        let annual_positive: Vec<&Transaction> = classified
            .unbudgeted_annual
            .iter()
            .filter(|t| t.amount > Decimal::ZERO)
            .collect();
        assert_eq!(annual_positive.len(), 1);
        assert_eq!(annual_positive[0].amount, dec(30000));

        // Build annual cashflow
        let ins_status = make_status(
            30,
            "Insurance",
            dec(3000),
            dec(1500),
            PaceIndicator::UnderBudget,
        );
        let mut ins_entry = make_entry(ins_status, vec![]);
        ins_entry.status.budget_mode = BudgetMode::Annual;
        let statuses = vec![ins_entry];

        let projects: Vec<ProjectStatusEntry> = vec![];

        let (_, annual_cf, _) =
            build_summaries(&statuses, &classified, &categories, &bm3, &projects);

        assert_eq!(annual_cf.income.total, dec(15000));
        assert_eq!(annual_cf.other_income.total, dec(30000));
        assert_eq!(annual_cf.budgeted_spending.total, dec(1500));
        assert_eq!(annual_cf.total_in, dec(45000));
        assert_eq!(annual_cf.total_out, dec(1500));
        assert_eq!(annual_cf.net_cashflow, dec(43500));
        assert_eq!(annual_cf.saved, dec(13500)); // 15000 - 1500 - 0
    }

    // ── build_cashflow_section edge cases ─────────────────────────────

    #[test]
    fn cashflow_section_mixed_positive_and_negative() {
        // When a section receives transactions of mixed sign, total is absolute
        // of sum (not sum of absolutes)
        let food_id = cat_id(1);
        let categories = vec![make_category(1, "Food", BudgetConfig::None)];

        // Mix of positive and negative in same category
        let transactions = vec![txn(food_id, -300), txn(food_id, 100)];

        let section = build_cashflow_section(&transactions, &categories);

        // Total = |(-300) + 100| = |-200| = 200
        assert_eq!(section.total, dec(200));
        // Items group by category, amount = |sum within category|
        assert_eq!(section.items.len(), 1);
        assert_eq!(section.items[0].amount, dec(200));
    }

    // ── compute_project_group_summary tests ──────────────────────────

    #[test]
    fn project_summary_multiple_projects() {
        let reno_status = BudgetStatus {
            category_id: cat_id(60),
            category_name: "Renovation".to_owned(),
            budget_amount: dec(5000),
            spent: dec(3000),
            remaining: dec(2000),
            time_left: None,
            pace: PaceIndicator::OnTrack,
            pace_delta: Decimal::ZERO,
            budget_mode: BudgetMode::Project,
        };
        let car_status = BudgetStatus {
            category_id: cat_id(61),
            category_name: "Car".to_owned(),
            budget_amount: dec(20000),
            spent: dec(22000),
            remaining: dec(-2000),
            time_left: None,
            pace: PaceIndicator::OverBudget,
            pace_delta: Decimal::ZERO,
            budget_mode: BudgetMode::Project,
        };

        let projects = vec![
            ProjectStatusEntry {
                status: reno_status,
                children: vec![],
                has_children: false,
                project_start_date: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
                project_end_date: None,
                finished: false,
            },
            ProjectStatusEntry {
                status: car_status,
                children: vec![],
                has_children: false,
                project_start_date: NaiveDate::from_ymd_opt(2025, 6, 1).unwrap(),
                project_end_date: Some(NaiveDate::from_ymd_opt(2026, 1, 31).unwrap()),
                finished: true,
            },
        ];

        let summary = compute_project_group_summary(&projects);

        assert_eq!(summary.total_budget, dec(25000));
        assert_eq!(summary.total_spent, dec(25000));
        assert_eq!(summary.remaining, dec(0));
        assert_eq!(summary.over_budget_count, 1);
        assert_eq!(summary.bar_max, dec(22000));
    }

    #[test]
    fn project_summary_empty() {
        let projects: Vec<ProjectStatusEntry> = vec![];
        let summary = compute_project_group_summary(&projects);

        assert_eq!(summary.total_budget, dec(0));
        assert_eq!(summary.total_spent, dec(0));
        assert_eq!(summary.remaining, dec(0));
        assert_eq!(summary.over_budget_count, 0);
        assert_eq!(summary.bar_max, dec(0));
    }
}
