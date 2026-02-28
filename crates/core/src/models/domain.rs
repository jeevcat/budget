use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use super::enums::{
    AccountType, BudgetMode, CategoryMethod, ConnectionStatus, CorrelationType, MatchField,
    PaceIndicator, RuleType,
};
use super::id::{AccountId, BudgetMonthId, CategoryId, ConnectionId, RuleId, TransactionId};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: AccountId,
    pub provider_account_id: String,
    pub name: String,
    pub nickname: Option<String>,
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
    pub valid_until: DateTime<Utc>,
    pub status: ConnectionStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Category {
    pub id: CategoryId,
    pub name: String,
    pub parent_id: Option<CategoryId>,
    pub budget_mode: Option<BudgetMode>,
    pub budget_amount: Option<Decimal>,
    pub project_start_date: Option<NaiveDate>,
    pub project_end_date: Option<NaiveDate>,
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
    pub correlation_id: Option<TransactionId>,
    pub correlation_type: Option<CorrelationType>,
    pub category_method: Option<CategoryMethod>,
    pub suggested_category: Option<String>,
    pub counterparty_name: Option<String>,
    pub counterparty_iban: Option<String>,
    pub counterparty_bic: Option<String>,
    pub bank_transaction_code: Option<String>,
    pub llm_justification: Option<String>,
    pub skip_correlation: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleCondition {
    pub field: MatchField,
    pub pattern: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub id: RuleId,
    pub rule_type: RuleType,
    pub conditions: Vec<RuleCondition>,
    pub target_category_id: Option<CategoryId>,
    pub target_correlation_type: Option<CorrelationType>,
    pub priority: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetMonth {
    pub id: BudgetMonthId,
    pub start_date: NaiveDate,
    pub end_date: Option<NaiveDate>,
    pub salary_transactions_detected: i32,
}

/// Spending breakdown for a direct child of a project category.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectChildSpending {
    pub category_id: CategoryId,
    pub category_name: String,
    pub spent: Decimal,
}

/// Result of computing budget status for a category in a budget month
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetStatus {
    pub category_id: CategoryId,
    pub category_name: String,
    pub budget_amount: Decimal,
    pub spent: Decimal,
    pub remaining: Decimal,
    /// Monthly = days left, Annual = months left, Project = days left (-1 if open-ended)
    pub time_left: i64,
    pub pace: PaceIndicator,
    pub budget_mode: BudgetMode,
}
