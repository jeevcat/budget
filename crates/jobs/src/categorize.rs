//! Categorize job handler: applies deterministic rules and LLM fallback to
//! assign categories to uncategorized transactions.
//!
//! The fan-out handler applies cheap deterministic rules in-line, then enqueues
//! one `CategorizeTransactionJob` per remaining transaction for LLM processing.

use apalis::prelude::*;
use apalis_sqlite::SqliteStorage;
use sqlx::SqlitePool;

use budget_core::db;
use budget_core::models::{RuleType, TransactionId};
use budget_core::rules::{CompiledRule, compile_rule_pattern, evaluate_categorization_rules};

use super::{CategorizeJob, CategorizeTransactionJob, LlmClient};

/// Minimum LLM confidence required to auto-assign a category.
const LLM_CONFIDENCE_THRESHOLD: f64 = 0.8;

/// Apply deterministic rules in-line, then enqueue one
/// `CategorizeTransactionJob` per remaining transaction for LLM processing.
///
/// # Errors
///
/// Returns an error if any database query or enqueue operation fails.
pub(crate) async fn categorize_fan_out(pool: &SqlitePool) -> Result<(), BoxDynError> {
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

    let mut by_rule: u32 = 0;
    let mut enqueued: u32 = 0;
    let mut storage = SqliteStorage::<CategorizeTransactionJob, _, _>::new(pool);

    for txn in &uncategorized {
        if let Some(category_id) = evaluate_categorization_rules(txn, &compiled_rules) {
            db::update_transaction_category(pool, txn.id, category_id).await?;
            by_rule += 1;
            continue;
        }

        storage
            .push(CategorizeTransactionJob {
                transaction_id: txn.id.to_string(),
            })
            .await?;
        enqueued += 1;
    }

    tracing::info!(
        total = uncategorized.len(),
        by_rule,
        enqueued,
        "categorize fan-out completed"
    );

    Ok(())
}

/// Categorize a single transaction via LLM.
///
/// Loads the transaction by ID, checks it is still uncategorized (race-safe
/// bail-out), then calls the LLM to propose a category.
///
/// # Errors
///
/// Returns an error if the database query or LLM call fails.
pub async fn handle_categorize_transaction_job(
    job: CategorizeTransactionJob,
    pool: Data<SqlitePool>,
    llm: Data<LlmClient>,
) -> Result<(), BoxDynError> {
    let txn_id: TransactionId = job
        .transaction_id
        .parse::<uuid::Uuid>()
        .map(TransactionId::from_uuid)
        .map_err(|e| format!("invalid transaction id: {e}"))?;

    let txn = db::get_transaction_by_id(&pool, txn_id)
        .await?
        .ok_or_else(|| format!("transaction {txn_id} not found"))?;

    // Race-safe: already categorized by another job or rule
    if txn.category_id.is_some() {
        tracing::debug!(txn_id = %txn_id, "already categorized, skipping");
        return Ok(());
    }

    let existing_categories = db::list_category_names(&pool).await?;

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
            txn_id = %txn_id,
            merchant = %txn.merchant_name,
            confidence = result.confidence,
            suggested = %result.category_name,
            "LLM confidence below threshold, saving suggestion"
        );
        db::update_transaction_suggested_category(&pool, txn_id, &result.category_name).await?;
        return Ok(());
    }

    if let Some(category) = db::get_category_by_name(&pool, &result.category_name).await? {
        db::update_transaction_category(&pool, txn_id, category.id).await?;
        tracing::debug!(txn_id = %txn_id, category = %result.category_name, "categorized by LLM");
    } else {
        tracing::debug!(
            txn_id = %txn_id,
            category_name = %result.category_name,
            "LLM proposed unknown category, saving suggestion"
        );
        db::update_transaction_suggested_category(&pool, txn_id, &result.category_name).await?;
    }

    Ok(())
}

/// Apalis handler for the fan-out job: applies rules then enqueues per-txn jobs.
///
/// # Errors
///
/// Returns an error if the fan-out fails.
pub async fn handle_categorize_job(
    _job: CategorizeJob,
    pool: Data<SqlitePool>,
) -> Result<(), BoxDynError> {
    categorize_fan_out(&pool).await
}
