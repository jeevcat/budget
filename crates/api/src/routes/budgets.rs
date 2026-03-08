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
}
