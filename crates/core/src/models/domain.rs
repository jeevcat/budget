use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use super::enums::{
    AccountType, ConnectionStatus, CorrelationType, MatchField, PaceIndicator, PeriodType, RuleType,
};
use super::id::{
    AccountId, BudgetMonthId, BudgetPeriodId, CategoryId, ConnectionId, ProjectId, RuleId,
    TransactionId,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: AccountId,
    pub provider_account_id: String,
    pub name: String,
    pub institution: String,
    pub account_type: AccountType,
    pub currency: String,
    pub connection_id: Option<ConnectionId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connection {
    pub id: ConnectionId,
    pub provider: String,
    pub provider_session_id: String,
    pub institution_name: String,
    pub valid_until: String,
    pub status: ConnectionStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Category {
    pub id: CategoryId,
    pub name: String,
    pub parent_id: Option<CategoryId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub id: TransactionId,
    pub account_id: AccountId,
    pub category_id: Option<CategoryId>,
    pub amount: Decimal,
    pub original_amount: Option<Decimal>,
    pub original_currency: Option<String>,
    pub merchant_name: String,
    pub description: String,
    pub posted_date: NaiveDate,
    pub budget_month_id: Option<BudgetMonthId>,
    pub project_id: Option<ProjectId>,
    pub correlation_id: Option<TransactionId>,
    pub correlation_type: Option<CorrelationType>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub id: RuleId,
    pub rule_type: RuleType,
    pub match_field: MatchField,
    pub match_pattern: String,
    pub target_category_id: Option<CategoryId>,
    pub target_correlation_type: Option<CorrelationType>,
    pub priority: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetPeriod {
    pub id: BudgetPeriodId,
    pub category_id: CategoryId,
    pub period_type: PeriodType,
    pub amount: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetMonth {
    pub id: BudgetMonthId,
    pub start_date: NaiveDate,
    pub end_date: Option<NaiveDate>,
    pub salary_transactions_detected: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: ProjectId,
    pub name: String,
    pub category_id: CategoryId,
    pub start_date: NaiveDate,
    pub end_date: Option<NaiveDate>,
    pub budget_amount: Option<Decimal>,
}

/// Result of computing budget status for a category in a budget month
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetStatus {
    pub category_id: CategoryId,
    pub budget_amount: Decimal,
    pub spent: Decimal,
    pub remaining: Decimal,
    pub days_left: i64,
    pub pace: PaceIndicator,
}
