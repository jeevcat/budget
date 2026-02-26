//! Correlate job handler: links related transactions across accounts
//! (transfers, reimbursements) using deterministic rules and LLM fallback.
//!
//! The fan-out handler applies cheap deterministic rules in-line, then enqueues
//! one `CorrelateTransactionJob` per remaining transaction for LLM processing.

use apalis::prelude::*;

use budget_core::db::Db;
use budget_core::models::{CorrelationType, RuleType, TransactionId};
use budget_core::rules::{CompiledRule, compile_rule, evaluate_correlation_rules};
use budget_providers::TransactionSummary;

use super::{ApalisPool, CorrelateJob, CorrelateTransactionJob, LlmClient};

/// Minimum LLM confidence required to auto-link a correlation.
const LLM_CONFIDENCE_THRESHOLD: f64 = 0.8;

/// Convert a `budget_providers::CorrelationType` to the domain
/// `budget_core::models::CorrelationType`.
fn to_domain_correlation_type(
    provider_type: &budget_providers::CorrelationType,
) -> CorrelationType {
    match provider_type {
        budget_providers::CorrelationType::Transfer => CorrelationType::Transfer,
        budget_providers::CorrelationType::Reimbursement => CorrelationType::Reimbursement,
    }
}

/// Build a [`TransactionSummary`] from a domain transaction for LLM input.
fn to_summary(txn: &budget_core::models::Transaction) -> TransactionSummary {
    TransactionSummary {
        merchant_name: txn.merchant_name.clone(),
        amount: txn.amount,
        description: if txn.description.is_empty() {
            None
        } else {
            Some(txn.description.clone())
        },
        posted_date: txn.posted_date,
    }
}

/// Apply deterministic rules in-line, then enqueue one
/// `CorrelateTransactionJob` per remaining transaction for LLM processing.
///
/// # Errors
///
/// Returns an error if any database query or enqueue operation fails.
pub(crate) async fn correlate_fan_out(
    db: &Db,
    apalis_pool: &ApalisPool,
) -> Result<(), BoxDynError> {
    // -- Compile correlation rules -------------------------------------------
    let raw_rules = db.list_rules_by_type(RuleType::Correlation).await?;
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

    // -- Load uncorrelated transactions --------------------------------------
    let uncorrelated = db.get_uncorrelated_transactions().await?;
    if uncorrelated.is_empty() {
        tracing::info!("no uncorrelated transactions, nothing to do");
        return Ok(());
    }

    // Track which transactions have already been paired in this run so we
    // do not correlate a transaction twice.
    let mut paired = std::collections::HashSet::<TransactionId>::new();

    let mut by_rule: u32 = 0;
    let mut enqueued: u32 = 0;

    let mut storage = apalis_postgres::PostgresStorage::new(apalis_pool);

    for txn in &uncorrelated {
        if paired.contains(&txn.id) {
            continue;
        }

        // Candidates: other uncorrelated transactions not yet paired in this run.
        // Cloned because `evaluate_correlation_rules` takes `&[Transaction]`.
        let candidates: Vec<budget_core::models::Transaction> = uncorrelated
            .iter()
            .filter(|c| c.id != txn.id && !paired.contains(&c.id))
            .cloned()
            .collect();

        if candidates.is_empty() {
            continue;
        }

        // -- Deterministic rules ---------------------------------------------
        if let Some((matched_id, corr_type)) =
            evaluate_correlation_rules(txn, &candidates, &compiled_rules)
        {
            link_pair(db, txn.id, matched_id, corr_type).await?;
            paired.insert(txn.id);
            paired.insert(matched_id);
            by_rule += 1;
            continue;
        }

        // -- Enqueue for LLM processing -------------------------------------
        storage
            .push(CorrelateTransactionJob {
                transaction_id: txn.id.to_string(),
            })
            .await?;
        enqueued += 1;
    }

    tracing::info!(
        total = uncorrelated.len(),
        by_rule,
        enqueued,
        "correlate fan-out completed"
    );

    Ok(())
}

/// Correlate a single transaction via LLM.
///
/// Loads the transaction by ID, checks it is still uncorrelated (race-safe
/// bail-out), finds candidates with the opposite amount, then calls the LLM
/// to propose whether they are correlated.
///
/// # Errors
///
/// Returns an error if the database query or LLM call fails.
pub async fn handle_correlate_transaction_job(
    job: CorrelateTransactionJob,
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

    // Race-safe: already correlated by another job or rule
    if txn.correlation_id.is_some() {
        tracing::debug!(txn_id = %txn_id, "already correlated, skipping");
        return Ok(());
    }

    // Find candidates: uncorrelated transactions with opposite amount
    let candidates = db.get_correlation_candidates(-txn.amount, txn_id).await?;

    for candidate in &candidates {
        let summary_a = to_summary(&txn);
        let summary_b = to_summary(candidate);

        let result = llm.propose_correlation(&summary_a, &summary_b).await?;

        if result.confidence >= LLM_CONFIDENCE_THRESHOLD
            && let Some(ref provider_corr_type) = result.correlation_type
        {
            let domain_type = to_domain_correlation_type(provider_corr_type);
            link_pair(&db, txn_id, candidate.id, domain_type).await?;
            tracing::debug!(
                txn_id = %txn_id,
                correlated_with = %candidate.id,
                "correlated by LLM"
            );
            return Ok(());
        }
    }

    tracing::debug!(txn_id = %txn_id, "no LLM correlation match found");
    Ok(())
}

/// Apalis handler for the fan-out job: applies rules then enqueues per-txn jobs.
///
/// # Errors
///
/// Returns an error if the fan-out fails.
pub async fn handle_correlate_job(
    _job: CorrelateJob,
    db: Data<Db>,
    apalis_pool: Data<ApalisPool>,
) -> Result<(), BoxDynError> {
    correlate_fan_out(&db, &apalis_pool).await
}

/// Persist a bidirectional correlation link between two transactions.
async fn link_pair(
    db: &Db,
    id_a: TransactionId,
    id_b: TransactionId,
    corr_type: CorrelationType,
) -> Result<(), BoxDynError> {
    db.update_transaction_correlation(id_a, id_b, corr_type)
        .await?;
    db.update_transaction_correlation(id_b, id_a, corr_type)
        .await?;
    Ok(())
}
