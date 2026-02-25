use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Serialize;

use budget_jobs::{BudgetRecomputeJob, CategorizeJob, CorrelateJob, SyncJob};

use crate::routes::AppError;
use crate::state::AppState;

/// Queue depth per job type for the Immich-style queue display.
#[derive(Serialize)]
struct QueueCount {
    job_type: String,
    active: i64,
    waiting: i64,
    completed: i64,
    failed: i64,
}

/// Build the jobs sub-router.
///
/// Mounts:
/// - `GET /counts` -- queue depth per job type
/// - `POST /sync/{account_id}` -- enqueue a bank sync job
/// - `POST /categorize` -- enqueue a categorization job
/// - `POST /correlate` -- enqueue a correlation job
/// - `POST /recompute` -- enqueue a budget recomputation job
/// - `POST /pipeline/{account_id}` -- enqueue a full-sync pipeline
///
/// # Errors
///
/// Individual handlers return `AppError` on failure.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/counts", get(queue_counts))
        .route("/sync/{account_id}", post(trigger_sync))
        .route("/categorize", post(trigger_categorize))
        .route("/correlate", post(trigger_correlate))
        .route("/recompute", post(trigger_recompute))
        .route("/pipeline/{account_id}", post(trigger_pipeline))
}

/// Return queue depth per job type for the Immich-style queue display.
///
/// # Errors
///
/// Returns `AppError` on database failure.
async fn queue_counts(State(state): State<AppState>) -> Result<Json<Vec<QueueCount>>, AppError> {
    let rows = sqlx::query_as::<_, (String, String, i64)>(
        "SELECT job_type, status, COUNT(*) as count FROM Jobs GROUP BY job_type, status",
    )
    .fetch_all(state.db.pool())
    .await?;

    // Aggregate into one QueueCount per job_type
    let mut map = std::collections::HashMap::<String, QueueCount>::new();
    for (job_type, status, count) in rows {
        let entry = map.entry(job_type.clone()).or_insert_with(|| QueueCount {
            job_type,
            active: 0,
            waiting: 0,
            completed: 0,
            failed: 0,
        });
        match status.as_str() {
            "Pending" => entry.waiting += count,
            "Running" => entry.active += count,
            "Done" => entry.completed += count,
            "Failed" | "Killed" => entry.failed += count,
            _ => {}
        }
    }

    let mut counts: Vec<QueueCount> = map.into_values().collect();
    counts.sort_by(|a, b| a.job_type.cmp(&b.job_type));
    Ok(Json(counts))
}

/// Enqueue a sync job for the specified account.
///
/// Returns 202 Accepted when the job is successfully pushed to the queue.
///
/// # Errors
///
/// Returns `AppError` if the job cannot be enqueued.
async fn trigger_sync(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
) -> Result<StatusCode, AppError> {
    state
        .sync_storage
        .push(SyncJob { account_id })
        .await
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(StatusCode::ACCEPTED)
}

/// Enqueue a categorization job.
///
/// Returns 202 Accepted when the job is successfully pushed to the queue.
///
/// # Errors
///
/// Returns `AppError` if the job cannot be enqueued.
async fn trigger_categorize(State(state): State<AppState>) -> Result<StatusCode, AppError> {
    state
        .categorize_storage
        .push(CategorizeJob)
        .await
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(StatusCode::ACCEPTED)
}

/// Enqueue a correlation job.
///
/// Returns 202 Accepted when the job is successfully pushed to the queue.
///
/// # Errors
///
/// Returns `AppError` if the job cannot be enqueued.
async fn trigger_correlate(State(state): State<AppState>) -> Result<StatusCode, AppError> {
    state
        .correlate_storage
        .push(CorrelateJob)
        .await
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(StatusCode::ACCEPTED)
}

/// Enqueue a budget recomputation job.
///
/// Returns 202 Accepted when the job is successfully pushed to the queue.
///
/// # Errors
///
/// Returns `AppError` if the job cannot be enqueued.
async fn trigger_recompute(State(state): State<AppState>) -> Result<StatusCode, AppError> {
    state
        .recompute_storage
        .push(BudgetRecomputeJob)
        .await
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(StatusCode::ACCEPTED)
}

/// Enqueue a full-sync pipeline (sync, categorize, correlate, recompute)
/// for the specified account.
///
/// Returns 202 Accepted when the pipeline is successfully started.
///
/// # Errors
///
/// Returns `AppError` if the pipeline cannot be enqueued.
async fn trigger_pipeline(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
) -> Result<StatusCode, AppError> {
    state
        .pipeline_storage
        .push_start(account_id)
        .await
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(StatusCode::ACCEPTED)
}
