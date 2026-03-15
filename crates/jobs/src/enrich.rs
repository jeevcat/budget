//! Amazon enrichment jobs: transaction sync, order detail fetching, and matching.
//!
//! The sync job paginates Amazon transactions one page at a time. For each page
//! it upserts transactions to the DB, fetches invoices inline for any new orders,
//! then enqueues a job for the next page (if any). The final page enqueues an
//! [`AmazonMatchJob`]. Invoice fetches happen inline because Amazon's session
//! expires within ~10 minutes — too short for separately queued jobs.
//!
//! Job types:
//! - [`AmazonSyncJob`]: fetches the first transaction page, processes it, enqueues next page
//! - [`AmazonPageJob`]: fetches one subsequent transaction page, processes it, enqueues next page
//! - [`AmazonFetchOrderJob`]: fallback single-order fetch (kept for retries / manual use)
//! - [`AmazonMatchJob`]: matches Amazon transactions to bank transactions

use std::path::PathBuf;
use std::time::Duration;

use apalis::prelude::*;
use tracing::{debug, error, info, warn};

use budget_amazon::{AmazonClient, CookieStore};
use budget_db::Db;

use super::pipeline::parse_run_id;
use super::queries;
use super::schedule_queries::{self, RunStatus};
use super::{AmazonFetchOrderJob, AmazonMatchJob, AmazonPageJob, ApalisPool};

/// Configuration for the Amazon enrichment client.
#[derive(Clone)]
pub struct AmazonEnrichConfig {
    pub cookies_dir: PathBuf,
}

/// Handle an [`AmazonSyncJob`]: fetch the first page of transactions, upsert + fetch
/// invoices inline, then enqueue the next page or a match job.
///
/// # Errors
///
/// Returns an error if cookies are expired, network requests fail, or DB operations fail.
pub async fn handle_amazon_sync_job(
    job: super::AmazonSyncJob,
    db: Data<Db>,
    config: Data<AmazonEnrichConfig>,
    pool: Data<ApalisPool>,
) -> Result<(), BoxDynError> {
    let account_id = job.account_id;
    info!(%account_id, "starting Amazon sync job");

    let cookies_path = config.cookies_dir.join(format!("{account_id}.json"));
    let cookies = CookieStore::load(&cookies_path).map_err(|e| {
        error!(path = %cookies_path.display(), %account_id, "failed to load Amazon cookies: {e}");
        e
    })?;

    if cookies.is_expired() {
        warn!(%account_id, "Amazon cookies are expired — re-login required");
        if let Some(run_uuid) = parse_run_id(job.schedule_run_id.as_deref()) {
            let _ = schedule_queries::complete_schedule_run(
                &pool,
                run_uuid,
                RunStatus::Failed,
                Some("Amazon cookies expired"),
            )
            .await;
        }
        return Err("Amazon cookies expired".into());
    }

    let mut client = AmazonClient::new(cookies).map_err(|e| {
        error!(%account_id, "failed to create Amazon client: {e}");
        e
    })?;

    let known_keys = db.get_amazon_dedup_keys(account_id).await?;
    debug!(%account_id, known = known_keys.len(), "loaded known Amazon dedup keys");

    let first_page = client.fetch_transactions_page().await.map_err(|e| {
        error!(%account_id, "failed to fetch Amazon transactions page: {e}");
        e
    })?;

    let (new_txns, stopped) = filter_new_transactions(first_page.transactions, &known_keys);
    info!(%account_id, count = new_txns.len(), stopped, "first page");

    // Upsert transactions and fetch invoices inline
    for txn in &new_txns {
        db.upsert_amazon_transaction(account_id, txn).await?;
    }
    let invoices_failed =
        fetch_invoices_for_transactions(&client, &new_txns, &db, account_id).await;

    // Decide what to enqueue next
    if stopped || !first_page.has_more {
        enqueue_match_job(&pool, account_id, job.schedule_run_id).await?;
    } else if let Some(page_key) = first_page.page_key {
        let mut page_storage = apalis_postgres::PostgresStorage::<AmazonPageJob>::new(&pool);
        page_storage
            .push(AmazonPageJob {
                account_id,
                page_key,
                page_num: 2,
                schedule_run_id: job.schedule_run_id,
            })
            .await
            .map_err(|e| format!("failed to enqueue page job: {e}"))?;
    } else {
        enqueue_match_job(&pool, account_id, job.schedule_run_id).await?;
    }

    if invoices_failed > 0 {
        warn!(%account_id, invoices_failed, "some invoices failed during sync");
    }

    Ok(())
}

