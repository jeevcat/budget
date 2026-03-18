//! Automatic hourly sync scheduler.
//!
//! Runs a 30-second tick loop that checks each account and enqueues pipelines
//! when the sync interval has elapsed, or retries after transient failures.

use std::time::Duration;

use chrono::{DateTime, Utc};

use budget_db::Db;

use super::pipeline::PipelineContext;
use super::schedule_queries::{self, AccountType, RunStatus, ScheduleRun, TriggerReason};
use super::storage::{JobStorage, PipelineStorage};
use super::{AmazonSyncJob, ApalisPool, PayPalSyncJob};

/// Sync interval: 1 hour between successful runs.
const SYNC_INTERVAL_SECS: i64 = 3600;

/// Maximum retry attempts before falling back to the next hourly window.
const MAX_RETRIES: i32 = 5;

/// Backoff cap in seconds (attempt 5+).
const MAX_BACKOFF_SECS: i64 = 900; // 15 minutes

/// Run the scheduler loop forever, ticking every 30 seconds.
pub async fn run_scheduler(db: &Db, pool: &ApalisPool) {
    let pipeline_storage = PipelineStorage::new(pool);
    let amazon_storage = JobStorage::<AmazonSyncJob>::new(pool);
    let paypal_storage = JobStorage::<PayPalSyncJob>::new(pool);
    let mut interval = tokio::time::interval(Duration::from_secs(30));
    loop {
        interval.tick().await;
        if let Err(e) = scheduler_tick(
            db,
            pool,
            &pipeline_storage,
            &amazon_storage,
            &paypal_storage,
            Utc::now(),
        )
        .await
        {
            tracing::warn!(error = %e, "scheduler tick failed");
        }
    }
}

/// Single scheduler tick: evaluate every account and enqueue pipelines as needed.
///
/// `now` is injectable for testability.
///
/// # Errors
///
/// Returns an error if the account list or schedule queries fail.
pub async fn scheduler_tick(
    db: &Db,
    pool: &ApalisPool,
    pipeline_storage: &PipelineStorage,
    amazon_storage: &JobStorage<AmazonSyncJob>,
    paypal_storage: &JobStorage<PayPalSyncJob>,
    now: DateTime<Utc>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Bank accounts
    let accounts = db.list_connected_accounts().await?;
    for account in &accounts {
        let account_uuid: uuid::Uuid = account.id.into();
        let latest =
            schedule_queries::get_latest_run_for_account(pool, account_uuid, AccountType::Bank)
                .await?;

        match evaluate_account(latest.as_ref(), now) {
            Action::Skip => {}
            Action::UpdateNextRun {
                run_id,
                next_run_at,
            } => {
                schedule_queries::update_next_run_at(pool, run_id, next_run_at).await?;
            }
            Action::Enqueue {
                reason,
                attempt,
                next_run_at,
            } => {
                enqueue_pipeline(
                    pool,
                    pipeline_storage,
                    account_uuid,
                    reason,
                    attempt,
                    next_run_at,
                    now,
                )
                .await?;
            }
        }
    }

    // Amazon accounts
    let amazon_accounts = db.list_amazon_accounts().await?;
    for account in &amazon_accounts {
        let account_uuid: uuid::Uuid = account.id.into();
        let latest =
            schedule_queries::get_latest_run_for_account(pool, account_uuid, AccountType::Amazon)
                .await?;

        match evaluate_account(latest.as_ref(), now) {
            Action::Skip => {}
            Action::UpdateNextRun {
                run_id,
                next_run_at,
            } => {
                schedule_queries::update_next_run_at(pool, run_id, next_run_at).await?;
            }
            Action::Enqueue {
                reason,
                attempt,
                next_run_at,
            } => {
                enqueue_amazon_sync(
                    pool,
                    amazon_storage,
                    account_uuid,
                    reason,
                    attempt,
                    next_run_at,
                    now,
                )
                .await?;
            }
        }
    }

    // PayPal accounts
    let paypal_accounts = db.list_paypal_accounts().await?;
    for account in &paypal_accounts {
        let account_uuid: uuid::Uuid = account.id.into();
        let latest =
            schedule_queries::get_latest_run_for_account(pool, account_uuid, AccountType::PayPal)
                .await?;

        match evaluate_account(latest.as_ref(), now) {
            Action::Skip => {}
            Action::UpdateNextRun {
                run_id,
                next_run_at,
            } => {
                schedule_queries::update_next_run_at(pool, run_id, next_run_at).await?;
            }
            Action::Enqueue {
                reason,
                attempt,
                next_run_at,
            } => {
                enqueue_paypal_sync(
                    pool,
                    paypal_storage,
                    account_uuid,
                    reason,
                    attempt,
                    next_run_at,
                    now,
                )
                .await?;
            }
        }
    }

    Ok(())
}

