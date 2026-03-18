//! `PayPal` enrichment jobs: transaction sync and matching.
//!
//! The sync job fetches transactions from the `PayPal` Transaction Search API,
//! automatically chunking into 31-day windows. After upserting all transactions,
//! it enqueues a [`PayPalMatchJob`] to match them against bank transactions.

use apalis::prelude::*;
use chrono::{Duration, Utc};
use tracing::{error, info};

use budget_db::Db;

use super::pipeline::parse_run_id;
use super::schedule_queries::{self, RunStatus};
use super::{ApalisPool, PayPalMatchJob};

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

    // Determine sync window: from latest known transaction date, or 90 days back
    let since = db
        .get_paypal_latest_transaction_date(account_id)
        .await?
        .map_or_else(
            || Utc::now() - Duration::days(90),
            |d| {
                d.and_hms_opt(0, 0, 0).unwrap_or_default().and_utc() - Duration::days(3) // overlap to catch late-arriving transactions
            },
        );

    info!(%account_id, since = %since.format("%Y-%m-%d"), "fetching PayPal transactions");

    let transactions = client.fetch_all_transactions(since).await.map_err(|e| {
        error!(%account_id, "PayPal fetch failed: {e}");
        e
    })?;

    let known_keys = db.get_paypal_dedup_keys(account_id).await?;
    let mut new_count = 0u32;

    for txn in &transactions {
        if known_keys.contains(&txn.transaction_id) {
            continue;
        }
        db.upsert_paypal_transaction(account_id, txn).await?;
        new_count += 1;
    }

    info!(
        %account_id,
        fetched = transactions.len(),
        new = new_count,
        "PayPal sync complete, enqueuing match job"
    );

    // Enqueue match job
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
