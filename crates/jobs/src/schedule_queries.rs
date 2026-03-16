use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::Row;
use uuid::Uuid;

use super::ApalisPool;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Whether a schedule run tracks a bank or Amazon account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "openapi", schema(as = JobAccountType))]
pub enum AccountType {
    Bank,
    Amazon,
}

impl fmt::Display for AccountType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bank => write!(f, "bank"),
            Self::Amazon => write!(f, "amazon"),
        }
    }
}

impl FromStr for AccountType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "bank" => Ok(Self::Bank),
            "amazon" => Ok(Self::Amazon),
            other => Err(format!("unknown AccountType: {other}")),
        }
    }
}

/// Status of a schedule run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum RunStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
}

impl fmt::Display for RunStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Running => write!(f, "running"),
            Self::Succeeded => write!(f, "succeeded"),
            Self::Failed => write!(f, "failed"),
        }
    }
}

impl FromStr for RunStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            other => Err(format!("unknown RunStatus: {other}")),
        }
    }
}

/// Reason a schedule run was triggered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum TriggerReason {
    Scheduled,
    Retry,
    Manual,
}

impl fmt::Display for TriggerReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Scheduled => write!(f, "scheduled"),
            Self::Retry => write!(f, "retry"),
            Self::Manual => write!(f, "manual"),
        }
    }
}

impl FromStr for TriggerReason {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "scheduled" => Ok(Self::Scheduled),
            "retry" => Ok(Self::Retry),
            "manual" => Ok(Self::Manual),
            other => Err(format!("unknown TriggerReason: {other}")),
        }
    }
}

/// A single row from `schedule_runs`.
#[derive(Debug, Clone)]
pub struct ScheduleRun {
    pub id: Uuid,
    pub account_id: Uuid,
    pub account_type: AccountType,
    pub status: RunStatus,
    pub trigger_reason: TriggerReason,
    pub attempt: i32,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub next_run_at: Option<DateTime<Utc>>,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Per-account schedule summary for the API response.
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AccountScheduleStatus {
    pub account_id: Uuid,
    pub account_name: String,
    pub account_type: AccountType,
    /// Whether this account has a bank connection and can be API-synced.
    pub syncable: bool,
    pub last_run_at: Option<DateTime<Utc>>,
    pub last_run_status: Option<RunStatus>,
    pub last_error: Option<String>,
    pub next_run_at: Option<DateTime<Utc>>,
    pub next_run_reason: Option<TriggerReason>,
}

// ---------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------

/// Insert a new schedule run.
///
/// # Errors
///
/// Returns `sqlx::Error` on database failure.
pub async fn insert_schedule_run(pool: &ApalisPool, run: &ScheduleRun) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO schedule_runs (id, account_id, account_type, status, trigger_reason, attempt, started_at, finished_at, next_run_at, error_message, created_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
    )
    .bind(run.id)
    .bind(run.account_id)
    .bind(run.account_type.to_string())
    .bind(run.status.to_string())
    .bind(run.trigger_reason.to_string())
    .bind(run.attempt)
    .bind(run.started_at)
    .bind(run.finished_at)
    .bind(run.next_run_at)
    .bind(run.error_message.as_deref())
    .bind(run.created_at)
    .execute(pool)
    .await?;
    Ok(())
}

/// Mark a schedule run as completed (succeeded or failed).
///
/// # Errors
///
/// Returns `sqlx::Error` on database failure.
pub async fn complete_schedule_run(
    pool: &ApalisPool,
    run_id: Uuid,
    status: RunStatus,
    error_msg: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE schedule_runs SET status = $1, finished_at = $2, error_message = $3 WHERE id = $4",
    )
    .bind(status.to_string())
    .bind(Utc::now())
    .bind(error_msg)
    .bind(run_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Get the latest schedule run for an account (by `created_at` descending).
///
/// # Errors
///
/// Returns `sqlx::Error` on database failure.
pub async fn get_latest_run_for_account(
    pool: &ApalisPool,
    account_id: Uuid,
    account_type: AccountType,
) -> Result<Option<ScheduleRun>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, account_id, account_type, status, trigger_reason, attempt, started_at, finished_at, \
                next_run_at, error_message, created_at \
         FROM schedule_runs WHERE account_id = $1 AND account_type = $2 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(account_id)
    .bind(account_type.to_string())
    .fetch_optional(pool)
    .await?;

    row.map(|r| parse_schedule_run(&r)).transpose()
}

