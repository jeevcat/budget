use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};

use budget_core::models::AccountId;
use budget_jobs::queries::{JobRecord, QueueCount};
use budget_jobs::schedule_queries::AccountScheduleStatus;
use budget_jobs::{CategorizeJob, CorrelateJob, PipelineContext, SyncJob};

use crate::routes::AppError;
use crate::state::AppState;

/// Build the jobs sub-router.
///
/// Mounts:
/// - `GET /` -- list all jobs
/// - `GET /counts` -- queue depth per job type
/// - `GET /schedule` -- per-account schedule status
/// - `POST /sync/{account_id}` -- enqueue a bank sync job
/// - `POST /categorize` -- enqueue a categorization job
/// - `POST /correlate` -- enqueue a correlation job
/// - `POST /pipeline/{account_id}` -- enqueue a full-sync pipeline
///
/// # Errors
///
/// Individual handlers return `AppError` on failure.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_jobs))
        .route("/counts", get(queue_counts))
        .route("/schedule", get(schedule_status))
        .route("/sync/{account_id}", post(trigger_sync))
        .route("/categorize", post(trigger_categorize))
        .route("/correlate", post(trigger_correlate))
        .route("/pipeline/{account_id}", post(trigger_pipeline))
}

/// List all jobs ordered by most recent first.
///
/// # Errors
///
/// Returns `AppError` on database failure.
async fn list_jobs(State(state): State<AppState>) -> Result<Json<Vec<JobRecord>>, AppError> {
    let jobs = budget_jobs::queries::list_jobs(&state.apalis_pool).await?;
    Ok(Json(jobs))
}

/// Return queue depth per job type for the Immich-style queue display.
///
/// # Errors
///
/// Returns `AppError` on database failure.
async fn queue_counts(State(state): State<AppState>) -> Result<Json<Vec<QueueCount>>, AppError> {
    let counts = budget_jobs::queries::queue_counts(&state.apalis_pool).await?;
    Ok(Json(counts))
}

/// Per-account schedule status for the scheduler UI.
///
/// # Errors
///
/// Returns `AppError` on database failure.
async fn schedule_status(
    State(state): State<AppState>,
) -> Result<Json<Vec<AccountScheduleStatus>>, AppError> {
    let status = budget_jobs::schedule_queries::get_all_schedule_status(&state.apalis_pool).await?;
    Ok(Json(status))
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
    Path(account_id): Path<AccountId>,
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

/// Enqueue a full-sync pipeline (sync, categorize, correlate)
/// for the specified account.
///
/// Returns 202 Accepted when the pipeline is successfully started.
///
/// # Errors
///
/// Returns `AppError` if the pipeline cannot be enqueued.
async fn trigger_pipeline(
    State(state): State<AppState>,
    Path(account_id): Path<AccountId>,
) -> Result<StatusCode, AppError> {
    let ctx = PipelineContext {
        account_id,
        schedule_run_id: None,
    };
    state
        .pipeline_storage
        .push_start(ctx)
        .await
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(StatusCode::ACCEPTED)
}
