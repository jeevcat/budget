//! Queries against the apalis jobs table.
//!
//! These encapsulate the backend-specific table name and column types
//! so callers don't need raw SQL.

use serde::Serialize;
use sqlx::Row;

use super::{ApalisPool, JOBS_TABLE};

/// A single job record from the apalis queue.
#[derive(Serialize)]
pub struct JobRecord {
    pub job_type: String,
    pub status: String,
    pub run_at: String,
}

/// Aggregate queue depth for one job type.
#[derive(Serialize)]
pub struct QueueCount {
    pub job_type: String,
    pub active: i64,
    pub waiting: i64,
    pub completed: i64,
    pub failed: i64,
}

/// List all jobs ordered by most recent first.
///
/// # Errors
///
/// Returns `sqlx::Error` on database failure.
pub async fn list_jobs(pool: &ApalisPool) -> Result<Vec<JobRecord>, sqlx::Error> {
    let rows = sqlx::query(&format!(
        "SELECT job_type, status, CAST(run_at AS TEXT) as run_at FROM {JOBS_TABLE} ORDER BY run_at DESC",
    ))
    .fetch_all(pool)
    .await?;

    rows.iter()
        .map(|row| {
            Ok(JobRecord {
                job_type: row.try_get("job_type")?,
                status: row.try_get("status")?,
                run_at: row.try_get("run_at")?,
            })
        })
        .collect()
}

/// Return queue depth per job type.
///
/// # Errors
///
/// Returns `sqlx::Error` on database failure.
pub async fn queue_counts(pool: &ApalisPool) -> Result<Vec<QueueCount>, sqlx::Error> {
    let rows = sqlx::query(&format!(
        "SELECT job_type, status, COUNT(*) as cnt FROM {JOBS_TABLE} GROUP BY job_type, status",
    ))
    .fetch_all(pool)
    .await?;

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
    Ok(counts)
}

/// Reset all running jobs to pending (startup recovery).
///
/// # Errors
///
/// Returns `sqlx::Error` on database failure.
pub async fn reset_all_running(pool: &ApalisPool) -> Result<u64, sqlx::Error> {
    let res = sqlx::query(&format!(
        "UPDATE {JOBS_TABLE} SET status = 'Pending', lock_by = NULL, lock_at = NULL WHERE status = 'Running'",
    ))
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}

/// Reclaim jobs stuck in `Running` with a lock older than `stale_seconds`.
///
/// # Errors
///
/// Returns `sqlx::Error` on database failure.
pub async fn reclaim_stale(pool: &ApalisPool, stale_seconds: i64) -> Result<u64, sqlx::Error> {
    let cutoff = chrono::Utc::now() - chrono::Duration::seconds(stale_seconds);
    let res = sqlx::query(&format!(
        "UPDATE {JOBS_TABLE} SET status = 'Pending', lock_by = NULL, lock_at = NULL \
         WHERE status = 'Running' AND lock_at < $1",
    ))
    .bind(cutoff)
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}