/// Update the `next_run_at` field for a schedule run.
///
/// # Errors
///
/// Returns `sqlx::Error` on database failure.
pub async fn update_next_run_at(
    pool: &ApalisPool,
    run_id: Uuid,
    next_run_at: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE schedule_runs SET next_run_at = $1 WHERE id = $2")
        .bind(next_run_at)
        .bind(run_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Return schedule status for every account (latest run per account).
///
/// # Errors
///
/// Returns `sqlx::Error` on database failure.
pub async fn get_all_schedule_status(
    pool: &ApalisPool,
) -> Result<Vec<AccountScheduleStatus>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT account_id, account_name, account_type, last_run_at, last_run_status, \
                last_error, next_run_at, trigger_reason \
         FROM ( \
             SELECT a.id AS account_id, COALESCE(a.nickname, a.name) AS account_name, \
                    'bank' AS account_type, \
                    sr.started_at AS last_run_at, sr.status AS last_run_status, \
                    sr.error_message AS last_error, sr.next_run_at, sr.trigger_reason \
             FROM accounts a \
             JOIN connections c ON c.id = a.connection_id \
             LEFT JOIN LATERAL ( \
                 SELECT started_at, status, error_message, next_run_at, trigger_reason \
                 FROM schedule_runs \
                 WHERE account_id = a.id AND account_type = 'bank' \
                 ORDER BY created_at DESC LIMIT 1 \
             ) sr ON true \
             UNION ALL \
             SELECT aa.id AS account_id, aa.label AS account_name, \
                    'amazon' AS account_type, \
                    sr.started_at AS last_run_at, sr.status AS last_run_status, \
                    sr.error_message AS last_error, sr.next_run_at, sr.trigger_reason \
             FROM amazon_accounts aa \
             LEFT JOIN LATERAL ( \
                 SELECT started_at, status, error_message, next_run_at, trigger_reason \
                 FROM schedule_runs \
                 WHERE account_id = aa.id AND account_type = 'amazon' \
                 ORDER BY created_at DESC LIMIT 1 \
             ) sr ON true \
         ) combined \
         ORDER BY account_name",
    )
    .fetch_all(pool)
    .await?;

    rows.iter()
        .map(|r| {
            let status_str: Option<String> = r.try_get("last_run_status")?;
            let reason_str: Option<String> = r.try_get("trigger_reason")?;
            let type_str: String = r.try_get("account_type")?;
            let account_type =
                AccountType::from_str(&type_str).map_err(|e| sqlx::Error::Decode(e.into()))?;
            Ok(AccountScheduleStatus {
                account_id: r.try_get("account_id")?,
                account_name: r.try_get("account_name")?,
                account_type,
                syncable: true,
                last_run_at: r.try_get("last_run_at")?,
                last_run_status: status_str
                    .as_deref()
                    .map(RunStatus::from_str)
                    .transpose()
                    .map_err(|e| sqlx::Error::Decode(e.into()))?,
                last_error: r.try_get("last_error")?,
                next_run_at: r.try_get("next_run_at")?,
                next_run_reason: reason_str
                    .as_deref()
                    .map(TriggerReason::from_str)
                    .transpose()
                    .map_err(|e| sqlx::Error::Decode(e.into()))?,
            })
        })
        .collect()
}

/// Parse a `schedule_runs` row into a `ScheduleRun`.
fn parse_schedule_run(row: &sqlx::postgres::PgRow) -> Result<ScheduleRun, sqlx::Error> {
    let status_str: String = row.try_get("status")?;
    let reason_str: String = row.try_get("trigger_reason")?;
    let type_str: String = row.try_get("account_type")?;
    Ok(ScheduleRun {
        id: row.try_get("id")?,
        account_id: row.try_get("account_id")?,
        account_type: AccountType::from_str(&type_str)
            .map_err(|e| sqlx::Error::Decode(e.into()))?,
        status: RunStatus::from_str(&status_str).map_err(|e| sqlx::Error::Decode(e.into()))?,
        trigger_reason: TriggerReason::from_str(&reason_str)
            .map_err(|e| sqlx::Error::Decode(e.into()))?,
        attempt: row.try_get("attempt")?,
        started_at: row.try_get("started_at")?,
        finished_at: row.try_get("finished_at")?,
        next_run_at: row.try_get("next_run_at")?,
        error_message: row.try_get("error_message")?,
        created_at: row.try_get("created_at")?,
    })
}