/// Handle an [`AmazonPageJob`]: fetch one subsequent page of transactions,
/// upsert + fetch invoices, then enqueue next page or match job.
///
/// Re-fetches the transactions page to get a fresh JWT and keep the session warm.
///
/// # Errors
///
/// Returns an error if cookies are expired, network requests fail, or DB operations fail.
pub async fn handle_amazon_page_job(
    job: AmazonPageJob,
    db: Data<Db>,
    config: Data<AmazonEnrichConfig>,
    pool: Data<ApalisPool>,
) -> Result<(), BoxDynError> {
    let account_id = job.account_id;
    info!(%account_id, page = job.page_num, "starting Amazon page job");

    let cookies_path = config.cookies_dir.join(format!("{account_id}.json"));
    let cookies = CookieStore::load(&cookies_path).map_err(|e| {
        error!(path = %cookies_path.display(), "failed to load Amazon cookies: {e}");
        e
    })?;

    let mut client = AmazonClient::new(cookies).map_err(|e| {
        error!(%account_id, "failed to create Amazon client: {e}");
        e
    })?;

    // Re-fetch the first page to get a fresh JWT (also keeps session warm)
    client.fetch_transactions_page().await.map_err(|e| {
        error!(%account_id, "failed to refresh JWT: {e}");
        e
    })?;

    let known_keys = db.get_amazon_dedup_keys(account_id).await?;

    let (txns, next_key) = client
        .fetch_transactions_next(&job.page_key)
        .await
        .map_err(|e| {
            error!(%account_id, page = job.page_num, "failed to fetch transactions page: {e}");
            e
        })?;

    if txns.is_empty() {
        info!(%account_id, page = job.page_num, "empty page, stopping");
        enqueue_match_job(&pool, account_id, job.schedule_run_id).await?;
        return Ok(());
    }

    let (new_txns, stopped) = filter_new_transactions(txns, &known_keys);
    info!(%account_id, page = job.page_num, count = new_txns.len(), stopped, "page fetched");

    for txn in &new_txns {
        db.upsert_amazon_transaction(account_id, txn).await?;
    }
    let invoices_failed =
        fetch_invoices_for_transactions(&client, &new_txns, &db, account_id).await;

    if stopped || next_key.is_none() {
        enqueue_match_job(&pool, account_id, job.schedule_run_id).await?;
    } else if let Some(key) = next_key {
        let mut page_storage = apalis_postgres::PostgresStorage::<AmazonPageJob>::new(&pool);
        page_storage
            .push(AmazonPageJob {
                account_id,
                page_key: key,
                page_num: job.page_num + 1,
                schedule_run_id: job.schedule_run_id,
            })
            .await
            .map_err(|e| format!("failed to enqueue page job: {e}"))?;
    }

    if invoices_failed > 0 {
        warn!(%account_id, page = job.page_num, invoices_failed, "some invoices failed");
    }

    Ok(())
}

/// Filter transactions to only new ones, stopping at the first known dedup key.
fn filter_new_transactions(
    txns: Vec<budget_amazon::AmazonTransaction>,
    known_keys: &std::collections::HashSet<String>,
) -> (Vec<budget_amazon::AmazonTransaction>, bool) {
    let mut new_txns = Vec::new();
    for txn in txns {
        if known_keys.contains(&txn.dedup_key) {
            info!(dedup_key = %txn.dedup_key, "reached known transaction, stopping");
            return (new_txns, true);
        }
        new_txns.push(txn);
    }
    (new_txns, false)
}

