//! Sync job handler: fetches transactions from a bank provider and upserts them
//! into the local database.

use apalis::prelude::*;

use budget_core::db::Db;
use budget_core::models::{
    AccountId, Bic, ConnectionStatus, CurrencyCode, ExchangeRateType, Iban, MerchantCategoryCode,
    ReferenceNumberSchema, Transaction,
};

use super::{BankProviderFactory, SyncJob};

/// Try to parse a string into a newtype, logging a warning on failure.
fn try_parse<T, E: std::fmt::Display>(value: &str, field: &str) -> Option<T>
where
    T: std::str::FromStr<Err = E>,
{
    match value.parse() {
        Ok(v) => Some(v),
        Err(e) => {
            tracing::warn!(field, value, %e, "skipping invalid value from provider");
            None
        }
    }
}

/// Map a provider transaction to a domain transaction for a given account.
fn to_domain(account_id: AccountId, ptxn: &budget_providers::Transaction) -> Transaction {
    Transaction {
        account_id,
        amount: ptxn.amount,
        original_amount: ptxn.original_amount,
        original_currency: ptxn
            .original_currency
            .as_deref()
            .and_then(|s| try_parse::<CurrencyCode, _>(s, "original_currency")),
        merchant_name: ptxn.merchant_name.clone(),
        remittance_information: ptxn.remittance_information.clone(),
        posted_date: ptxn.posted_date,
        counterparty_name: ptxn.counterparty_name.clone(),
        counterparty_iban: ptxn
            .counterparty_iban
            .as_deref()
            .and_then(|s| try_parse::<Iban, _>(s, "counterparty_iban")),
        counterparty_bic: ptxn
            .counterparty_bic
            .as_deref()
            .and_then(|s| try_parse::<Bic, _>(s, "counterparty_bic")),
        bank_transaction_code: ptxn.bank_transaction_code.clone(),
        merchant_category_code: ptxn
            .merchant_category_code
            .as_deref()
            .and_then(|s| try_parse::<MerchantCategoryCode, _>(s, "merchant_category_code")),
        bank_transaction_code_code: ptxn.bank_transaction_code_code.clone(),
        bank_transaction_code_sub_code: ptxn.bank_transaction_code_sub_code.clone(),
        exchange_rate: ptxn.exchange_rate.clone(),
        exchange_rate_unit_currency: ptxn
            .exchange_rate_unit_currency
            .as_deref()
            .and_then(|s| try_parse::<CurrencyCode, _>(s, "exchange_rate_unit_currency")),
        exchange_rate_type: ptxn
            .exchange_rate_type
            .as_deref()
            .and_then(|s| try_parse::<ExchangeRateType, _>(s, "exchange_rate_type")),
        exchange_rate_contract_id: ptxn.exchange_rate_contract_id.clone(),
        reference_number: ptxn.reference_number.clone(),
        reference_number_schema: ptxn
            .reference_number_schema
            .as_deref()
            .map(|s| s.parse::<ReferenceNumberSchema>().expect("infallible")),
        note: ptxn.note.clone(),
        balance_after_transaction: ptxn.balance_after_transaction,
        balance_after_transaction_currency: ptxn
            .balance_after_transaction_currency
            .as_deref()
            .and_then(|s| try_parse::<CurrencyCode, _>(s, "balance_after_transaction_currency")),
        creditor_account_additional_id: ptxn.creditor_account_additional_id.clone(),
        debtor_account_additional_id: ptxn.debtor_account_additional_id.clone(),
        ..Default::default()
    }
}

/// Fetch transactions from a bank provider for the given account and upsert
/// them into the database.
///
/// This is the shared implementation used by both the standalone sync handler
/// and the pipeline step.
///
/// # Errors
///
/// Returns an error if:
/// - The account does not exist in the database.
/// - The account's connection is missing, expired, or revoked.
/// - The bank provider call fails.
/// - One or more transaction upserts fail (partial failure — successful
///   upserts are preserved).
pub(crate) async fn sync_account(
    account_id: AccountId,
    db: &Db,
    factory: &BankProviderFactory,
) -> Result<(), BoxDynError> {
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
        let txn = to_domain(account.id, ptxn);

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
    sync_account(job.account_id, &db, &factory).await
}