/// What the scheduler should do for a single account this tick.
enum Action {
    /// Nothing to do — either a run is in progress or it's too early.
    Skip,
    /// Update the `next_run_at` on an existing run (for UI display only).
    UpdateNextRun {
        run_id: uuid::Uuid,
        next_run_at: DateTime<Utc>,
    },
    /// Enqueue a new pipeline run.
    Enqueue {
        reason: TriggerReason,
        attempt: i32,
        next_run_at: Option<DateTime<Utc>>,
    },
}

/// Decide what to do for an account based on its latest schedule run.
fn evaluate_account(latest: Option<&ScheduleRun>, now: DateTime<Utc>) -> Action {
    let Some(run) = latest else {
        // No prior run — schedule immediately
        return Action::Enqueue {
            reason: TriggerReason::Scheduled,
            attempt: 1,
            next_run_at: None,
        };
    };

    match run.status {
        RunStatus::Running | RunStatus::Pending => Action::Skip,

        RunStatus::Succeeded => {
            let finished = run.finished_at.unwrap_or(run.created_at);
            let elapsed = (now - finished).num_seconds();
            if elapsed >= SYNC_INTERVAL_SECS {
                Action::Enqueue {
                    reason: TriggerReason::Scheduled,
                    attempt: 1,
                    next_run_at: None,
                }
            } else {
                let next = finished + chrono::Duration::seconds(SYNC_INTERVAL_SECS);
                if run.next_run_at.is_none() {
                    Action::UpdateNextRun {
                        run_id: run.id,
                        next_run_at: next,
                    }
                } else {
                    Action::Skip
                }
            }
        }

        RunStatus::Failed => {
            let finished = run.finished_at.unwrap_or(run.created_at);
            let error = run.error_message.as_deref().unwrap_or("");

            // Non-transient errors: wait for next hourly window
            if !is_transient(error) {
                let next = finished + chrono::Duration::seconds(SYNC_INTERVAL_SECS);
                if run.next_run_at.is_none() {
                    return Action::UpdateNextRun {
                        run_id: run.id,
                        next_run_at: next,
                    };
                }
                let elapsed = (now - finished).num_seconds();
                if elapsed >= SYNC_INTERVAL_SECS {
                    return Action::Enqueue {
                        reason: TriggerReason::Scheduled,
                        attempt: 1,
                        next_run_at: None,
                    };
                }
                return Action::Skip;
            }

            // Max retries exhausted: wait for next hourly window
            if run.attempt >= MAX_RETRIES {
                let next = finished + chrono::Duration::seconds(SYNC_INTERVAL_SECS);
                if run.next_run_at.is_none() {
                    return Action::UpdateNextRun {
                        run_id: run.id,
                        next_run_at: next,
                    };
                }
                let elapsed = (now - finished).num_seconds();
                if elapsed >= SYNC_INTERVAL_SECS {
                    return Action::Enqueue {
                        reason: TriggerReason::Scheduled,
                        attempt: 1,
                        next_run_at: None,
                    };
                }
                return Action::Skip;
            }

            // Transient error with retries remaining: check backoff
            let backoff = backoff_seconds(run.attempt);
            let elapsed = (now - finished).num_seconds();
            if elapsed >= backoff {
                Action::Enqueue {
                    reason: TriggerReason::Retry,
                    attempt: run.attempt + 1,
                    next_run_at: None,
                }
            } else {
                let next = finished + chrono::Duration::seconds(backoff);
                if run.next_run_at.is_none() {
                    Action::UpdateNextRun {
                        run_id: run.id,
                        next_run_at: next,
                    }
                } else {
                    Action::Skip
                }
            }
        }
    }
}

