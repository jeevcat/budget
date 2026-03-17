use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use chrono::{NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use budget_core::budget::{
    DailySpendPoint, SalaryStatus, budget_year_months, build_daily_cumulative_series,
    collect_budget_subtree, collect_category_subtree, compute_budget_status,
    compute_project_child_breakdowns, detect_budget_month_boundaries, effective_budget_mode,
    filter_for_budget, filter_for_project, is_in_budget_month, predict_salary_arrivals,
    salary_category_ids,
};
use budget_core::models::{
    BudgetConfig, BudgetMode, BudgetMonth, BudgetStatus, BudgetType, Category, CategoryId,
    PaceIndicator, ProjectChildSpending, Transaction,
};

use crate::routes::AppError;
use crate::state::AppState;

#[derive(Deserialize, utoipa::IntoParams)]
struct StatusQuery {
    month_id: Option<Uuid>,
}

#[derive(Serialize, utoipa::ToSchema)]
struct ChildCategoryInfo {
    category_id: CategoryId,
    category_name: String,
}

#[derive(Serialize, utoipa::ToSchema)]
struct StatusEntry {
    #[serde(flatten)]
    #[schema(inline)]
    status: BudgetStatus,
    children: Vec<ChildCategoryInfo>,
    has_children: bool,
}

#[derive(Serialize, utoipa::ToSchema)]
struct ProjectStatusEntry {
    #[serde(flatten)]
    #[schema(inline)]
    status: BudgetStatus,
    children: Vec<ProjectChildSpending>,
    has_children: bool,
    project_start_date: NaiveDate,
    project_end_date: Option<NaiveDate>,
    finished: bool,
}

#[derive(Serialize, utoipa::ToSchema)]
struct BudgetGroupSummary {
    #[schema(value_type = String)]
    total_budget: Decimal,
    #[schema(value_type = String)]
    total_spent: Decimal,
    #[schema(value_type = String)]
    remaining: Decimal,
    over_budget_count: usize,
    #[schema(value_type = String)]
    bar_max: Decimal,
}

/// A single line item in a cash-flow section, grouped by category.
#[derive(Serialize, utoipa::ToSchema)]
struct CashFlowItem {
    category_id: Option<CategoryId>,
    label: String,
    #[schema(value_type = String)]
    amount: Decimal,
    transaction_count: usize,
    transactions: Vec<Transaction>,
}

/// Unified cash-flow ledger for a budget period (month or year).
///
/// Replaces the old `BudgetGroupSummary` (for monthly/annual) + `CashFlowSummary` pair.
/// Budgeted category rows stay in `statuses[]` — they already carry pace/spent/budget/remaining.
#[derive(Serialize, utoipa::ToSchema)]
struct LedgerSummary {
    /// Salary income + positive unbudgeted transactions ("money in").
    income: Vec<CashFlowItem>,
    #[schema(value_type = String)]
    total_in: Decimal,
    /// Negative unbudgeted / uncategorized transactions.
    unbudgeted: Vec<CashFlowItem>,
    /// Budgeted spending + unbudgeted spending.
    #[schema(value_type = String)]
    total_out: Decimal,
    /// `total_in - total_out`
    #[schema(value_type = String)]
    net: Decimal,
    /// Salary income minus `total_out`.
    #[schema(value_type = String)]
    saved: Decimal,
    /// `max(spent, budget)` across variable categories (for bar scaling).
    #[schema(value_type = String)]
    bar_max: Decimal,
    /// Year-to-date monthly-budgeted total budget, included in `total_out`.
    #[schema(value_type = String)]
    monthly_budget: Decimal,
    /// Year-to-date monthly-budgeted spending, included in `total_out`.
    #[schema(value_type = String)]
    monthly_spent: Decimal,
    /// `monthly_budget - monthly_spent`
    #[schema(value_type = String)]
    monthly_remaining: Decimal,
    /// Year-to-date project total budget, included in `total_out`.
    #[schema(value_type = String)]
    project_budget: Decimal,
    /// Year-to-date project spending, included in `total_out`.
    #[schema(value_type = String)]
    project_spent: Decimal,
    /// `project_budget - project_spent`
    #[schema(value_type = String)]
    project_remaining: Decimal,
}

#[derive(Serialize, utoipa::ToSchema)]
struct StatusResponse {
    month: BudgetMonth,
    statuses: Vec<StatusEntry>,
    projects: Vec<ProjectStatusEntry>,
    monthly_ledger: LedgerSummary,
    annual_ledger: LedgerSummary,
    project_summary: BudgetGroupSummary,
    /// Transactions contributing to monthly budgets in the active month.
    monthly_transactions: Vec<Transaction>,
    /// Transactions contributing to annual budgets across the budget year.
    annual_transactions: Vec<Transaction>,
    /// Transactions contributing to project budgets.
    project_transactions: Vec<Transaction>,
    /// The budget year (calendar year of the January-anchored start).
    budget_year: i32,
    /// Salary arrival prediction (only present for the current open month).
    salary_status: Option<SalaryStatus>,
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
pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(status))
        .routes(routes!(list_months))
        .routes(routes!(burndown))
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
            // Extend the first budget month backward to cover any
            // transactions that predate the first salary. We only pass
            // salary transactions above, so the backward-extension in
            // detect_budget_month_boundaries never fires; one cheap
            // MIN(posted_date) query fixes that.
            if let Some(first) = months.first_mut()
                && let Some(earliest) = state.db.get_earliest_transaction_date().await?
                && earliest < first.start_date
            {
                first.start_date = earliest;
            }
            Ok(months)
        }
        Err(budget_core::error::Error::NoSalaryCategory) => Ok(Vec::new()),
        Err(e) => Err(AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

/// Pre-classified transactions and auxiliary dashboard data.
struct DashboardContext {
    monthly: Vec<Transaction>,
    /// Monthly-budgeted transactions across the entire budget year.
    monthly_annual: Vec<Transaction>,
    annual: Vec<Transaction>,
    project: Vec<Transaction>,
    /// Project transactions filtered to the budget year.
    project_annual: Vec<Transaction>,
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
) -> DashboardContext {
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

    let monthly_annual: Vec<Transaction> = budget_txns
        .iter()
        .filter(|t| {
            t.categorization
                .category_id()
                .is_some_and(|cid| monthly_cat_ids.contains(&cid))
                && is_in_date_range(t.posted_date, year_start, year_end)
        })
        .map(|t| (*t).clone())
        .collect();

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

    let project_all = filter_for_project(transactions, categories);
    let project: Vec<Transaction> = project_all.iter().map(|t| (*t).clone()).collect();
    let project_annual: Vec<Transaction> = project_all
        .iter()
        .filter(|t| is_in_date_range(t.posted_date, year_start, year_end))
        .map(|t| (*t).clone())
        .collect();

    let budget_year = chrono::Datelike::year(&month.start_date);

    let salary_cats = salary_category_ids(categories);
    let income = collect_income_transactions(
        transactions,
        &salary_cats,
        Some(month.start_date),
        month.end_date,
    );
    let annual_income =
        collect_income_transactions(transactions, &salary_cats, year_start, year_end);

    DashboardContext {
        monthly,
        monthly_annual,
        annual,
        project,
        project_annual,
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

/// Roll-up spending totals for a sub-ledger (monthly budgets, projects, etc.).
struct RollUp {
    budget: Decimal,
    spent: Decimal,
}

impl RollUp {
    const ZERO: Self = Self {
        budget: Decimal::ZERO,
        spent: Decimal::ZERO,
    };
}

/// Build a [`LedgerSummary`] from budget entries, income, and unbudgeted transactions.
///
/// Merges salary income + positive unbudgeted into `income`, negative unbudgeted
/// into `unbudgeted`, and computes `totals/net/saved/bar_max`.
fn build_ledger(
    entries: &[&StatusEntry],
    budgeted_spending_total: Decimal,
    monthly: &RollUp,
    project: &RollUp,
    income_txns: &[Transaction],
    unbudgeted_txns: &[Transaction],
    categories: &[Category],
) -> LedgerSummary {
    // Income: salary + positive unbudgeted
    let mut income_items = group_by_category(income_txns, categories);
    let positive_unbudgeted: Vec<Transaction> = unbudgeted_txns
        .iter()
        .filter(|t| t.amount > Decimal::ZERO)
        .cloned()
        .collect();
    income_items.extend(group_by_category(&positive_unbudgeted, categories));
    income_items.sort_by(|a, b| b.amount.cmp(&a.amount));
    let total_in = income_items
        .iter()
        .fold(Decimal::ZERO, |acc, i| acc + i.amount);

    // Unbudgeted spending: negative unbudgeted
    let negative_unbudgeted: Vec<Transaction> = unbudgeted_txns
        .iter()
        .filter(|t| t.amount < Decimal::ZERO)
        .cloned()
        .collect();
    let unbudgeted = group_by_category(&negative_unbudgeted, categories);
    let unbudgeted_spending_total = unbudgeted
        .iter()
        .fold(Decimal::ZERO, |acc, i| acc + i.amount);

    let total_out =
        budgeted_spending_total + unbudgeted_spending_total + monthly.spent + project.spent;
    let net = total_in - total_out;

    // Saved: salary income only (not other income like RSUs)
    let salary_income = income_txns
        .iter()
        .fold(Decimal::ZERO, |acc, t| acc + t.amount);
    let saved = salary_income - total_out;

    // bar_max from variable category entries only (fixed categories render as
    // checkmarks, not bars, so they shouldn't inflate the bar scale)
    let cat_map: std::collections::HashMap<CategoryId, &Category> =
        categories.iter().map(|c| (c.id, c)).collect();
    let mut bar_max = Decimal::ZERO;
    for entry in entries {
        let s = &entry.status;
        let is_fixed = cat_map
            .get(&s.category_id)
            .is_some_and(|c| c.budget.budget_type() == Some(BudgetType::Fixed));
        if !is_fixed {
            bar_max = bar_max.max(s.spent.abs()).max(s.budget_amount);
        }
    }

    let monthly_remaining = monthly.budget - monthly.spent;
    let project_remaining = project.budget - project.spent;
    LedgerSummary {
        income: income_items,
        total_in,
        unbudgeted,
        total_out,
        net,
        saved,
        bar_max,
        monthly_budget: monthly.budget,
        monthly_spent: monthly.spent,
        monthly_remaining,
        project_budget: project.budget,
        project_spent: project.spent,
        project_remaining,
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
                None,
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

/// Build monthly and annual ledger summaries from classified transactions.
fn build_ledgers(
    statuses: &[StatusEntry],
    classified: &DashboardContext,
    categories: &[Category],
    num_months: usize,
) -> (LedgerSummary, LedgerSummary) {
    let monthly_entries: Vec<&StatusEntry> = statuses
        .iter()
        .filter(|e| e.status.budget_mode == BudgetMode::Monthly)
        .collect();
    let annual_entries: Vec<&StatusEntry> = statuses
        .iter()
        .filter(|e| e.status.budget_mode == BudgetMode::Annual)
        .collect();

    let monthly_budgeted_spent = negate_sum(&classified.monthly);
    let monthly_ledger = build_ledger(
        &monthly_entries,
        monthly_budgeted_spent,
        &RollUp::ZERO,
        &RollUp::ZERO,
        &classified.income,
        &classified.unbudgeted,
        categories,
    );

    let annual_budgeted_spent = negate_sum(&classified.annual);
    let monthly_annual_spent = negate_sum(&classified.monthly_annual);
    let monthly_annual_budget = monthly_entries
        .iter()
        .fold(Decimal::ZERO, |acc, e| acc + e.status.budget_amount)
        * Decimal::from(num_months);

    let project_annual_spent = negate_sum(&classified.project_annual);
    let project_annual_budget = categories
        .iter()
        .filter_map(|c| match &c.budget {
            BudgetConfig::Project {
                amount,
                start_date,
                end_date,
            } => {
                let year = classified.budget_year;
                let year_start =
                    chrono::NaiveDate::from_ymd_opt(year, 1, 1).unwrap_or(chrono::NaiveDate::MIN);
                let year_end =
                    chrono::NaiveDate::from_ymd_opt(year, 12, 31).unwrap_or(chrono::NaiveDate::MAX);
                let project_end = end_date.unwrap_or(chrono::NaiveDate::MAX);
                if *start_date <= year_end && project_end >= year_start {
                    Some(*amount)
                } else {
                    None
                }
            }
            _ => None,
        })
        .fold(Decimal::ZERO, |acc, a| acc + a);

    let monthly_rollup = RollUp {
        budget: monthly_annual_budget,
        spent: monthly_annual_spent,
    };
    let project_rollup = RollUp {
        budget: project_annual_budget,
        spent: project_annual_spent,
    };
    let annual_ledger = build_ledger(
        &annual_entries,
        annual_budgeted_spent,
        &monthly_rollup,
        &project_rollup,
        &classified.annual_income,
        &classified.unbudgeted_annual,
        categories,
    );

    (monthly_ledger, annual_ledger)
}

/// Compute budget status for every budgeted category in a given month.
///
/// Returns all data needed to render the dashboard: budget statuses, pre-filtered
/// transactions grouped by mode, uncategorized count, budget year, and project
fn seasonal_context_for(
    cat: &Category,
    transactions: &[Transaction],
    budget_months: &[BudgetMonth],
    categories: &[Category],
) -> Option<budget_core::seasonality::SeasonalContext> {
    if cat.budget.budget_type() != Some(budget_core::models::BudgetType::Variable) {
        return None;
    }
    let series = budget_core::budget::build_monthly_spending_series(
        transactions,
        cat.id,
        budget_months,
        categories,
    );
    budget_core::seasonality::compute_seasonal_context(&series)
}

/// child breakdowns. The frontend is a pure display layer — no business logic.
///
/// # Errors
///
/// Returns 404 if the requested budget month does not exist.
/// Returns `AppError` if any database query fails.
#[utoipa::path(get, path = "/status", tag = "budgets", params(StatusQuery), responses((status = 200, body = StatusResponse)), security(("bearer_token" = [])))]
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
    // Go back far enough for MSTL to have 24+ months of history
    if let Some(first_month) = budget_months.first() {
        earliest = earliest.min(first_month.start_date);
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
            let seasonal = seasonal_context_for(cat, &transactions, &budget_months, &categories);
            let status = compute_budget_status(
                cat,
                &transactions,
                month,
                &budget_months,
                &categories,
                reference_date,
                seasonal.as_ref(),
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

    let (monthly_ledger, annual_ledger) =
        build_ledgers(&statuses, &classified, &categories, year_months.len());
    let project_summary = compute_project_group_summary(&projects);

    let salary_status = if month.end_date.is_none() {
        Some(predict_salary_arrivals(
            &transactions,
            &categories,
            Utc::now().date_naive(),
            state.expected_salary_count,
        ))
    } else {
        None
    };

    Ok(Json(StatusResponse {
        month: month.clone(),
        statuses,
        projects,
        monthly_ledger,
        annual_ledger,
        project_summary,
        monthly_transactions: classified.monthly,
        annual_transactions: classified.annual,
        project_transactions: classified.project,
        budget_year: classified.budget_year,
        salary_status,
    }))
}

/// List all budget months, derived on the fly from transactions.
///
/// # Errors
///
/// Returns `AppError` if the database query fails.
#[utoipa::path(get, path = "/months", tag = "budgets", responses((status = 200, body = Vec<BudgetMonth>)), security(("bearer_token" = [])))]
async fn list_months(State(state): State<AppState>) -> Result<Json<Vec<BudgetMonth>>, AppError> {
    let categories = state.db.list_categories().await?;
    let months = derive_months(&state, &categories).await?;
    Ok(Json(months))
}

#[derive(Deserialize, utoipa::IntoParams)]
struct BurndownQuery {
    category_id: Uuid,
    month_id: Option<Uuid>,
}

#[derive(Serialize, utoipa::ToSchema)]
struct BurndownMonthSeries {
    start_date: NaiveDate,
    total_days: u16,
    points: Vec<DailySpendPoint>,
}

#[derive(Serialize, utoipa::ToSchema)]
struct SubcategorySeries {
    category_id: CategoryId,
    category_name: String,
    current: BurndownMonthSeries,
}

#[derive(Serialize, utoipa::ToSchema)]
struct BurndownResponse {
    category_name: String,
    #[schema(value_type = String)]
    budget_amount: Decimal,
    current: BurndownMonthSeries,
    prior: Vec<BurndownMonthSeries>,
    #[schema(value_type = Option<String>)]
    predicted_landing: Option<Decimal>,
    subcategories: Vec<SubcategorySeries>,
}

/// Build burndown series for up to 3 prior closed months.
fn build_prior_series(
    prior_months: &[BudgetMonth],
    filtered: &[&Transaction],
    subtree: &[CategoryId],
) -> Vec<BurndownMonthSeries> {
    prior_months
        .iter()
        .rev()
        .filter(|bm| bm.end_date.is_some())
        .take(3)
        .map(|bm| {
            let end = bm.end_date.expect("filtered to closed months");
            let points = build_daily_cumulative_series(filtered, subtree, bm, end);
            let total = (end - bm.start_date).num_days() + 1;
            BurndownMonthSeries {
                start_date: bm.start_date,
                total_days: u16::try_from(total).unwrap_or(u16::MAX),
                points,
            }
        })
        .collect()
}

/// Predict end-of-month landing via linear extrapolation of current spend.
fn predict_landing(
    month: &BudgetMonth,
    current: &BurndownMonthSeries,
    total_days: i64,
    today: NaiveDate,
) -> Option<Decimal> {
    if month.end_date.is_some() {
        return None;
    }
    let elapsed = (today - month.start_date).num_days().max(1);
    let total = total_days.max(1);
    let spent = current
        .points
        .last()
        .map_or(Decimal::ZERO, |p| p.cumulative);
    if spent > Decimal::ZERO && elapsed > 0 {
        let factor = Decimal::from(total) / Decimal::from(elapsed);
        Some(spent * factor)
    } else {
        None
    }
}

/// Build per-subcategory burndown series for budget-less direct children.
fn build_subcategory_series(
    parent_id: CategoryId,
    categories: &[Category],
    subtree: &[CategoryId],
    filtered: &[&Transaction],
    month: &BudgetMonth,
    today: NaiveDate,
    total_days: i64,
) -> Vec<SubcategorySeries> {
    let total_days_u16 = u16::try_from(total_days).unwrap_or(u16::MAX);
    categories
        .iter()
        .filter(|c| c.parent_id == Some(parent_id) && subtree.contains(&c.id))
        .filter_map(|child| {
            let child_subtree = collect_budget_subtree(child.id, categories);
            let child_points =
                build_daily_cumulative_series(filtered, &child_subtree, month, today);
            let has_spending = child_points
                .last()
                .is_some_and(|p| p.cumulative != Decimal::ZERO);
            if !has_spending {
                return None;
            }
            Some(SubcategorySeries {
                category_id: child.id,
                category_name: child.name.to_string(),
                current: BurndownMonthSeries {
                    start_date: month.start_date,
                    total_days: total_days_u16,
                    points: child_points,
                },
            })
        })
        .collect()
}

/// Return daily cumulative spend for a category over the current (or specified)
/// budget month, plus up to 3 prior closed months for comparison.
///
/// # Errors
///
/// Returns 404 if the category or budget month doesn't exist.
/// Returns 400 if the category is not a monthly-mode variable budget.
#[utoipa::path(get, path = "/burndown", tag = "budgets", params(BurndownQuery), responses((status = 200, body = BurndownResponse)), security(("bearer_token" = [])))]
async fn burndown(
    State(state): State<AppState>,
    Query(query): Query<BurndownQuery>,
) -> Result<Json<BurndownResponse>, AppError> {
    let categories = state.db.list_categories().await?;
    let mut budget_months = derive_months(&state, &categories).await?;
    budget_months.sort_by_key(|bm| bm.start_date);

    let category_id = CategoryId::from_uuid(query.category_id);
    let category = categories
        .iter()
        .find(|c| c.id == category_id)
        .ok_or_else(|| AppError(StatusCode::NOT_FOUND, "category not found".to_owned()))?;

    // Validate: must be monthly-mode variable
    let mode = effective_budget_mode(category, &categories);
    if mode != Some(BudgetMode::Monthly) {
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            "burndown only supports monthly-mode categories".to_owned(),
        ));
    }
    if category.budget.budget_type() == Some(BudgetType::Fixed) {
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            "burndown only supports variable-type categories".to_owned(),
        ));
    }

    let target_month = if let Some(id) = query.month_id {
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

    let target_idx = budget_months
        .iter()
        .position(|bm| bm.id == target_month.id)
        .unwrap_or(0);

    // Fetch transactions going back far enough for prior months
    let prior_start_idx = target_idx.saturating_sub(3);
    let earliest = budget_months[prior_start_idx].start_date;
    let transactions = state.db.list_transactions_since(earliest).await?;
    let filtered = filter_for_budget(&transactions, &categories);
    let subtree = collect_budget_subtree(category_id, &categories);

    let today = Utc::now().date_naive();

    // Current month series
    let current_points = build_daily_cumulative_series(&filtered, &subtree, target_month, today);
    let total_days_current = target_month.end_date.map_or_else(
        || (today - target_month.start_date).num_days() + 1,
        |end| (end - target_month.start_date).num_days() + 1,
    );
    let current = BurndownMonthSeries {
        start_date: target_month.start_date,
        total_days: u16::try_from(total_days_current).unwrap_or(u16::MAX),
        points: current_points,
    };

    let prior = build_prior_series(
        &budget_months[prior_start_idx..target_idx],
        &filtered,
        &subtree,
    );

    let predicted_landing = predict_landing(target_month, &current, total_days_current, today);

    let budget_amount = category.budget.amount().unwrap_or(Decimal::ZERO);

    let subcategories = build_subcategory_series(
        category_id,
        &categories,
        &subtree,
        &filtered,
        target_month,
        today,
        total_days_current,
    );

    Ok(Json(BurndownResponse {
        category_name: category.name.to_string(),
        budget_amount,
        current,
        prior,
        predicted_landing,
        subcategories,
    }))
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
            seasonal_factor: None,
            trend_monthly: None,
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

    // ── build_ledger tests ────────────────────────────────────────

    #[test]
    fn ledger_splits_unbudgeted_by_sign() {
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
            txn(rsu_id, 10000), // positive → income
            txn(tax_id, -2000), // negative → unbudgeted
        ];

        let ledger = build_ledger(
            &[],
            dec(2500),
            &RollUp::ZERO,
            &RollUp::ZERO,
            &income_txns,
            &unbudgeted_txns,
            &categories,
        );

        // Income should have salary (5000) + RSU (10000)
        let salary_item = ledger.income.iter().find(|i| i.label == "Salary");
        let rsu_item = ledger.income.iter().find(|i| i.label == "RSU");
        assert_eq!(salary_item.unwrap().amount, dec(5000));
        assert_eq!(rsu_item.unwrap().amount, dec(10000));
        assert_eq!(ledger.total_in, dec(15000));
        // Unbudgeted should have Tax (2000)
        assert_eq!(ledger.unbudgeted.len(), 1);
        assert_eq!(ledger.unbudgeted[0].amount, dec(2000));
    }

    #[test]
    fn ledger_computed_totals() {
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
            txn(rsu_id, 2000),  // positive → income
            txn(misc_id, -800), // negative → unbudgeted
        ];
        let ledger = build_ledger(
            &[],
            dec(3000),
            &RollUp::ZERO,
            &RollUp::ZERO,
            &income_txns,
            &unbudgeted_txns,
            &categories,
        );

        // total_in = salary + RSU = 5000 + 2000
        assert_eq!(ledger.total_in, dec(7000));
        // total_out = budgeted + unbudgeted = 3000 + 800
        assert_eq!(ledger.total_out, dec(3800));
        // net = total_in - total_out = 7000 - 3800
        assert_eq!(ledger.net, dec(3200));
        // saved = salary_income - total_out = 5000 - 3800
        assert_eq!(ledger.saved, dec(1200));
    }

    #[test]
    fn ledger_saved_excludes_other_income() {
        let salary_id = cat_id(10);
        let windfall_id = cat_id(20);
        let categories = vec![
            make_category(10, "Salary", BudgetConfig::Salary),
            make_category(20, "Windfall", BudgetConfig::None),
        ];

        let income_txns = vec![txn(salary_id, 5000)];
        // Large windfall should NOT inflate saved
        let unbudgeted_txns = vec![txn(windfall_id, 50000)];

        let ledger = build_ledger(
            &[],
            dec(4000),
            &RollUp::ZERO,
            &RollUp::ZERO,
            &income_txns,
            &unbudgeted_txns,
            &categories,
        );

        // Saved = salary - total_out = 5000 - 4000 = 1000
        assert_eq!(ledger.saved, dec(1000));
        // But net includes it: 55000 - 4000 = 51000
        assert_eq!(ledger.net, dec(51000));
    }

    #[test]
    fn ledger_income_only() {
        let salary_id = cat_id(10);
        let categories = vec![make_category(10, "Salary", BudgetConfig::Salary)];

        let income_txns = vec![txn(salary_id, 5000)];

        let ledger = build_ledger(
            &[],
            dec(0),
            &RollUp::ZERO,
            &RollUp::ZERO,
            &income_txns,
            &[],
            &categories,
        );

        assert_eq!(ledger.total_in, dec(5000));
        assert_eq!(ledger.total_out, dec(0));
        assert_eq!(ledger.net, dec(5000));
        assert_eq!(ledger.saved, dec(5000));
    }

    #[test]
    fn ledger_spending_only() {
        let misc_id = cat_id(30);
        let categories = vec![make_category(30, "Misc", BudgetConfig::None)];

        let unbudgeted_txns = vec![txn(misc_id, -1500)];

        let ledger = build_ledger(
            &[],
            dec(2000),
            &RollUp::ZERO,
            &RollUp::ZERO,
            &[],
            &unbudgeted_txns,
            &categories,
        );

        assert_eq!(ledger.total_in, dec(0));
        assert_eq!(ledger.total_out, dec(3500));
        assert_eq!(ledger.net, dec(-3500));
        assert_eq!(ledger.saved, dec(-3500));
    }

    #[test]
    fn ledger_zero_amount_txns_excluded_from_split() {
        let misc_id = cat_id(30);
        let categories = vec![make_category(30, "Misc", BudgetConfig::None)];

        // Zero-amount transaction → neither positive nor negative
        let unbudgeted_txns = vec![txn(misc_id, 0)];

        let ledger = build_ledger(
            &[],
            dec(0),
            &RollUp::ZERO,
            &RollUp::ZERO,
            &[],
            &unbudgeted_txns,
            &categories,
        );

        assert!(ledger.income.is_empty());
        assert!(ledger.unbudgeted.is_empty());
    }

    #[test]
    fn ledger_bar_max_from_entries() {
        let food_entry = make_entry(
            make_status(1, "Food", dec(5000), dec(3000), PaceIndicator::OnTrack),
            vec![],
        );
        let entries: Vec<&StatusEntry> = vec![&food_entry];
        let ledger = build_ledger(
            &entries,
            dec(0),
            &RollUp::ZERO,
            &RollUp::ZERO,
            &[],
            &[],
            &[],
        );
        assert_eq!(ledger.bar_max, dec(5000));
    }

    #[test]
    fn ledger_bar_max_excludes_fixed_categories() {
        // Rent is fixed at 1500, groceries is variable at 400
        let rent_entry = make_entry(
            make_status(1, "Rent", dec(1500), dec(1500), PaceIndicator::OnTrack),
            vec![],
        );
        let groceries_entry = make_entry(
            make_status(2, "Groceries", dec(400), dec(350), PaceIndicator::OnTrack),
            vec![],
        );
        let rent_cat = make_category(
            1,
            "Rent",
            BudgetConfig::Monthly {
                amount: dec(1500),
                budget_type: BudgetType::Fixed,
            },
        );
        let groceries_cat = make_category(
            2,
            "Groceries",
            BudgetConfig::Monthly {
                amount: dec(400),
                budget_type: BudgetType::Variable,
            },
        );
        let entries: Vec<&StatusEntry> = vec![&rent_entry, &groceries_entry];
        let categories = vec![rent_cat, groceries_cat];
        let ledger = build_ledger(
            &entries,
            dec(0),
            &RollUp::ZERO,
            &RollUp::ZERO,
            &[],
            &[],
            &categories,
        );
        // bar_max should be 400 (groceries), not 1500 (rent)
        assert_eq!(ledger.bar_max, dec(400));
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

    // ── build_ledger integration test ────────────────────────────

    #[test]
    fn build_ledger_full_scenario() {
        let salary_id = cat_id(10);
        let rsu_id = cat_id(40);
        let tax_id = cat_id(50);

        let categories = vec![
            make_category(10, "Salary", BudgetConfig::Salary),
            make_category(40, "RSU", BudgetConfig::None),
            make_category(50, "Tax", BudgetConfig::None),
        ];

        let food_status = make_status(20, "Food", dec(1000), dec(600), PaceIndicator::OnTrack);
        let mut food_entry = make_entry(food_status, vec![]);
        food_entry.status.budget_mode = BudgetMode::Monthly;
        let entries: Vec<&StatusEntry> = vec![&food_entry];

        let income_txns = vec![txn(salary_id, 5000)];
        let unbudgeted = vec![
            txn(rsu_id, 10000), // positive → income
            txn(tax_id, -500),  // negative → unbudgeted
        ];

        let monthly = build_ledger(
            &entries,
            dec(600),
            &RollUp::ZERO,
            &RollUp::ZERO,
            &income_txns,
            &unbudgeted,
            &categories,
        );

        // Income: Salary (5000) + RSU (10000)
        assert_eq!(monthly.total_in, dec(15000));
        // Unbudgeted spending: Tax (500)
        let unb_total: Decimal = monthly.unbudgeted.iter().map(|i| i.amount).sum();
        assert_eq!(unb_total, dec(500));
        // total_out = budgeted (600) + unbudgeted (500)
        assert_eq!(monthly.total_out, dec(1100));
        assert_eq!(monthly.net, dec(13900));
        // saved = salary (5000) - total_out (1100)
        assert_eq!(monthly.saved, dec(3900));
        // bar_max from entries
        assert_eq!(monthly.bar_max, dec(1000));
    }

    #[test]
    fn build_ledger_empty() {
        let ledger = build_ledger(&[], dec(0), &RollUp::ZERO, &RollUp::ZERO, &[], &[], &[]);

        assert_eq!(ledger.total_in, dec(0));
        assert_eq!(ledger.total_out, dec(0));
        assert_eq!(ledger.net, dec(0));
        assert_eq!(ledger.saved, dec(0));
        assert_eq!(ledger.bar_max, dec(0));
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

        // When fed to build_ledger, the split happens correctly
        let food_status = make_status(40, "Food", dec(500), dec(300), PaceIndicator::OnTrack);
        let mut food_entry = make_entry(food_status, vec![]);
        food_entry.status.budget_mode = BudgetMode::Monthly;
        let entries: Vec<&StatusEntry> = vec![&food_entry];

        let monthly_budgeted_spent = negate_sum(&classified.monthly);
        let ledger = build_ledger(
            &entries,
            monthly_budgeted_spent,
            &RollUp::ZERO,
            &RollUp::ZERO,
            &classified.income,
            &classified.unbudgeted,
            &categories,
        );

        // Income: Salary (5000) + RSU (15000)
        assert_eq!(ledger.total_in, dec(20000));
        // Unbudgeted spending: Tax (2000) + Uncategorized (500)
        let unb_total: Decimal = ledger.unbudgeted.iter().map(|i| i.amount).sum();
        assert_eq!(unb_total, dec(2500)); // 2000 + 500
    }

    #[test]
    fn ledger_other_income_groups_by_category() {
        let rsu_id = cat_id(20);
        let car_id = cat_id(21);
        let categories = vec![
            make_category(20, "RSU", BudgetConfig::None),
            make_category(21, "Car Sale", BudgetConfig::None),
        ];

        let unbudgeted_txns = vec![txn(rsu_id, 59000), txn(car_id, 15000)];

        let ledger = build_ledger(
            &[],
            dec(0),
            &RollUp::ZERO,
            &RollUp::ZERO,
            &[],
            &unbudgeted_txns,
            &categories,
        );

        assert_eq!(ledger.income.len(), 2);
        // Sorted by amount descending
        assert_eq!(ledger.income[0].label, "RSU");
        assert_eq!(ledger.income[0].amount, dec(59000));
        assert_eq!(ledger.income[1].label, "Car Sale");
        assert_eq!(ledger.income[1].amount, dec(15000));
        assert_eq!(ledger.total_in, dec(74000));
    }

    #[test]
    fn ledger_unbudgeted_spending_groups_by_category() {
        let tax_id = cat_id(30);
        let categories = vec![make_category(30, "Tax", BudgetConfig::None)];

        let unbudgeted_txns = vec![
            txn(tax_id, -2000),
            txn(tax_id, -500),
            uncategorized_txn(-800),
        ];

        let ledger = build_ledger(
            &[],
            dec(0),
            &RollUp::ZERO,
            &RollUp::ZERO,
            &[],
            &unbudgeted_txns,
            &categories,
        );

        assert_eq!(ledger.unbudgeted.len(), 2);
        // Tax: |-2000 + -500| = 2500 (sorted first)
        assert_eq!(ledger.unbudgeted[0].label, "Tax");
        assert_eq!(ledger.unbudgeted[0].amount, dec(2500));
        assert_eq!(ledger.unbudgeted[0].transaction_count, 2);
        // Uncategorized: 800
        assert_eq!(ledger.unbudgeted[1].label, "Uncategorized");
        assert_eq!(ledger.unbudgeted[1].amount, dec(800));
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

    // ── build_ledger edge cases ─────────────────────────────────────

    #[test]
    fn ledger_multiple_income_categories() {
        let salary_id = cat_id(10);
        let bonus_id = cat_id(11);
        let categories = vec![
            make_category(10, "Salary", BudgetConfig::Salary),
            make_category(11, "Bonus", BudgetConfig::None),
        ];

        // Both are income transactions (salary-category)
        let income_txns = vec![txn(salary_id, 5000), txn(bonus_id, 1000)];

        let ledger = build_ledger(
            &[],
            dec(0),
            &RollUp::ZERO,
            &RollUp::ZERO,
            &income_txns,
            &[],
            &categories,
        );

        assert_eq!(ledger.total_in, dec(6000));
        assert_eq!(ledger.income.len(), 2);
        // Sorted by amount descending
        assert_eq!(ledger.income[0].label, "Salary");
        assert_eq!(ledger.income[0].amount, dec(5000));
        assert_eq!(ledger.income[1].label, "Bonus");
        assert_eq!(ledger.income[1].amount, dec(1000));
    }

    #[test]
    fn ledger_all_sections_populated() {
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

        let ledger = build_ledger(
            &[],
            dec(4500),
            &RollUp::ZERO,
            &RollUp::ZERO,
            &income_txns,
            &unbudgeted_txns,
            &categories,
        );

        // Income: Salary (8000) + RSU (20000) = 28000
        assert_eq!(ledger.total_in, dec(28000));
        // Salary item
        let salary = ledger.income.iter().find(|i| i.label == "Salary").unwrap();
        assert_eq!(salary.amount, dec(8000));
        // RSU item
        let rsu = ledger.income.iter().find(|i| i.label == "RSU").unwrap();
        assert_eq!(rsu.amount, dec(20000));

        // Unbudgeted spending: Tax (3000) + Uncategorized (1000) = 4000
        assert_eq!(ledger.unbudgeted.len(), 2);

        // total_out = budgeted (4500) + unbudgeted (4000) = 8500
        assert_eq!(ledger.total_out, dec(8500));
        assert_eq!(ledger.net, dec(19500));
        // saved = salary (8000) - total_out (8500) = -500
        assert_eq!(ledger.saved, dec(-500));
    }

    #[test]
    fn ledger_negative_saved_when_overspent() {
        let salary_id = cat_id(10);
        let categories = vec![make_category(10, "Salary", BudgetConfig::Salary)];

        let income_txns = vec![txn(salary_id, 3000)];

        let ledger = build_ledger(
            &[],
            dec(4000),
            &RollUp::ZERO,
            &RollUp::ZERO,
            &income_txns,
            &[],
            &categories,
        );

        // saved = salary (3000) - total_out (4000) = -1000
        assert_eq!(ledger.saved, dec(-1000));
        // net also negative
        assert_eq!(ledger.net, dec(-1000));
    }

    #[test]
    fn build_ledger_multiple_monthly_and_annual() {
        let food_status = make_status(20, "Food", dec(1000), dec(800), PaceIndicator::OnTrack);
        let mut food_entry = make_entry(food_status, vec![]);
        food_entry.status.budget_mode = BudgetMode::Monthly;

        let transport_status =
            make_status(21, "Transport", dec(200), dec(150), PaceIndicator::OnTrack);
        let mut transport_entry = make_entry(transport_status, vec![]);
        transport_entry.status.budget_mode = BudgetMode::Monthly;

        let monthly_entries: Vec<&StatusEntry> = vec![&food_entry, &transport_entry];
        let monthly = build_ledger(
            &monthly_entries,
            dec(950),
            &RollUp::ZERO,
            &RollUp::ZERO,
            &[],
            &[],
            &[],
        );

        // total_out = budgeted spending only
        assert_eq!(monthly.total_out, dec(950));
        // bar_max from entries: max(1000, 800, 200, 150) = 1000
        assert_eq!(monthly.bar_max, dec(1000));
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

    // ── end-to-end integration: classify → build_ledger ───────────

    #[test]
    fn end_to_end_classify_and_ledger_with_all_transaction_types() {
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
            txn_on(
                salary_id,
                5000,
                NaiveDate::from_ymd_opt(2026, 2, 5).unwrap(),
            ),
            txn_on(food_id, -700, NaiveDate::from_ymd_opt(2026, 2, 10).unwrap()),
            txn_on(rsu_id, 25000, NaiveDate::from_ymd_opt(2026, 2, 12).unwrap()),
            txn_on(tax_id, -3000, NaiveDate::from_ymd_opt(2026, 2, 15).unwrap()),
            uncategorized_txn_on(200, NaiveDate::from_ymd_opt(2026, 2, 18).unwrap()),
            uncategorized_txn_on(-150, NaiveDate::from_ymd_opt(2026, 2, 20).unwrap()),
        ];

        let classified = classify_transactions(&transactions, &categories, &month, &year_months);

        assert_eq!(classified.monthly.len(), 1); // food
        assert_eq!(classified.income.len(), 1); // salary
        assert_eq!(classified.unbudgeted.len(), 4);

        let food_status = make_status(20, "Food", dec(1000), dec(700), PaceIndicator::OnTrack);
        let mut food_entry = make_entry(food_status, vec![]);
        food_entry.status.budget_mode = BudgetMode::Monthly;

        let entries: Vec<&StatusEntry> = vec![&food_entry];
        let monthly_budgeted_spent = negate_sum(&classified.monthly);
        let ledger = build_ledger(
            &entries,
            monthly_budgeted_spent,
            &RollUp::ZERO,
            &RollUp::ZERO,
            &classified.income,
            &classified.unbudgeted,
            &categories,
        );

        // Income: Salary (5000) + RSU (25000) + uncategorized+ (200) = 30200
        assert_eq!(ledger.total_in, dec(30200));

        // Unbudgeted spending: Tax (3000) + uncategorized- (150) = 3150
        let unb_total: Decimal = ledger.unbudgeted.iter().map(|i| i.amount).sum();
        assert_eq!(unb_total, dec(3150));
        assert_eq!(ledger.unbudgeted.len(), 2);

        // total_out = budgeted (700) + unbudgeted (3150) = 3850
        assert_eq!(ledger.total_out, dec(3850));
        assert_eq!(ledger.net, dec(26350)); // 30200 - 3850
        // saved = salary (5000) - total_out (3850) = 1150
        assert_eq!(ledger.saved, dec(1150));
    }

    #[test]
    fn end_to_end_annual_cashflow_spans_multiple_months() {
        let (salary_id, insurance_id, rsu_id) = (cat_id(10), cat_id(30), cat_id(40));
        let d = |m, d| NaiveDate::from_ymd_opt(2026, m, d).unwrap();
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
        let bm = |start: NaiveDate, end: Option<NaiveDate>| BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: start,
            end_date: end,
            salary_transactions_detected: 1,
        };
        let bm1 = bm(d(1, 1), Some(d(1, 31)));
        let bm2 = bm(d(2, 1), Some(d(2, 28)));
        let bm3 = bm(d(3, 1), None);
        let all_months = [bm1.clone(), bm2.clone(), bm3.clone()];
        let year_months = budget_core::budget::budget_year_months(&all_months, &bm3);
        let transactions = vec![
            txn_on(salary_id, 5000, d(1, 15)),
            txn_on(salary_id, 5000, d(2, 15)),
            txn_on(salary_id, 5000, d(3, 5)),
            txn_on(insurance_id, -1000, d(1, 10)),
            txn_on(insurance_id, -500, d(2, 10)),
            txn_on(rsu_id, 30000, d(1, 20)),
        ];

        let classified = classify_transactions(&transactions, &categories, &bm3, &year_months);
        assert_eq!(classified.annual_income.len(), 3);
        assert_eq!(classified.annual.len(), 2);
        let annual_positive: Vec<_> = classified
            .unbudgeted_annual
            .iter()
            .filter(|t| t.amount > Decimal::ZERO)
            .collect();
        assert_eq!(
            (annual_positive.len(), annual_positive[0].amount),
            (1, dec(30000))
        );

        let ins_status = make_status(
            30,
            "Insurance",
            dec(3000),
            dec(1500),
            PaceIndicator::UnderBudget,
        );
        let mut ins_entry = make_entry(ins_status, vec![]);
        ins_entry.status.budget_mode = BudgetMode::Annual;
        let monthly_rollup = RollUp {
            budget: dec(0),
            spent: negate_sum(&classified.monthly_annual),
        };
        let annual_ledger = build_ledger(
            &[&ins_entry],
            negate_sum(&classified.annual),
            &monthly_rollup,
            &RollUp::ZERO,
            &classified.annual_income,
            &classified.unbudgeted_annual,
            &categories,
        );

        assert_eq!(annual_ledger.total_in, dec(45000));
        assert_eq!(annual_ledger.total_out, dec(1500));
        assert_eq!(annual_ledger.net, dec(43500));
        assert_eq!(annual_ledger.saved, dec(13500));
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
            seasonal_factor: None,
            trend_monthly: None,
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
            seasonal_factor: None,
            trend_monthly: None,
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
