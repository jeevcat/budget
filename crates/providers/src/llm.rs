use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::error::ProviderError;

/// Result of LLM-based transaction categorization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategorizeResult {
    pub category_name: String,
    pub confidence: f64,
    /// One-sentence explanation of why this category was chosen
    pub justification: String,
}

/// Summary of a transaction, used as input for correlation proposals.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionSummary {
    pub merchant_name: String,
    pub amount: Decimal,
    pub description: Option<String>,
    pub posted_date: NaiveDate,
    pub category: Option<String>,
}

/// The type of correlation between two transactions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CorrelationType {
    Transfer,
    Reimbursement,
}

/// Which transaction field a proposed rule should match against.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MatchField {
    Merchant,
    Description,
    CounterpartyName,
    CounterpartyIban,
    CounterpartyBic,
    BankTransactionCode,
}

/// Result of LLM-based correlation analysis between two transactions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrelationResult {
    pub correlation_type: Option<CorrelationType>,
    pub confidence: f64,
}

/// A deterministic rule proposed by the LLM after a user correction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposedRule {
    pub match_field: MatchField,
    /// The pattern to match against the field
    pub match_pattern: String,
    /// Human-readable explanation of why this rule was proposed
    pub explanation: String,
}

/// Input for LLM-based transaction categorization.
pub struct CategorizeInput<'a> {
    pub merchant_name: &'a str,
    pub amount: Decimal,
    pub description: Option<&'a str>,
    pub existing_categories: &'a [String],
    pub bank_transaction_code: Option<&'a str>,
    pub counterparty_name: Option<&'a str>,
    pub counterparty_iban: Option<&'a str>,
    pub counterparty_bic: Option<&'a str>,
}

/// Full transaction context for per-transaction rule generation.
pub struct RuleContext {
    pub merchant_name: String,
    pub description: String,
    pub amount: Decimal,
    pub posted_date: NaiveDate,
    pub category_name: String,
    pub sibling_merchants: Vec<String>,
    pub existing_rule_patterns: Vec<String>,
    pub counterparty_name: Option<String>,
    pub counterparty_iban: Option<String>,
    pub counterparty_bic: Option<String>,
    pub bank_transaction_code: Option<String>,
}

#[trait_variant::make(Send)]
pub trait LlmProvider {
    async fn categorize(
        &self,
        input: &CategorizeInput<'_>,
    ) -> Result<CategorizeResult, ProviderError>;

    async fn propose_correlation(
        &self,
        txn_a: &TransactionSummary,
        txn_b: &TransactionSummary,
    ) -> Result<CorrelationResult, ProviderError>;

    async fn propose_rules(
        &self,
        context: &RuleContext,
    ) -> Result<Vec<ProposedRule>, ProviderError>;
}
