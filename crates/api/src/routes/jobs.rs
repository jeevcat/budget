use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Serialize;
use sqlx::Row;

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

/// Individual job record for the jobs list endpoint.
#[derive(Serialize)]
struct JobRecord {
    job_type: String,
    status: String,
    run_at: String,
}

/// Build the jobs sub-router.
///
/// Mounts:
/// - `GET /` -- list all jobs
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
        .route("/", get(list_jobs))
        .route("/counts", get(queue_counts))
        .route("/sync/{account_id}", post(trigger_sync))
        .route("/categorize", post(trigger_categorize))
        .route("/correlate", post(trigger_correlate))
        .route("/recompute", post(trigger_recompute))
        .route("/pipeline/{account_id}", post(trigger_pipeline))
}

/// List all jobs ordered by most recent first.
///
/// # Errors
///
/// Returns `AppError` on database failure.
async fn list_jobs(State(state): State<AppState>) -> Result<Json<Vec<JobRecord>>, AppError> {
    let rows = sqlx::query(&format!(
        "SELECT job_type, status, CAST(run_at AS TEXT) as run_at FROM {} ORDER BY run_at DESC",
        budget_jobs::JOBS_TABLE,
    ))
    .fetch_all(state.db.pool())
    .await?;

    let jobs = rows
        .iter()
        .map(|row| {
            Ok(JobRecord {
                job_type: row.try_get("job_type")?,
                status: row.try_get("status")?,
                run_at: row.try_get("run_at")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?;

    Ok(Json(jobs))
}

/// Return queue depth per job type for the Immich-style queue display.
///
/// # Errors
///
/// Returns `AppError` on database failure.
async fn queue_counts(State(state): State<AppState>) -> Result<Json<Vec<QueueCount>>, AppError> {
    let rows = sqlx::query(&format!(
        "SELECT job_type, status, COUNT(*) as cnt FROM {} GROUP BY job_type, status",
        budget_jobs::JOBS_TABLE,
    ))
    .fetch_all(state.db.pool())
    .await?;

    // Aggregate into one QueueCount per job_type
    let mut map = std::collections::HashMap::<String, QueueCount>::new();
    for row in rows {
        let job_type: String = row.try_get("job_type")?;
        let status: String = row.try_get("status")?;
        let count: i64 = row.try_get("cnt")?;
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
