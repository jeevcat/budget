//! `PayPal` enrichment jobs: transaction sync and matching.
//!
//! The sync job fetches transactions from the `PayPal` Transaction Search API,
//! automatically chunking into 31-day windows. After upserting all transactions,
//! it enqueues a [`PayPalMatchJob`] to match them against bank transactions.
//!
//! Sync strategy mirrors bank sync: always try to reach the full 3-year API
//! horizon. On first run this means fetching the entire history. On subsequent
//! runs only the forward window (latest − 3 days → now) contains new data, so
//! the API calls are cheap. If a previous sync was interrupted before reaching
//! the horizon, the backfill continues where it left off.

use apalis::prelude::*;
use chrono::{Duration, Utc};
use tracing::{error, info};

use budget_db::Db;

use super::pipeline::parse_run_id;
use super::schedule_queries::{self, RunStatus};
use super::{ApalisPool, PayPalMatchJob};

/// `PayPal` API maximum history: 3 years.
const MAX_HISTORY_DAYS: i64 = 3 * 365;

/// Overlap window for forward sync to catch late-arriving transactions.
const OVERLAP_DAYS: i64 = 3;

/// Handle a [`super::PayPalSyncJob`]: fetch transactions from `PayPal` and upsert them.
///
/// # Errors
///
/// Returns an error if credentials are missing, the API call fails, or DB operations fail.
pub async fn handle_paypal_sync_job(
    job: super::PayPalSyncJob,
    db: Data<Db>,
    pool: Data<ApalisPool>,
) -> Result<(), BoxDynError> {
    let account_id = job.account_id;
    info!(%account_id, "starting PayPal sync job");

    let credentials = db.get_paypal_credentials(account_id).await?;
    let Some((client_id, client_secret, sandbox)) = credentials else {
        error!(%account_id, "PayPal account not found");
        return Err("PayPal account not found".into());
    };

    let mut client = if sandbox {
        budget_paypal::PayPalClient::sandbox(client_id, client_secret)
    } else {
        budget_paypal::PayPalClient::new(client_id, client_secret)
    }
    .map_err(|e| {
        error!(%account_id, "failed to create PayPal client: {e}");
        e
    })?;

    // Sync strategy:
    //   1. Forward: latest stored date (−3d overlap) → now.
    //   2. Backfill: 3-year horizon → earliest stored date (+3d overlap).
    //
    // On the very first sync both collapse into one range: horizon → now.
    // On subsequent syncs the backfill range is empty (earliest ≤ horizon)
    // so only the forward window runs. If a previous first sync was
    // interrupted, the backfill picks up from where the earliest stored
    // date is.
    //
    // Dedup is handled by the DB unique constraint on
    // (paypal_account_id, paypal_transaction_id).
    let now = Utc::now();
    let history_horizon = now - Duration::days(MAX_HISTORY_DAYS);

    let (earliest, latest) = db.get_paypal_date_range(account_id).await?;

    let known_keys = db.get_paypal_dedup_keys(account_id).await?;
    let mut new_count = 0u32;
    let mut total_fetched = 0usize;

    // -- Forward sync: latest → now ------------------------------------------
    let forward_since = latest.map_or(history_horizon, |d| {
        d.and_hms_opt(0, 0, 0).unwrap_or_default().and_utc() - Duration::days(OVERLAP_DAYS)
    });

    info!(%account_id, since = %forward_since.format("%Y-%m-%d"), "forward sync");
    let forward_txns = client
        .fetch_all_transactions(forward_since)
        .await
        .map_err(|e| {
            error!(%account_id, "PayPal forward fetch failed: {e}");
            e
        })?;
    total_fetched += forward_txns.len();

    for txn in &forward_txns {
        if !known_keys.contains(&txn.transaction_id) {
            db.upsert_paypal_transaction(account_id, txn).await?;
            new_count += 1;
        }
    }

    // -- Backfill: horizon → earliest ----------------------------------------
    if let Some(earliest_date) = earliest {
        let earliest_dt = earliest_date
            .and_hms_opt(0, 0, 0)
            .unwrap_or_default()
            .and_utc();

        if earliest_dt > history_horizon + Duration::days(OVERLAP_DAYS) {
            let backfill_end = earliest_dt + Duration::days(OVERLAP_DAYS);
            info!(
                %account_id,
                from = %history_horizon.format("%Y-%m-%d"),
                to = %backfill_end.format("%Y-%m-%d"),
                "backfilling older PayPal history"
            );

            let backfill_txns = client
                .fetch_all_transactions_until(history_horizon, backfill_end)
                .await
                .map_err(|e| {
                    error!(%account_id, "PayPal backfill failed: {e}");
                    e
                })?;
            total_fetched += backfill_txns.len();

            for txn in &backfill_txns {
                if !known_keys.contains(&txn.transaction_id) {
                    db.upsert_paypal_transaction(account_id, txn).await?;
                    new_count += 1;
                }
            }
        }
    }

    info!(
        %account_id,
        fetched = total_fetched,
        new = new_count,
        "PayPal sync complete, enqueuing match job"
    );

    let mut storage = apalis_postgres::PostgresStorage::<PayPalMatchJob>::new(&pool);
    storage
        .push(PayPalMatchJob {
            account_id,
            schedule_run_id: job.schedule_run_id,
        })
        .await
        .map_err(|e| format!("failed to enqueue PayPal match job: {e}"))?;

    Ok(())
}

/// Handle a [`PayPalMatchJob`]: match `PayPal` transactions to bank transactions.
///
/// # Errors
///
/// Returns an error if DB operations fail.
pub async fn handle_paypal_match_job(
    job: PayPalMatchJob,
    db: Data<Db>,
    pool: Data<ApalisPool>,
) -> Result<(), BoxDynError> {
    let run_uuid = parse_run_id(job.schedule_run_id.as_deref());

    let result = paypal_match_inner(&job, &db).await;

    if let Some(run_id) = run_uuid {
        match &result {
            Ok(()) => {
                let _ = schedule_queries::complete_schedule_run(
                    &pool,
                    run_id,
                    RunStatus::Succeeded,
                    None,
                )
                .await;
            }
            Err(e) => {
                let _ = schedule_queries::complete_schedule_run(
                    &pool,
                    run_id,
                    RunStatus::Failed,
                    Some(&e.to_string()),
                )
                .await;
            }
        }
    }

    result
}

async fn paypal_match_inner(job: &PayPalMatchJob, db: &Db) -> Result<(), BoxDynError> {
    let unmatched = db.get_unmatched_paypal_transactions(job.account_id).await?;
    let bank_candidates = db.get_paypal_matchable_bank_transactions().await?;

    let matches = budget_paypal::find_matches(&unmatched, &bank_candidates);
    let count = matches.len();

    if matches.is_empty() {
        info!(account_id = %job.account_id, "no new PayPal matches found");
    } else {
        db.insert_paypal_matches(&matches).await?;
        info!(account_id = %job.account_id, count, "inserted PayPal matches");
    }

    Ok(())
}
