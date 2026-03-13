//! Amazon enrichment jobs: transaction sync, order detail fetching, and matching.
//!
//! Split into three job types that run via the apalis queue:
//! - [`AmazonSyncJob`]: fetches transactions, upserts to DB, enqueues order fetches + match job
//! - [`AmazonFetchOrderJob`]: fetches a single order's invoice page (rate-limited by the worker)
//! - [`AmazonMatchJob`]: matches Amazon transactions to bank transactions

use std::path::PathBuf;

use apalis::prelude::*;
use tracing::{debug, error, info, warn};

use budget_amazon::{AmazonClient, CookieStore};
use budget_db::Db;

use super::pipeline::parse_run_id;
use super::schedule_queries::{self, RunStatus};
use super::{AmazonFetchOrderJob, AmazonMatchJob, ApalisPool};

/// Configuration for the Amazon enrichment client.
#[derive(Clone)]
pub struct AmazonEnrichConfig {
    pub cookies_dir: PathBuf,
}

/// Handle an [`AmazonSyncJob`]: load cookies, paginate transactions, upsert to DB,
/// then fan out one [`AmazonFetchOrderJob`] per unfetched order and one [`AmazonMatchJob`].
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

    let result = amazon_sync_inner(account_id, &db, &config, &pool).await;

    if let Err(ref e) = result {
        if let Some(run_uuid) = parse_run_id(job.schedule_run_id.as_deref()) {
            let _ = schedule_queries::complete_schedule_run(
                &pool,
                run_uuid,
                RunStatus::Failed,
                Some(&e.to_string()),
            )
            .await;
        }
        return result;
    }

    // Enqueue the match job, propagating schedule_run_id
    let mut match_storage = apalis_postgres::PostgresStorage::<AmazonMatchJob>::new(&pool);
    match_storage
        .push(AmazonMatchJob {
            account_id,
            schedule_run_id: job.schedule_run_id,
        })
        .await
        .map_err(|e| format!("failed to enqueue match job: {e}"))?;

    Ok(())
}

/// Core sync logic extracted so errors can be caught for schedule run tracking.
async fn amazon_sync_inner(
    account_id: budget_core::models::AmazonAccountId,
    db: &Db,
    config: &AmazonEnrichConfig,
    pool: &ApalisPool,
) -> Result<(), BoxDynError> {
    let cookies_path = config.cookies_dir.join(format!("{account_id}.json"));

    let cookies = CookieStore::load(&cookies_path).map_err(|e| {
        error!(path = %cookies_path.display(), %account_id, "failed to load Amazon cookies: {e}");
        e
    })?;

    if cookies.is_expired() {
        warn!(%account_id, "Amazon cookies are expired — re-login required");
        return Err("Amazon cookies expired".into());
    }

    let mut client = AmazonClient::new(cookies).map_err(|e| {
        error!(%account_id, "failed to create Amazon client: {e}");
        e
    })?;

    // Get known dedup keys for incremental sync
    let known_keys = db.get_amazon_dedup_keys(account_id).await?;
    debug!(%account_id, known = known_keys.len(), "loaded known Amazon dedup keys");

    // Fetch new transactions, stopping when we hit a known one
    let transactions = client
        .fetch_all_transactions(|key| known_keys.contains(key))
        .await
        .map_err(|e| {
            error!(%account_id, "failed to fetch Amazon transactions: {e}");
            e
        })?;

    info!(
        %account_id,
        count = transactions.len(),
        "fetched new Amazon transactions"
    );

    // Upsert transactions to DB
    for txn in &transactions {
        db.upsert_amazon_transaction(account_id, txn).await?;
    }

    // Enqueue one job per unfetched order
    let unfetched_orders = db.get_unfetched_order_ids(account_id).await?;
    info!(
        %account_id,
        count = unfetched_orders.len(),
        "enqueuing order detail fetch jobs"
    );

    let mut order_storage = apalis_postgres::PostgresStorage::<AmazonFetchOrderJob>::new(pool);
    for order_id in &unfetched_orders {
        order_storage
            .push(AmazonFetchOrderJob {
                account_id,
                order_id: order_id.clone(),
            })
            .await
            .map_err(|e| format!("failed to enqueue order fetch for {order_id}: {e}"))?;
    }

    info!(
        %account_id,
        order_jobs = unfetched_orders.len(),
        "Amazon sync job completed"
    );
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
