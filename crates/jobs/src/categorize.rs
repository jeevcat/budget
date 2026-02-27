//! Categorize job handler: applies deterministic rules and LLM fallback to
//! assign categories to uncategorized transactions.
//!
//! The fan-out handler applies cheap deterministic rules in-line, then enqueues
//! one `CategorizeTransactionJob` per remaining transaction for LLM processing.

use apalis::prelude::*;

use budget_core::db::Db;
use budget_core::models::{CategoryMethod, RuleType, TransactionId};
use budget_core::rules::{CompiledRule, compile_rule, evaluate_categorization_rules};

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
        .filter_map(|rule| match compile_rule(rule) {
            Ok(compiled) => Some(compiled),
            Err(e) => {
                tracing::warn!(rule_id = %rule.id, error = %e, "skipping rule with invalid pattern");
                None
            }
        })
        .collect();

    // -- Load rule-eligible transactions (uncategorized + LLM-categorized) ----
    let eligible = db.get_rule_eligible_transactions().await?;
    if eligible.is_empty() {
        tracing::info!("no rule-eligible transactions, nothing to do");
        return Ok(());
    }

    let mut by_rule: u32 = 0;
    let mut enqueued: u32 = 0;
    let mut skipped: u32 = 0;

    let mut storage = apalis_postgres::PostgresStorage::new(apalis_pool);

    for txn in &eligible {
        if let Some(category_id) = evaluate_categorization_rules(txn, &compiled_rules) {
            db.update_transaction_category(txn.id, category_id, CategoryMethod::Rule, None)
                .await?;
            by_rule += 1;
            continue;
        }

        // Only enqueue truly uncategorized transactions for LLM; already
        // LLM-categorized ones that didn't match a rule keep their category.
        if txn.category_id.is_some() {
            skipped += 1;
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
        total = eligible.len(),
        by_rule,
        enqueued,
        skipped,
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

    let input = budget_providers::CategorizeInput {
        merchant_name: &txn.merchant_name,
        amount: txn.amount,
        description,
        existing_categories: &existing_categories,
        bank_transaction_code: txn.bank_transaction_code.as_deref(),
        counterparty_name: txn.counterparty_name.as_deref(),
        counterparty_iban: txn.counterparty_iban.as_deref(),
        counterparty_bic: txn.counterparty_bic.as_deref(),
    };

    let result = llm.categorize(&input).await?;

    let justification = Some(result.justification.as_str());

    if result.confidence < LLM_CONFIDENCE_THRESHOLD {
        tracing::debug!(
            txn_id = %txn_id,
            merchant = %txn.merchant_name,
            confidence = result.confidence,
            suggested = %result.category_name,
            "LLM confidence below threshold, saving suggestion"
        );
        db.update_transaction_suggested_category(txn_id, &result.category_name, justification)
            .await?;
        return Ok(());
    }

    if let Some(category) = db.get_category_by_name(&result.category_name).await? {
        db.update_transaction_category(txn_id, category.id, CategoryMethod::Llm, justification)
            .await?;
        tracing::debug!(txn_id = %txn_id, category = %result.category_name, "categorized by LLM");
    } else {
        tracing::debug!(
            txn_id = %txn_id,
            category_name = %result.category_name,
            "LLM proposed unknown category, saving suggestion"
        );
        db.update_transaction_suggested_category(txn_id, &result.category_name, justification)
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
