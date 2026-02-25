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

/// Queue depth per job type for the Immich-style queue display.
#[derive(Serialize)]
struct QueueCount {
    job_type: String,
    active: i64,
    waiting: i64,
    completed: i64,
    failed: i64,
}

/// Extract the pipeline step index from the apalis metadata JSON.
///
/// The metadata stores `WorkflowContext { step_index }` under the fully
/// qualified type name key. For pipeline jobs without a `WorkflowContext`
/// (e.g. newly enqueued), defaults to step 0 (Sync).
/// Returns `None` for non-pipeline jobs.
fn extract_pipeline_step(job_type: &str, metadata: &str) -> Option<u64> {
    let is_pipeline = job_type.contains("Vec<u8>") || job_type == "Vec<u8>";
    if !is_pipeline {
        return None;
    }

    let obj: serde_json::Value = serde_json::from_str(metadata).ok()?;
    let step = obj
        .get("apalis_workflow::sequential::context::WorkflowContext")
        .and_then(|ctx| ctx.get("step_index"))
        .and_then(serde_json::Value::as_u64);

    // Pipeline job without WorkflowContext metadata defaults to step 0
    Some(step.unwrap_or(0))
}

/// Sanitize the `last_result` JSON from apalis into a human-readable string.
///
/// - `{"Ok":"Done"}` or `{"Ok":{"Next":...}}` -> `None` (success, hide)
/// - `{"Err":"msg"}` -> `Some("msg")` (show just the error message)
/// - Unparseable -> return raw as-is
fn sanitize_result(raw: &str) -> Option<String> {
    let val: serde_json::Value = match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(_) => return Some(raw.to_owned()),
    };

    if val.get("Ok").is_some() {
        return None;
    }

    if let Some(err) = val.get("Err") {
        return Some(err.as_str().map_or_else(|| err.to_string(), str::to_owned));
    }

    Some(raw.to_owned())
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
                let pipeline_step = metadata
                    .as_deref()
                    .and_then(|m| extract_pipeline_step(&job_type, m));
                let last_result = last_result.as_deref().and_then(sanitize_result);
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

/// Return queue depth per job type for the Immich-style queue display.
///
/// # Errors
///
/// Returns `AppError` on database failure.
async fn queue_counts(State(state): State<AppState>) -> Result<Json<Vec<QueueCount>>, AppError> {
    let rows = sqlx::query_as::<_, (String, String, i64)>(
        "SELECT job_type, status, COUNT(*) as count FROM Jobs GROUP BY job_type, status",
    )
    .fetch_all(&state.pool)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_result_hides_ok_done() {
        assert_eq!(sanitize_result(r#"{"Ok":"Done"}"#), None);
    }

    #[test]
    fn sanitize_result_hides_ok_next() {
        assert_eq!(sanitize_result(r#"{"Ok":{"Next":"abc"}}"#), None);
    }

    #[test]
    fn sanitize_result_extracts_error_message() {
        assert_eq!(
            sanitize_result(r#"{"Err":"connection timeout"}"#),
            Some("connection timeout".to_owned()),
        );
    }

    #[test]
    fn sanitize_result_passes_through_unparseable() {
        assert_eq!(sanitize_result("not json"), Some("not json".to_owned()),);
    }

    #[test]
    fn extract_pipeline_step_returns_step_index() {
        let metadata =
            r#"{"apalis_workflow::sequential::context::WorkflowContext":{"step_index":2}}"#;
        assert_eq!(extract_pipeline_step("Vec<u8>", metadata), Some(2));
    }

    #[test]
    fn extract_pipeline_step_defaults_to_zero() {
        // Pipeline job but no WorkflowContext in metadata
        assert_eq!(extract_pipeline_step("Vec<u8>", "{}"), Some(0));
    }

    #[test]
    fn extract_pipeline_step_none_for_non_pipeline() {
        let metadata =
            r#"{"apalis_workflow::sequential::context::WorkflowContext":{"step_index":1}}"#;
        assert_eq!(extract_pipeline_step("SyncJob", metadata), None);
    }
}
