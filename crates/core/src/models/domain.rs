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
    /// Array of free-text payment detail lines from the bank.
    /// May contain "Key: Value" pairs, reference numbers, or plain text.
    /// Source: Enable Banking `remittance_information`
    pub remittance_information: Vec<String>,
    pub posted_date: NaiveDate,
    pub correlation_id: Option<TransactionId>,
    pub correlation_type: Option<CorrelationType>,
    pub category_method: Option<CategoryMethod>,
    pub suggested_category: Option<String>,
    pub counterparty_name: Option<String>,
    pub counterparty_iban: Option<String>,
    pub counterparty_bic: Option<String>,
    /// Human-readable bank transaction label (e.g. "Gehalt/Rente"). Bank-specific, not standardized.
    /// Source: Enable Banking `bank_transaction_code.description`
    pub bank_transaction_code: Option<String>,
    pub llm_justification: Option<String>,
    pub skip_correlation: bool,
    /// ISO 18245 MCC code (e.g. "5411" = grocery). Only present for card transactions.
    /// Source: Enable Banking `merchant_category_code`
    pub merchant_category_code: Option<String>,
    /// ISO 20022 domain code (e.g. "PMNT" for payments).
    /// Source: Enable Banking `bank_transaction_code.code`
    pub bank_transaction_code_code: Option<String>,
    /// ISO 20022 sub-family code (e.g. "ICDT-STDO").
    /// Source: Enable Banking `bank_transaction_code.sub_code`
    pub bank_transaction_code_sub_code: Option<String>,
    /// Actual FX rate applied (e.g. "1.0856"), stored as string to preserve bank precision.
    /// Source: Enable Banking `exchange_rate.exchange_rate`
    pub exchange_rate: Option<String>,
    /// ISO 4217 currency code in which the exchange rate is expressed.
    /// Source: Enable Banking `exchange_rate.unit_currency`
    pub exchange_rate_unit_currency: Option<String>,
    /// FX rate type: AGRD (agreed/contract), SALE, or SPOT.
    /// Source: Enable Banking `exchange_rate.rate_type`
    pub exchange_rate_type: Option<String>,
    /// FX contract reference when `rate_type` is AGRD (agreed).
    /// Source: Enable Banking `exchange_rate.contract_identification`
    pub exchange_rate_contract_id: Option<String>,
    /// Structured payment reference (e.g. "RF07850352502356628678117").
    /// Source: Enable Banking `reference_number`
    pub reference_number: Option<String>,
    /// Scheme of the reference number: BERF, FIRF, INTL, NORF, SDDM, SEBG.
    /// Source: Enable Banking `reference_number_schema`
    pub reference_number_schema: Option<String>,
    /// Internal note made by PSU (Payment Service User), distinct from remittance info.
    /// Source: Enable Banking `note`
    pub note: Option<String>,
    /// Account balance after this transaction (amount component).
    /// Source: Enable Banking `balance_after_transaction.amount`
    pub balance_after_transaction: Option<Decimal>,
    /// Currency of the balance after transaction (usually same as account currency).
    /// Source: Enable Banking `balance_after_transaction.currency`
    pub balance_after_transaction_currency: Option<String>,
    /// Non-IBAN creditor account IDs: JSONB array of `{identification, scheme_name, issuer}`.
    /// Source: Enable Banking `creditor_account_additional_identification`
    pub creditor_account_additional_id: Option<serde_json::Value>,
    /// Non-IBAN debtor account IDs: JSONB array of `{identification, scheme_name, issuer}`.
    /// Source: Enable Banking `debtor_account_additional_identification`
    pub debtor_account_additional_id: Option<serde_json::Value>,
}

impl Default for Transaction {
    fn default() -> Self {
        Self {
            id: TransactionId::new(),
            account_id: AccountId::new(),
            category_id: None,
            amount: Decimal::ZERO,
            original_amount: None,
            original_currency: None,
            merchant_name: String::new(),
            remittance_information: Vec::new(),
            posted_date: NaiveDate::from_ymd_opt(2025, 1, 1).expect("valid date"),
            correlation_id: None,
            correlation_type: None,
            category_method: None,
            suggested_category: None,
            counterparty_name: None,
            counterparty_iban: None,
            counterparty_bic: None,
            bank_transaction_code: None,
            llm_justification: None,
            skip_correlation: false,
            merchant_category_code: None,
            bank_transaction_code_code: None,
            bank_transaction_code_sub_code: None,
            exchange_rate: None,
            exchange_rate_unit_currency: None,
            exchange_rate_type: None,
            exchange_rate_contract_id: None,
            reference_number: None,
            reference_number_schema: None,
            note: None,
            balance_after_transaction: None,
            balance_after_transaction_currency: None,
            creditor_account_additional_id: None,
            debtor_account_additional_id: None,
        }
    }
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
