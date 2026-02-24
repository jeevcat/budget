//! Correlate job handler: links related transactions across accounts
//! (transfers, reimbursements) using deterministic rules and LLM fallback.

use apalis::prelude::*;
use sqlx::SqlitePool;

use budget_core::db;
use budget_core::models::{CorrelationType, RuleType, TransactionId};
use budget_core::rules::{CompiledRule, compile_rule_pattern, evaluate_correlation_rules};
use budget_providers::TransactionSummary;

use super::{CorrelateJob, LlmClient};

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

/// Correlate uncorrelated transactions using a two-layer approach:
///
/// 1. **Deterministic rules** -- user-defined correlation patterns are
///    evaluated against each transaction and its candidate partners. The
///    first match wins.
/// 2. **LLM fallback** -- for plausible pairs (equal and opposite amounts),
///    the LLM proposes whether they are a transfer or reimbursement. Only
///    high-confidence results are accepted.
///
/// Both sides of a matched pair are updated with mutual correlation links.
///
/// # Errors
///
/// Returns an error if any database query or LLM call fails.
pub async fn handle_correlate_job(
    _job: CorrelateJob,
    pool: Data<SqlitePool>,
    llm: Data<LlmClient>,
) -> Result<(), BoxDynError> {
    // -- Compile correlation rules -------------------------------------------
    let raw_rules = db::list_rules_by_type(&pool, RuleType::Correlation).await?;
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

    // -- Load uncorrelated transactions --------------------------------------
    let uncorrelated = db::get_uncorrelated_transactions(&pool).await?;
    if uncorrelated.is_empty() {
        tracing::info!("no uncorrelated transactions, nothing to do");
        return Ok(());
    }

    // Track which transactions have already been paired in this run so we
    // do not correlate a transaction twice.
    let mut paired = std::collections::HashSet::<TransactionId>::new();

    let mut by_rule: u32 = 0;
    let mut by_llm: u32 = 0;
    let mut remaining: u32 = 0;

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
            remaining += 1;
            continue;
        }

        // -- Layer 1: deterministic rules ------------------------------------
        if let Some((matched_id, corr_type)) =
            evaluate_correlation_rules(txn, &candidates, &compiled_rules)
        {
            link_pair(&pool, txn.id, matched_id, corr_type).await?;
            paired.insert(txn.id);
            paired.insert(matched_id);
            by_rule += 1;
            continue;
        }

        // -- Layer 2: LLM fallback for plausible pairs -----------------------
        // Only consider candidates with equal and opposite amounts to limit
        // LLM calls to likely matches.
        let mut found_llm_match = false;
        for candidate in &candidates {
            if candidate.amount != -txn.amount {
                continue;
            }

            let summary_a = to_summary(txn);
            let summary_b = to_summary(candidate);

            let result = llm.propose_correlation(&summary_a, &summary_b).await?;

            if result.confidence >= LLM_CONFIDENCE_THRESHOLD
                && let Some(ref provider_corr_type) = result.correlation_type
            {
                let domain_type = to_domain_correlation_type(provider_corr_type);
                link_pair(&pool, txn.id, candidate.id, domain_type).await?;
                paired.insert(txn.id);
                paired.insert(candidate.id);
                by_llm += 1;
                found_llm_match = true;
                break;
            }
        }

        if !found_llm_match {
            remaining += 1;
        }
    }

    tracing::info!(
        total = uncorrelated.len(),
        by_rule,
        by_llm,
        remaining,
        "correlate job completed"
    );

    Ok(())
}

/// Persist a bidirectional correlation link between two transactions.
async fn link_pair(
    pool: &SqlitePool,
    id_a: TransactionId,
    id_b: TransactionId,
    corr_type: CorrelationType,
) -> Result<(), BoxDynError> {
    db::update_transaction_correlation(pool, id_a, id_b, corr_type).await?;
    db::update_transaction_correlation(pool, id_b, id_a, corr_type).await?;
    Ok(())
}
