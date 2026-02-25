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
/// The LLM receives the list of existing category names so it prefers known
/// categories over inventing new ones. When the LLM is consulted but the
/// transaction remains uncategorized (low confidence or unknown category),
/// the proposed name is saved as `suggested_category` for the user to
/// review.
///
/// # Errors
///
/// Returns an error if any database query or LLM call fails.
pub(crate) async fn categorize_transactions(
    pool: &SqlitePool,
    llm: &LlmClient,
) -> Result<(), BoxDynError> {
    // -- Compile categorization rules ----------------------------------------
    let raw_rules = db::list_rules_by_type(pool, RuleType::Categorization).await?;
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
    let uncategorized = db::get_uncategorized_transactions(pool).await?;
    if uncategorized.is_empty() {
        tracing::info!("no uncategorized transactions, nothing to do");
        return Ok(());
    }

    // -- Load existing category names for the LLM prompt ---------------------
    let existing_categories = db::list_category_names(pool).await?;

    let mut by_rule: u32 = 0;
    let mut by_llm: u32 = 0;
    let mut suggested: u32 = 0;
    let remaining: u32 = 0;

    for txn in &uncategorized {
        // Layer 1: deterministic rules
        if let Some(category_id) = evaluate_categorization_rules(txn, &compiled_rules) {
            db::update_transaction_category(pool, txn.id, category_id).await?;
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
            .categorize(
                &txn.merchant_name,
                txn.amount,
                description,
                &existing_categories,
            )
            .await?;

        if result.confidence < LLM_CONFIDENCE_THRESHOLD {
            tracing::debug!(
                txn_id = %txn.id,
                merchant = %txn.merchant_name,
                confidence = result.confidence,
                suggested = %result.category_name,
                "LLM confidence below threshold, saving suggestion"
            );
            db::update_transaction_suggested_category(pool, txn.id, &result.category_name).await?;
            suggested += 1;
            continue;
        }

        // Resolve the category name to a domain CategoryId
        if let Some(category) = db::get_category_by_name(pool, &result.category_name).await? {
            db::update_transaction_category(pool, txn.id, category.id).await?;
            by_llm += 1;
        } else {
            tracing::debug!(
                txn_id = %txn.id,
                category_name = %result.category_name,
                "LLM proposed unknown category, saving suggestion"
            );
            db::update_transaction_suggested_category(pool, txn.id, &result.category_name).await?;
            suggested += 1;
        }
    }

    tracing::info!(
        total = uncategorized.len(),
        by_rule,
        by_llm,
        suggested,
        remaining,
        "categorize job completed"
    );

    Ok(())
}

/// Apalis handler that delegates to [`categorize_transactions`].
///
/// # Errors
///
/// Returns an error if categorization fails.
pub async fn handle_categorize_job(
    _job: CategorizeJob,
    pool: Data<SqlitePool>,
    llm: Data<LlmClient>,
) -> Result<(), BoxDynError> {
    categorize_transactions(&pool, &llm).await
}