/// Fetch invoices inline for all order IDs referenced by the given transactions.
///
/// Returns the number of failed invoice fetches (non-fatal — we log and continue).
async fn fetch_invoices_for_transactions(
    client: &AmazonClient,
    txns: &[budget_amazon::AmazonTransaction],
    db: &Db,
    account_id: budget_core::models::AmazonAccountId,
) -> u32 {
    // Collect unique order IDs from this batch, skipping already-fetched ones
    let order_ids: Vec<String> = {
        let mut seen = std::collections::HashSet::new();
        txns.iter()
            .flat_map(|t| &t.order_ids)
            .filter(|id| seen.insert((*id).clone()))
            .cloned()
            .collect()
    };

    let unfetched: Vec<String> = match db.get_unfetched_order_ids(account_id).await {
        Ok(ids) => {
            let unfetched_set: std::collections::HashSet<&str> =
                ids.iter().map(String::as_str).collect();
            order_ids
                .into_iter()
                .filter(|id| unfetched_set.contains(id.as_str()))
                .collect()
        }
        Err(e) => {
            warn!(%account_id, "failed to check unfetched orders, fetching all: {e}");
            order_ids
        }
    };

    if unfetched.is_empty() {
        return 0;
    }

    info!(%account_id, count = unfetched.len(), "fetching invoices inline");
    let mut failed = 0u32;

    for (i, order_id) in unfetched.iter().enumerate() {
        if i > 0 {
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
        match client.fetch_order_details(order_id).await {
            Ok(order) => {
                if let Err(e) = db.upsert_amazon_order(&order).await {
                    warn!(order_id = %order_id, "failed to upsert order: {e}");
                    failed += 1;
                } else {
                    debug!(order_id = %order_id, items = order.items.len(), "fetched invoice");
                }
            }
            Err(budget_amazon::AmazonError::CookiesExpired) => {
                warn!(
                    order_id = %order_id,
                    remaining = unfetched.len() - i - 1,
                    "session expired — stopping invoice fetches"
                );
                failed += u32::try_from(unfetched.len() - i).unwrap_or(u32::MAX);
                break;
            }
            Err(e) => {
                warn!(order_id = %order_id, "failed to fetch invoice: {e}");
                failed += 1;
            }
        }
    }

    failed
}

/// Enqueue an [`AmazonMatchJob`].
async fn enqueue_match_job(
    pool: &ApalisPool,
    account_id: budget_core::models::AmazonAccountId,
    schedule_run_id: Option<String>,
) -> Result<(), BoxDynError> {
    let mut storage = apalis_postgres::PostgresStorage::<AmazonMatchJob>::new(pool);
    storage
        .push(AmazonMatchJob {
            account_id,
            schedule_run_id,
        })
        .await
        .map_err(|e| format!("failed to enqueue match job: {e}"))?;
    Ok(())
}

/// Handle an [`AmazonFetchOrderJob`]: fetch a single order's invoice page and upsert to DB.
///
/// Rate limiting is handled by the worker configuration (not in-handler sleeps).
///
/// # Errors
///
/// Returns an error if cookies are expired, the fetch fails, or DB operations fail.
pub async fn handle_amazon_fetch_order_job(
    job: AmazonFetchOrderJob,
    db: Data<Db>,
    config: Data<AmazonEnrichConfig>,
    pool: Data<ApalisPool>,
) -> Result<(), BoxDynError> {
    let cookies_path = config.cookies_dir.join(format!("{}.json", job.account_id));

    let cookies = CookieStore::load(&cookies_path).map_err(|e| {
        error!(path = %cookies_path.display(), "failed to load Amazon cookies: {e}");
        e
    })?;

    let client = AmazonClient::new(cookies).map_err(|e| {
        error!("failed to create Amazon client: {e}");
        e
    })?;

    match client.fetch_order_details(&job.order_id).await {
        Ok(order) => {
            db.upsert_amazon_order(&order).await?;
            debug!(
                order_id = %job.order_id,
                items = order.items.len(),
                "fetched order details"
            );
        }
        Err(budget_amazon::AmazonError::CookiesExpired) => {
            let account_str = job.account_id.to_string();
            let killed = queries::kill_pending_order_jobs(&pool, &account_str)
                .await
                .unwrap_or(0);
            warn!(
                order_id = %job.order_id,
                killed,
                "cookies expired — killed remaining order fetch jobs"
            );
            return Err(budget_amazon::AmazonError::CookiesExpired.into());
        }
        Err(e) => {
            warn!(order_id = %job.order_id, "failed to fetch order details: {e}");
            return Err(e.into());
        }
    }

    Ok(())
}

/// Handle an [`AmazonMatchJob`]: match unmatched Amazon transactions to bank transactions.
///
/// # Errors
///
/// Returns an error if DB operations fail.
pub async fn handle_amazon_match_job(
    job: AmazonMatchJob,
    db: Data<Db>,
    pool: Data<ApalisPool>,
) -> Result<(), BoxDynError> {
    let run_uuid = parse_run_id(job.schedule_run_id.as_deref());

    let result = amazon_match_inner(&job, &db).await;

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

async fn amazon_match_inner(job: &AmazonMatchJob, db: &Db) -> Result<(), BoxDynError> {
    let unmatched_amazon = db.get_unmatched_amazon_transactions(job.account_id).await?;
    let bank_candidates = db.get_amazon_matchable_bank_transactions().await?;

    let matches = budget_amazon::find_matches(&unmatched_amazon, &bank_candidates);
    let matches_count = matches.len();

    if matches.is_empty() {
        info!(account_id = %job.account_id, "no new Amazon matches found");
    } else {
        db.insert_amazon_matches(&matches).await?;
        info!(account_id = %job.account_id, count = matches_count, "inserted Amazon matches");
    }

    Ok(())
}
