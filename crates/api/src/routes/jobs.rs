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
    /// For pipeline jobs, the current step index (0=sync, 1=categorize, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pipeline_step: Option<u64>,
}

/// Extract the pipeline step index from the apalis metadata JSON.
///
/// The metadata stores `WorkflowContext { step_index }` under the fully
/// qualified type name key. Returns `None` for non-pipeline jobs.
fn extract_pipeline_step(metadata: &str) -> Option<u64> {
    let obj: serde_json::Value = serde_json::from_str(metadata).ok()?;
    obj.get("apalis_workflow::sequential::context::WorkflowContext")?
        .get("step_index")?
        .as_u64()
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
/// - `POST /pipeline/{account_id}` -- enqueue a full-sync pipeline
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
        .route("/pipeline/{account_id}", post(trigger_pipeline))
}

/// List recent jobs from the Apalis `Jobs` table.
///
/// Returns the 100 most recent jobs ordered by `run_at` descending.
///
/// # Errors
///
/// Returns `AppError` on database failure.
async fn list_jobs(State(state): State<AppState>) -> Result<Json<Vec<JobRecord>>, AppError> {
    let rows = sqlx::query_as::<_, (String, String, String, i64, i64, i64, Option<i64>, Option<String>, Option<String>, Option<String>)>(
        "SELECT id, job_type, status, attempts, max_attempts, run_at, done_at, last_result, lock_by, metadata \
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
                metadata,
            )| {
                let pipeline_step = metadata.as_deref().and_then(extract_pipeline_step);
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
                    pipeline_step,
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
