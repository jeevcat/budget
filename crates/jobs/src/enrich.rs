//! Amazon enrichment job: fetches Amazon transactions and order details,
//! matches them against bank transactions, and stores the results.

use std::path::PathBuf;
use std::time::Duration;

use tracing::{debug, error, info, warn};

use budget_amazon::{AmazonClient, CookieStore};
use budget_db::Db;

/// Configuration for the Amazon enrichment client.
#[derive(Clone)]
pub struct AmazonEnrichConfig {
    pub base_url: String,
    pub cookies_path: PathBuf,
}

/// Run the Amazon enrichment job.
///
/// 1. Loads cookies and verifies they're valid
/// 2. Fetches new transactions (stops at already-known ones)
/// 3. Upserts transactions to DB
/// 4. Fetches order details for unfetched order IDs
/// 5. Matches Amazon transactions to bank transactions
///
/// # Errors
///
/// Returns an error if cookies are expired, network requests fail, or DB operations fail.
pub async fn run_amazon_enrich(
    db: &Db,
    config: &AmazonEnrichConfig,
) -> Result<EnrichResult, Box<dyn std::error::Error + Send + Sync>> {
    info!("starting Amazon enrichment job");

    let cookies = CookieStore::load(&config.cookies_path).map_err(|e| {
        error!(path = %config.cookies_path.display(), "failed to load Amazon cookies: {e}");
        e
    })?;

    if cookies.is_expired() {
        warn!("Amazon cookies are expired — re-login required");
        return Err("Amazon cookies expired".into());
    }

    let mut client = AmazonClient::new(cookies, config.base_url.clone()).map_err(|e| {
        error!("failed to create Amazon client: {e}");
        e
    })?;

    // Get known dedup keys for incremental sync
    let known_keys = db.get_amazon_dedup_keys().await?;
    debug!(known = known_keys.len(), "loaded known Amazon dedup keys");

    // Fetch new transactions, stopping when we hit a known one
    let transactions = client
        .fetch_all_transactions(|key| known_keys.contains(key))
        .await
        .map_err(|e| {
            error!("failed to fetch Amazon transactions: {e}");
            e
        })?;

    info!(
        count = transactions.len(),
        "fetched new Amazon transactions"
    );

    // Upsert transactions to DB
    for txn in &transactions {
        db.upsert_amazon_transaction(txn).await?;
    }

    // Fetch order details for any unfetched order IDs
    let unfetched_orders = db.get_unfetched_order_ids().await?;
    info!(
        count = unfetched_orders.len(),
        "fetching order details for unfetched orders"
    );

    let mut orders_fetched = 0;
    for order_id in &unfetched_orders {
        // Randomized delay between order fetches (2-5 seconds)
        let delay_secs = 2 + (rand_u32() % 4);
        tokio::time::sleep(Duration::from_secs(u64::from(delay_secs))).await;

        match client.fetch_order_details(order_id).await {
            Ok(order) => {
                db.upsert_amazon_order(&order).await?;
                orders_fetched += 1;
                debug!(order_id = %order_id, items = order.items.len(), "fetched order details");
            }
            Err(e) => {
                warn!(order_id = %order_id, "failed to fetch order details: {e}");
            }
        }
    }

    // Match Amazon transactions to bank transactions
    let unmatched_amazon = db.get_unmatched_amazon_transactions().await?;
    let bank_candidates = db.get_amazon_matchable_bank_transactions().await?;

    let matches = budget_amazon::find_matches(&unmatched_amazon, &bank_candidates);
    let matches_count = matches.len();

    if !matches.is_empty() {
        db.insert_amazon_matches(&matches).await?;
        info!(count = matches_count, "inserted Amazon matches");
    }

    let result = EnrichResult {
        transactions_fetched: transactions.len(),
        orders_fetched,
        matches_created: matches_count,
    };

    info!(?result, "Amazon enrichment job completed");
    Ok(result)
}

#[derive(Debug)]
pub struct EnrichResult {
    pub transactions_fetched: usize,
    pub orders_fetched: usize,
    pub matches_created: usize,
}

/// Simple non-crypto random u32 for delay jitter.
#[allow(clippy::cast_possible_truncation)]
fn rand_u32() -> u32 {
    use std::time::SystemTime;
    let t = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    t.subsec_nanos() ^ (t.as_secs() as u32)
}