/// Insert a `schedule_runs` row and push the pipeline.
async fn enqueue_pipeline(
    pool: &ApalisPool,
    pipeline_storage: &PipelineStorage,
    account_id: uuid::Uuid,
    reason: TriggerReason,
    attempt: i32,
    next_run_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Result<(), Box<dyn std::error::Error>> {
    let run_id = uuid::Uuid::new_v4();

    let run = ScheduleRun {
        id: run_id,
        account_id,
        account_type: AccountType::Bank,
        status: RunStatus::Running,
        trigger_reason: reason,
        attempt,
        started_at: Some(now),
        finished_at: None,
        next_run_at,
        error_message: None,
        created_at: now,
    };

    schedule_queries::insert_schedule_run(pool, &run).await?;

    let ctx = PipelineContext {
        account_id: budget_core::models::AccountId::from_uuid(account_id),
        schedule_run_id: Some(run_id.to_string()),
        full_resync: false,
    };

    if let Err(e) = pipeline_storage.push_start(ctx).await {
        // Pipeline enqueue failed — mark the run as failed immediately
        let _ = schedule_queries::complete_schedule_run(pool, run_id, RunStatus::Failed, Some(&e))
            .await;
        return Err(e.into());
    }

    tracing::info!(
        %account_id,
        %run_id,
        reason = %reason,
        attempt,
        "scheduled pipeline"
    );

    Ok(())
}

/// Insert a `schedule_runs` row and push an Amazon sync job.
async fn enqueue_amazon_sync(
    pool: &ApalisPool,
    amazon_storage: &JobStorage<AmazonSyncJob>,
    account_id: uuid::Uuid,
    reason: TriggerReason,
    attempt: i32,
    next_run_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Result<(), Box<dyn std::error::Error>> {
    let run_id = uuid::Uuid::new_v4();

    let run = ScheduleRun {
        id: run_id,
        account_id,
        account_type: AccountType::Amazon,
        status: RunStatus::Running,
        trigger_reason: reason,
        attempt,
        started_at: Some(now),
        finished_at: None,
        next_run_at,
        error_message: None,
        created_at: now,
    };

    schedule_queries::insert_schedule_run(pool, &run).await?;

    if let Err(e) = amazon_storage
        .push(AmazonSyncJob {
            account_id: budget_core::models::AmazonAccountId::from_uuid(account_id),
            schedule_run_id: Some(run_id.to_string()),
        })
        .await
    {
        let _ = schedule_queries::complete_schedule_run(pool, run_id, RunStatus::Failed, Some(&e))
            .await;
        return Err(e.into());
    }

    tracing::info!(
        %account_id,
        %run_id,
        reason = %reason,
        attempt,
        "scheduled Amazon sync"
    );

    Ok(())
}

/// Insert a `schedule_runs` row and push a `PayPal` sync job.
async fn enqueue_paypal_sync(
    pool: &ApalisPool,
    paypal_storage: &JobStorage<PayPalSyncJob>,
    account_id: uuid::Uuid,
    reason: TriggerReason,
    attempt: i32,
    next_run_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Result<(), Box<dyn std::error::Error>> {
    let run_id = uuid::Uuid::new_v4();

    let run = ScheduleRun {
        id: run_id,
        account_id,
        account_type: AccountType::PayPal,
        status: RunStatus::Running,
        trigger_reason: reason,
        attempt,
        started_at: Some(now),
        finished_at: None,
        next_run_at,
        error_message: None,
        created_at: now,
    };

    schedule_queries::insert_schedule_run(pool, &run).await?;

    if let Err(e) = paypal_storage
        .push(PayPalSyncJob {
            account_id: budget_core::models::PayPalAccountId::from_uuid(account_id),
            schedule_run_id: Some(run_id.to_string()),
        })
        .await
    {
        let _ = schedule_queries::complete_schedule_run(pool, run_id, RunStatus::Failed, Some(&e))
            .await;
        return Err(e.into());
    }

    tracing::info!(
        %account_id,
        %run_id,
        reason = %reason,
        attempt,
        "scheduled PayPal sync"
    );

    Ok(())
}

/// Exponential backoff: 60s * 2^(attempt-1), capped at 15 minutes.
fn backoff_seconds(attempt: i32) -> i64 {
    let base = 60i64 * (1i64 << (attempt - 1).min(10));
    base.min(MAX_BACKOFF_SECS)
}

/// Classify whether an error message indicates a transient failure worth retrying.
fn is_transient(error: &str) -> bool {
    let lower = error.to_lowercase();
    [
        "rate limited",
        "connection failed",
        "timeout",
        "connection reset",
        "temporarily unavailable",
    ]
    .iter()
    .any(|p| lower.contains(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_progression() {
        assert_eq!(backoff_seconds(1), 60);
        assert_eq!(backoff_seconds(2), 120);
        assert_eq!(backoff_seconds(3), 240);
        assert_eq!(backoff_seconds(4), 480);
        assert_eq!(backoff_seconds(5), 900); // capped
        assert_eq!(backoff_seconds(6), 900); // still capped
    }

    #[test]
    fn transient_detection() {
        assert!(is_transient("Connection failed: timeout"));
        assert!(is_transient("RATE LIMITED by provider"));
        assert!(is_transient("connection reset by peer"));
        assert!(is_transient("service temporarily unavailable"));
        assert!(!is_transient("account not found"));
        assert!(!is_transient("invalid credentials"));
    }

    #[test]
    fn no_prior_run_enqueues() {
        let now = Utc::now();
        match evaluate_account(None, now) {
            Action::Enqueue {
                reason, attempt, ..
            } => {
                assert_eq!(reason, TriggerReason::Scheduled);
                assert_eq!(attempt, 1);
            }
            _ => panic!("expected Enqueue"),
        }
    }

    #[test]
    fn running_run_is_skipped() {
        let now = Utc::now();
        let run = ScheduleRun {
            id: uuid::Uuid::new_v4(),
            account_id: uuid::Uuid::new_v4(),
            account_type: AccountType::Bank,
            status: RunStatus::Running,
            trigger_reason: TriggerReason::Scheduled,
            attempt: 1,
            started_at: Some(now),
            finished_at: None,
            next_run_at: None,
            error_message: None,
            created_at: now,
        };
        assert!(matches!(evaluate_account(Some(&run), now), Action::Skip));
    }
}
