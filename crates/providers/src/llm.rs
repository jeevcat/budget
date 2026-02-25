use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::error::ProviderError;

/// Result of LLM-based transaction categorization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategorizeResult {
    pub category_name: String,
    pub confidence: f64,
}

/// Summary of a transaction, used as input for correlation proposals.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionSummary {
    pub merchant_name: String,
    pub amount: Decimal,
    pub description: Option<String>,
    pub posted_date: NaiveDate,
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

#[trait_variant::make(Send)]
pub trait LlmProvider {
    async fn categorize(
        &self,
        merchant_name: &str,
        amount: Decimal,
        description: Option<&str>,
        existing_categories: &[String],
    ) -> Result<CategorizeResult, ProviderError>;

    async fn propose_correlation(
        &self,
        txn_a: &TransactionSummary,
        txn_b: &TransactionSummary,
    ) -> Result<CorrelationResult, ProviderError>;

    async fn propose_rule(
        &self,
        merchant_name: &str,
        user_category: &str,
    ) -> Result<ProposedRule, ProviderError>;
}
