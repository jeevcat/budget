//! Sync job handler: fetches transactions from a bank provider and upserts them
//! into the local database.

use apalis::prelude::*;
use chrono::Utc;
use sqlx::SqlitePool;
use uuid::Uuid;

use budget_core::db;
use budget_core::models::{AccountId, Transaction, TransactionId};

use super::{BankClient, SyncJob};

/// Fetch transactions from the bank provider for the account specified in
/// `job.account_id` and upsert them into the database.
///
/// The handler pulls the last 90 days of transactions from the provider,
/// converts each to a domain `Transaction`, and upserts with provider-level
/// deduplication so re-syncs are safe.
///
/// # Errors
///
/// Returns an error if:
/// - `job.account_id` is not a valid UUID.
/// - The account does not exist in the database.
/// - The bank provider call fails.
/// - Any database write fails.
pub async fn handle_sync_job(
    job: SyncJob,
    pool: Data<SqlitePool>,
    bank: Data<BankClient>,
) -> Result<(), BoxDynError> {
    let uuid: Uuid = job
        .account_id
        .parse()
        .map_err(|e| format!("invalid account_id UUID: {e}"))?;
    let account_id = AccountId::from_uuid(uuid);

    let account = db::get_account(&pool, account_id)
        .await?
        .ok_or_else(|| format!("account {account_id} not found"))?;

    let provider_account_id = budget_providers::AccountId(account.provider_account_id.clone());

    // Fetch the last 90 days of transactions
    let since = Utc::now().date_naive() - chrono::Duration::days(90);
    let provider_txns = bank.fetch_transactions(&provider_account_id, since).await?;

    let count = provider_txns.len();

    for ptxn in &provider_txns {
        let txn = Transaction {
            id: TransactionId::new(),
            account_id: account.id,
            category_id: None,
            amount: ptxn.amount,
            original_amount: ptxn.original_amount,
            original_currency: ptxn.original_currency.clone(),
            merchant_name: ptxn.merchant_name.clone(),
            description: ptxn.description.clone().unwrap_or_default(),
            posted_date: ptxn.posted_date,
            budget_month_id: None,
            project_id: None,
            correlation_id: None,
            correlation_type: None,
        };

        db::upsert_transaction(&pool, &txn, Some(&ptxn.provider_transaction_id)).await?;
    }

    tracing::info!(
        account_id = %account.id,
        transactions_synced = count,
        "sync job completed"
    );

    Ok(())
}
