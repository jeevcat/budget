//! Categorize job handler: applies deterministic rules and LLM fallback to
//! assign categories to uncategorized transactions.

use apalis::prelude::*;
use sqlx::SqlitePool;

use budget_core::db;
use budget_core::models::RuleType;
use budget_core::rules::{CompiledRule, compile_rule_pattern, evaluate_categorization_rules};

use super::{CategorizeJob, LlmClient};

/// Minimum LLM confidence required to auto-assign a category.
const LLM_CONFIDENCE_THRESHOLD: f64 = 0.8;

/// Categorize all uncategorized transactions using a two-layer approach:
///
/// 1. **Deterministic rules** -- user-defined merchant, description, and
///    amount-range patterns are evaluated first. The highest-priority match
///    wins.
/// 2. **LLM fallback** -- for transactions that no rule matched, the LLM
///    provider proposes a category with a confidence score. Only results at
///    or above the confidence threshold are accepted; the rest remain
///    uncategorized for manual review.
///
/// # Errors
///
/// Returns an error if any database query or LLM call fails.
pub async fn handle_categorize_job(
    _job: CategorizeJob,
    pool: Data<SqlitePool>,
    llm: Data<LlmClient>,
) -> Result<(), BoxDynError> {
    // -- Compile categorization rules ----------------------------------------
    let raw_rules = db::list_rules_by_type(&pool, RuleType::Categorization).await?;
    let compiled_rules: Vec<CompiledRule> = raw_rules
        .iter()
        .filter_map(|rule| match compile_rule_pattern(rule) {
            Ok(compiled) => Some(compiled),
            Err(e) => {
                tracing::warn!(rule_id = %rule.id, error = %e, "skipping rule with invalid pattern");
                None
            }
        })
        .collect();

    // -- Load uncategorized transactions -------------------------------------
    let uncategorized = db::get_uncategorized_transactions(&pool).await?;
    if uncategorized.is_empty() {
        tracing::info!("no uncategorized transactions, nothing to do");
        return Ok(());
    }

    let mut by_rule: u32 = 0;
    let mut by_llm: u32 = 0;
    let mut remaining: u32 = 0;

    for txn in &uncategorized {
        // Layer 1: deterministic rules
        if let Some(category_id) = evaluate_categorization_rules(txn, &compiled_rules) {
            db::update_transaction_category(&pool, txn.id, category_id).await?;
            by_rule += 1;
            continue;
        }

        // Layer 2: LLM fallback
        let description = if txn.description.is_empty() {
            None
        } else {
            Some(txn.description.as_str())
        };

        let result = llm
            .categorize(&txn.merchant_name, txn.amount, description)
            .await?;

        if result.confidence < LLM_CONFIDENCE_THRESHOLD {
            tracing::debug!(
                txn_id = %txn.id,
                merchant = %txn.merchant_name,
                confidence = result.confidence,
                "LLM confidence below threshold, leaving uncategorized"
            );
            remaining += 1;
            continue;
        }

        // Resolve the category name to a domain CategoryId
        if let Some(category) = db::get_category_by_name(&pool, &result.category_name).await? {
            db::update_transaction_category(&pool, txn.id, category.id).await?;
            by_llm += 1;
        } else {
            tracing::debug!(
                txn_id = %txn.id,
                category_name = %result.category_name,
                "LLM proposed unknown category, leaving uncategorized"
            );
            remaining += 1;
        }
    }

    tracing::info!(
        total = uncategorized.len(),
        by_rule,
        by_llm,
        remaining,
        "categorize job completed"
    );

    Ok(())
}
