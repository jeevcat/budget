//! Categorize job handler: applies deterministic rules and LLM fallback to
//! assign categories to uncategorized transactions.
//!
//! The fan-out handler applies cheap deterministic rules in-line, then enqueues
//! one `CategorizeTransactionJob` per remaining transaction for LLM processing.

use apalis::prelude::*;

use budget_core::db::Db;
use budget_core::models::{CategoryMethod, RuleType, TransactionId};
use budget_core::rules::{CompiledRule, compile_rule_pattern, evaluate_categorization_rules};

use super::{ApalisPool, CategorizeJob, CategorizeTransactionJob, LlmClient};

/// Minimum LLM confidence required to auto-assign a category.
const LLM_CONFIDENCE_THRESHOLD: f64 = 0.8;

/// Apply deterministic rules in-line, then enqueue one
/// `CategorizeTransactionJob` per remaining transaction for LLM processing.
///
/// # Errors
///
/// Returns an error if any database query or enqueue operation fails.
pub(crate) async fn categorize_fan_out(
    db: &Db,
    apalis_pool: &ApalisPool,
) -> Result<(), BoxDynError> {
    // -- Compile categorization rules ----------------------------------------
    let raw_rules = db.list_rules_by_type(RuleType::Categorization).await?;
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
    let uncategorized = db.get_uncategorized_transactions().await?;
    if uncategorized.is_empty() {
        tracing::info!("no uncategorized transactions, nothing to do");
        return Ok(());
    }

    let mut by_rule: u32 = 0;
    let mut enqueued: u32 = 0;

    let mut storage = apalis_postgres::PostgresStorage::new(apalis_pool);

    for txn in &uncategorized {
        if let Some(category_id) = evaluate_categorization_rules(txn, &compiled_rules) {
            db.update_transaction_category(txn.id, category_id, CategoryMethod::Rule)
                .await?;
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
    db: Data<Db>,
    llm: Data<LlmClient>,
) -> Result<(), BoxDynError> {
    let txn_id: TransactionId = job
        .transaction_id
        .parse::<uuid::Uuid>()
        .map(TransactionId::from_uuid)
        .map_err(|e| format!("invalid transaction id: {e}"))?;

    let txn = db
        .get_transaction_by_id(txn_id)
        .await?
        .ok_or_else(|| format!("transaction {txn_id} not found"))?;

    // Race-safe: already categorized by another job or rule
    if txn.category_id.is_some() {
        tracing::debug!(txn_id = %txn_id, "already categorized, skipping");
        return Ok(());
    }

    let existing_categories = db.list_category_names().await?;

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
        db.update_transaction_suggested_category(txn_id, &result.category_name)
            .await?;
        return Ok(());
    }

    if let Some(category) = db.get_category_by_name(&result.category_name).await? {
        db.update_transaction_category(txn_id, category.id, CategoryMethod::Llm)
            .await?;
        tracing::debug!(txn_id = %txn_id, category = %result.category_name, "categorized by LLM");
    } else {
        tracing::debug!(
            txn_id = %txn_id,
            category_name = %result.category_name,
            "LLM proposed unknown category, saving suggestion"
        );
        db.update_transaction_suggested_category(txn_id, &result.category_name)
            .await?;
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
    db: Data<Db>,
    apalis_pool: Data<ApalisPool>,
) -> Result<(), BoxDynError> {
    categorize_fan_out(&db, &apalis_pool).await
}
