use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use utoipa_axum::{router::OpenApiRouter, routes};

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
pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_jobs))
        .routes(routes!(queue_counts))
        .routes(routes!(schedule_status))
        .routes(routes!(trigger_sync))
        .routes(routes!(trigger_categorize))
        .routes(routes!(trigger_correlate))
        .routes(routes!(trigger_pipeline))
}

/// List all jobs ordered by most recent first.
///
/// # Errors
///
/// Returns `AppError` on database failure.
#[utoipa::path(get, path = "/", tag = "jobs", responses((status = 200, body = Vec<JobRecord>)), security(("bearer_token" = [])))]
async fn list_jobs(State(state): State<AppState>) -> Result<Json<Vec<JobRecord>>, AppError> {
    let jobs = budget_jobs::queries::list_jobs(&state.apalis_pool)
        .await
        .map_err(|e| {
            AppError(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("database error: {e}"),
            )
        })?;
    Ok(Json(jobs))
}

/// Return queue depth per job type for the Immich-style queue display.
///
/// # Errors
///
/// Returns `AppError` on database failure.
#[utoipa::path(get, path = "/counts", tag = "jobs", responses((status = 200, body = Vec<QueueCount>)), security(("bearer_token" = [])))]
async fn queue_counts(State(state): State<AppState>) -> Result<Json<Vec<QueueCount>>, AppError> {
    let counts = budget_jobs::queries::queue_counts(&state.apalis_pool)
        .await
        .map_err(|e| {
            AppError(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("database error: {e}"),
            )
        })?;
    Ok(Json(counts))
}

/// Per-account schedule status for the scheduler UI.
///
/// # Errors
///
/// Returns `AppError` on database failure.
#[utoipa::path(get, path = "/schedule", tag = "jobs", responses((status = 200, body = Vec<AccountScheduleStatus>)), security(("bearer_token" = [])))]
async fn schedule_status(
    State(state): State<AppState>,
) -> Result<Json<Vec<AccountScheduleStatus>>, AppError> {
    let status = budget_jobs::schedule_queries::get_all_schedule_status(&state.apalis_pool)
        .await
        .map_err(|e| {
            AppError(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("database error: {e}"),
            )
        })?;
    Ok(Json(status))
}

/// Enqueue a sync job for the specified account.
///
/// Returns 202 Accepted when the job is successfully pushed to the queue.
///
/// # Errors
///
/// Returns `AppError` if the job cannot be enqueued.
#[utoipa::path(post, path = "/sync/{account_id}", tag = "jobs", params(("account_id" = AccountId, Path, description = "Account UUID")), responses((status = 202)), security(("bearer_token" = [])))]
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
#[utoipa::path(post, path = "/categorize", tag = "jobs", responses((status = 202)), security(("bearer_token" = [])))]
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
#[utoipa::path(post, path = "/correlate", tag = "jobs", responses((status = 202)), security(("bearer_token" = [])))]
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
#[utoipa::path(post, path = "/pipeline/{account_id}", tag = "jobs", params(("account_id" = AccountId, Path, description = "Account UUID")), responses((status = 202)), security(("bearer_token" = [])))]
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
