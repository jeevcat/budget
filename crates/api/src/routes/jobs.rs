use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::Serialize;

use budget_jobs::{BudgetRecomputeJob, CategorizeJob, CorrelateJob, SyncJob};

use crate::routes::AppError;
use crate::state::AppState;

/// A row from the Apalis `Jobs` table, serialized for the frontend.
#[derive(Serialize)]
struct JobRecord {
    id: String,
    job_type: String,
    status: String,
    attempts: i64,
    max_attempts: i64,
    run_at: Option<String>,
    done_at: Option<String>,
    last_result: Option<String>,
    lock_by: Option<String>,
}

/// Convert a Unix timestamp (seconds) to an ISO 8601 string.
fn unix_to_iso(ts: i64) -> String {
    DateTime::<Utc>::from_timestamp(ts, 0)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_default()
}

/// Build the jobs sub-router.
///
/// Mounts:
/// - `GET /` -- list recent jobs
/// - `POST /sync/{account_id}` -- enqueue a bank sync job
/// - `POST /categorize` -- enqueue a categorization job
/// - `POST /correlate` -- enqueue a correlation job
/// - `POST /recompute` -- enqueue a budget recomputation job
///
/// # Errors
///
/// Individual handlers return `AppError` on failure.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_jobs))
        .route("/sync/{account_id}", post(trigger_sync))
        .route("/categorize", post(trigger_categorize))
        .route("/correlate", post(trigger_correlate))
        .route("/recompute", post(trigger_recompute))
}

/// List recent jobs from the Apalis `Jobs` table.
///
/// Returns the 100 most recent jobs ordered by `run_at` descending.
///
/// # Errors
///
/// Returns `AppError` on database failure.
async fn list_jobs(State(state): State<AppState>) -> Result<Json<Vec<JobRecord>>, AppError> {
    let rows = sqlx::query_as::<_, (String, String, String, i64, i64, i64, Option<i64>, Option<String>, Option<String>)>(
        "SELECT id, job_type, status, attempts, max_attempts, run_at, done_at, last_result, lock_by \
         FROM Jobs ORDER BY run_at DESC LIMIT 100",
    )
    .fetch_all(&state.pool)
    .await?;

    let jobs = rows
        .into_iter()
        .map(
            |(
                id,
                job_type,
                status,
                attempts,
                max_attempts,
                run_at,
                done_at,
                last_result,
                lock_by,
            )| {
                JobRecord {
                    id,
                    job_type,
                    status,
                    attempts,
                    max_attempts,
                    run_at: Some(unix_to_iso(run_at)),
                    done_at: done_at.map(unix_to_iso),
                    last_result,
                    lock_by,
                }
            },
        )
        .collect();

    Ok(Json(jobs))
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
