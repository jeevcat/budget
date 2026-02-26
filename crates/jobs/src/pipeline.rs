//! Pipeline step functions for the full-sync workflow.
//!
//! Each step wraps the shared logic from the corresponding standalone handler,
//! passing `account_id: String` through as a token so the pipeline can be
//! triggered for a specific account.

use apalis::prelude::*;
use apalis_workflow::Workflow;

use budget_core::db::Db;

use super::{ApalisPool, BankProviderFactory};

/// Create a [`Workflow`] whose Backend type parameter is inferred from a
/// reference to the concrete backend. This avoids naming the full
/// backend-specific storage type.
pub(crate) fn workflow_for<T, B>(_backend: &B) -> Workflow<T, T, B> {
    Workflow::new("full-sync-pipeline")
}

/// Step 1: Sync transactions for the given account.
///
/// # Errors
///
/// Returns an error if the sync fails.
pub async fn step_sync(
    account_id: String,
    db: Data<Db>,
    factory: Data<BankProviderFactory>,
) -> Result<String, BoxDynError> {
    super::sync::sync_account(&account_id, &db, &factory).await?;
    Ok(account_id)
}

/// Step 2: Apply categorization rules and enqueue per-transaction LLM jobs.
///
/// # Errors
///
/// Returns an error if the fan-out fails.
pub async fn step_categorize(
    account_id: String,
    db: Data<Db>,
    apalis_pool: Data<ApalisPool>,
) -> Result<String, BoxDynError> {
    super::categorize::categorize_fan_out(&db, &apalis_pool).await?;
    Ok(account_id)
}

/// Step 3: Apply correlation rules and enqueue per-transaction LLM jobs.
///
/// # Errors
///
/// Returns an error if the fan-out fails.
pub async fn step_correlate(
    account_id: String,
    db: Data<Db>,
    apalis_pool: Data<ApalisPool>,
) -> Result<String, BoxDynError> {
    super::correlate::correlate_fan_out(&db, &apalis_pool).await?;
    Ok(account_id)
}

/// Step 4: Recompute budget month boundaries and assignments.
///
/// # Errors
///
/// Returns an error if budget recomputation fails.
pub async fn step_recompute(_account_id: String, db: Data<Db>) -> Result<(), BoxDynError> {
    super::recompute::recompute_budgets(&db).await
}
