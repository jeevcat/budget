//! Pipeline step functions for the full-sync workflow.
//!
//! Each step wraps the shared logic from the corresponding standalone handler,
//! passing a [`PipelineContext`] through so the pipeline can be triggered for a
//! specific account and optionally track a schedule run.

use apalis::prelude::*;
use apalis_workflow::Workflow;
use serde::{Deserialize, Serialize};

use budget_core::db::Db;

use super::schedule_queries::{self, RunStatus};
use super::{ApalisPool, BankProviderFactory};

/// Token carried through every pipeline step. Replaces the bare `account_id`
/// string so scheduler-triggered runs can record their outcome.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineContext {
    pub account_id: String,
    /// Set when the scheduler triggers the pipeline; `None` for manual triggers.
    pub schedule_run_id: Option<String>,
}

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
/// Returns an error if the sync fails. Marks the schedule run as failed when present.
pub async fn step_sync(
    ctx: PipelineContext,
    db: Data<Db>,
    factory: Data<BankProviderFactory>,
    pool: Data<ApalisPool>,
) -> Result<PipelineContext, BoxDynError> {
    match super::sync::sync_account(&ctx.account_id, &db, &factory).await {
        Ok(()) => Ok(ctx),
        Err(e) => {
            fail_schedule_run(&pool, &ctx, &e).await;
            Err(e)
        }
    }
}

/// Step 2: Apply categorization rules and enqueue per-transaction LLM jobs.
///
/// # Errors
///
/// Returns an error if the fan-out fails.
pub async fn step_categorize(
    ctx: PipelineContext,
    db: Data<Db>,
    apalis_pool: Data<ApalisPool>,
) -> Result<PipelineContext, BoxDynError> {
    match super::categorize::categorize_fan_out(&db, &apalis_pool).await {
        Ok(()) => Ok(ctx),
        Err(e) => {
            fail_schedule_run(&apalis_pool, &ctx, &e).await;
            Err(e)
        }
    }
}

/// Step 3 (final): Apply correlation rules and enqueue per-transaction LLM jobs.
/// Marks the schedule run as succeeded when present.
///
/// # Errors
///
/// Returns an error if the fan-out fails.
pub async fn step_correlate(
    ctx: PipelineContext,
    db: Data<Db>,
    apalis_pool: Data<ApalisPool>,
) -> Result<(), BoxDynError> {
    match super::correlate::correlate_fan_out(&db, &apalis_pool).await {
        Ok(()) => {
            if let Some(ref run_id) = ctx.schedule_run_id {
                let _ = schedule_queries::complete_schedule_run(
                    &apalis_pool,
                    run_id,
                    RunStatus::Succeeded,
                    None,
                )
                .await;
            }
            Ok(())
        }
        Err(e) => {
            fail_schedule_run(&apalis_pool, &ctx, &e).await;
            Err(e)
        }
    }
}

/// Mark the schedule run as failed if one is associated with this pipeline.
async fn fail_schedule_run(pool: &ApalisPool, ctx: &PipelineContext, error: &BoxDynError) {
    if let Some(ref run_id) = ctx.schedule_run_id {
        let _ = schedule_queries::complete_schedule_run(
            pool,
            run_id,
            RunStatus::Failed,
            Some(&error.to_string()),
        )
        .await;
    }
}
