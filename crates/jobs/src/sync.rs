//! Sync job handler: fetches transactions from a bank provider and upserts them
//! into the local database.

use apalis::prelude::*;
use uuid::Uuid;

use budget_core::db::Db;
use budget_core::models::{AccountId, ConnectionStatus, Transaction, TransactionId};

use super::{BankProviderFactory, SyncJob};

/// Fetch transactions from a bank provider for the given account and upsert
/// them into the database.
///
/// This is the shared implementation used by both the standalone sync handler
/// and the pipeline step.
///
/// # Errors
///
/// Returns an error if:
/// - `raw_account_id` is not a valid UUID.
/// - The account does not exist in the database.
/// - The account's connection is missing, expired, or revoked.
/// - The bank provider call fails.
/// - One or more transaction upserts fail (partial failure — successful
///   upserts are preserved).
pub(crate) async fn sync_account(
    raw_account_id: &str,
    db: &Db,
    factory: &BankProviderFactory,
) -> Result<(), BoxDynError> {
    let uuid: Uuid = raw_account_id
        .parse()
        .map_err(|e| format!("invalid account_id UUID: {e}"))?;
    let account_id = AccountId::from_uuid(uuid);

    let account = db
        .get_account(account_id)
        .await?
        .ok_or_else(|| format!("account {account_id} not found"))?;

    tracing::info!(
        account_id = %account.id,
        provider_account_id = %account.provider_account_id,
        institution = %account.institution,
        connection_id = ?account.connection_id,
        "starting sync"
    );

    // Resolve the bank provider from the account's connection
    let provider_name = match account.connection_id {
        Some(conn_id) => {
            let connection = db.get_connection(conn_id).await?.ok_or_else(|| {
                format!("connection {conn_id} not found for account {account_id}")
            })?;

            tracing::debug!(
                connection_id = %conn_id,
                provider = %connection.provider,
                status = %connection.status,
                valid_until = %connection.valid_until,
                "resolved connection"
            );

            if connection.status != ConnectionStatus::Active {
                return Err(format!(
                    "connection {} is {}, cannot sync",
                    conn_id, connection.status
                )
                .into());
            }

            Some(connection.provider)
        }
        None => None,
    };

    let bank = factory.create(provider_name.as_deref())?;

    let provider_account_id = budget_providers::AccountId(account.provider_account_id.clone());

    // Use the most recent transaction date as a starting point (with overlap),
    // or fetch all available history for the initial sync.
    let latest = db.get_latest_transaction_date(account.id).await?;
    let since = latest.map(|date| date - chrono::Duration::days(7));
    tracing::debug!(since = ?since, latest_in_db = ?latest, provider_account_id = %account.provider_account_id, "fetching transactions");
    let provider_txns = bank.fetch_transactions(&provider_account_id, since).await?;

    let count = provider_txns.len();

    let mut failed: Vec<String> = Vec::new();
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
            correlation_id: None,
            correlation_type: None,
            category_method: None,
            suggested_category: None,
            counterparty_name: ptxn.counterparty_name.clone(),
            counterparty_iban: ptxn.counterparty_iban.clone(),
            counterparty_bic: ptxn.counterparty_bic.clone(),
            bank_transaction_code: ptxn.bank_transaction_code.clone(),
        };

        if let Err(e) = db
            .upsert_transaction(&txn, Some(&ptxn.provider_transaction_id))
            .await
        {
            tracing::warn!(
                provider_transaction_id = %ptxn.provider_transaction_id,
                error = %e,
                "failed to upsert transaction, continuing"
            );
            failed.push(format!("{}: {e}", ptxn.provider_transaction_id));
        }
    }

    let succeeded = count - failed.len();
    tracing::info!(
        account_id = %account.id,
        transactions_synced = succeeded,
        transactions_failed = failed.len(),
        "sync completed"
    );

    if !failed.is_empty() {
        return Err(format!(
            "sync partially failed: {succeeded}/{count} succeeded, {} failed: {}",
            failed.len(),
            failed.join(", ")
        )
        .into());
    }

    Ok(())
}

/// Apalis handler that delegates to [`sync_account`].
///
/// # Errors
///
/// Returns an error if the sync fails.
pub async fn handle_sync_job(
    job: SyncJob,
    db: Data<Db>,
    factory: Data<BankProviderFactory>,
) -> Result<(), BoxDynError> {
    sync_account(&job.account_id, &db, &factory).await
}
