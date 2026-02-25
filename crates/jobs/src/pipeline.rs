//! Pipeline step functions for the full-sync workflow.
//!
//! Each step wraps the shared logic from the corresponding standalone handler,
//! passing `account_id: String` through as a token so the pipeline can be
//! triggered for a specific account.

use apalis::prelude::*;
use sqlx::SqlitePool;

use super::{BankProviderFactory, LlmClient};

/// Step 1: Sync transactions for the given account.
///
/// # Errors
///
/// Returns an error if the sync fails.
pub async fn step_sync(
    account_id: String,
    pool: Data<SqlitePool>,
    factory: Data<BankProviderFactory>,
) -> Result<String, BoxDynError> {
    super::sync::sync_account(&account_id, &pool, &factory).await?;
    Ok(account_id)
}

/// Step 2: Categorize all uncategorized transactions.
///
/// # Errors
///
/// Returns an error if categorization fails.
pub async fn step_categorize(
    account_id: String,
    pool: Data<SqlitePool>,
    llm: Data<LlmClient>,
) -> Result<String, BoxDynError> {
    super::categorize::categorize_transactions(&pool, &llm).await?;
    Ok(account_id)
}

/// Step 3: Correlate uncorrelated transactions.
///
/// # Errors
///
/// Returns an error if correlation fails.
pub async fn step_correlate(
    account_id: String,
    pool: Data<SqlitePool>,
    llm: Data<LlmClient>,
) -> Result<String, BoxDynError> {
    super::correlate::correlate_transactions(&pool, &llm).await?;
    Ok(account_id)
}

/// Step 4: Recompute budget month boundaries and assignments.
///
/// # Errors
///
/// Returns an error if budget recomputation fails.
pub async fn step_recompute(
    _account_id: String,
    pool: Data<SqlitePool>,
) -> Result<(), BoxDynError> {
    super::recompute::recompute_budgets(&pool).await
}
