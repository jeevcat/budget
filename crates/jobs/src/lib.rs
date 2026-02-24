use std::pin::Pin;
use std::sync::Arc;

use apalis::prelude::*;
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use budget_providers::{CategorizeResult, CorrelationResult, ProviderError, TransactionSummary};

pub mod categorize;
pub mod correlate;
pub mod recompute;
pub mod sync;

pub use categorize::handle_categorize_job;
pub use correlate::handle_correlate_job;
pub use recompute::handle_recompute_job;
pub use sync::handle_sync_job;

// ---------------------------------------------------------------------------
// Type-erased provider wrappers
//
// The provider traits use `async fn` (via trait_variant), which desugars to
// `-> impl Future`. That makes them incompatible with `dyn` dispatch.
// These wrapper types box the future so they can be stored in `Data<T>` and
// shared across the apalis worker pool.
// ---------------------------------------------------------------------------

type BoxFuture<'a, T> = Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

/// Type-erased bank provider suitable for injection into apalis handlers
/// via `Data<BankClient>`.
///
/// Construct with [`BankClient::new`], passing any type that implements
/// [`budget_providers::BankProvider`].
/// Clone is derived from the inner `Arc`, so cloning is a cheap
/// atomic reference count increment.
#[derive(Clone)]
pub struct BankClient {
    inner: Arc<dyn ErasedBankProvider + Send + Sync>,
}

impl BankClient {
    /// Wrap a concrete [`BankProvider`](budget_providers::BankProvider)
    /// implementation for dynamic dispatch.
    pub fn new<T: budget_providers::BankProvider + Sync + 'static>(provider: T) -> Self {
        Self {
            inner: Arc::new(provider),
        }
    }

    /// Fetch transactions from the provider.
    ///
    /// # Errors
    ///
    /// Propagates any [`ProviderError`] from the underlying provider.
    pub async fn fetch_transactions(
        &self,
        account_id: &budget_providers::AccountId,
        since: NaiveDate,
    ) -> Result<Vec<budget_providers::Transaction>, ProviderError> {
        self.inner
            .fetch_transactions_erased(account_id, since)
            .await
    }
}

// Manual dyn-compatible mirror of the subset of `BankProvider` used by jobs.
trait ErasedBankProvider {
    fn fetch_transactions_erased<'a>(
        &'a self,
        account_id: &'a budget_providers::AccountId,
        since: NaiveDate,
    ) -> BoxFuture<'a, Result<Vec<budget_providers::Transaction>, ProviderError>>;
}

impl<T: budget_providers::BankProvider + Sync> ErasedBankProvider for T {
    fn fetch_transactions_erased<'a>(
        &'a self,
        account_id: &'a budget_providers::AccountId,
        since: NaiveDate,
    ) -> BoxFuture<'a, Result<Vec<budget_providers::Transaction>, ProviderError>> {
        Box::pin(self.fetch_transactions(account_id, since))
    }
}

/// Type-erased LLM provider suitable for injection into apalis handlers
/// via `Data<LlmClient>`.
///
/// Construct with [`LlmClient::new`], passing any type that implements
/// [`budget_providers::LlmProvider`].
/// Clone is derived from the inner `Arc`, so cloning is a cheap
/// atomic reference count increment.
#[derive(Clone)]
pub struct LlmClient {
    inner: Arc<dyn ErasedLlmProvider + Send + Sync>,
}

impl LlmClient {
    /// Wrap a concrete [`LlmProvider`](budget_providers::LlmProvider)
    /// implementation for dynamic dispatch.
    pub fn new<T: budget_providers::LlmProvider + Sync + 'static>(provider: T) -> Self {
        Self {
            inner: Arc::new(provider),
        }
    }

    /// Classify a transaction by merchant name, amount, and description.
    ///
    /// # Errors
    ///
    /// Propagates any [`ProviderError`] from the underlying provider.
    pub async fn categorize(
        &self,
        merchant_name: &str,
        amount: Decimal,
        description: Option<&str>,
    ) -> Result<CategorizeResult, ProviderError> {
        self.inner
            .categorize_erased(merchant_name, amount, description)
            .await
    }

    /// Propose whether two transactions are correlated (transfer /
    /// reimbursement).
    ///
    /// # Errors
    ///
    /// Propagates any [`ProviderError`] from the underlying provider.
    pub async fn propose_correlation(
        &self,
        txn_a: &TransactionSummary,
        txn_b: &TransactionSummary,
    ) -> Result<CorrelationResult, ProviderError> {
        self.inner.propose_correlation_erased(txn_a, txn_b).await
    }
}

// Manual dyn-compatible mirror of the subset of `LlmProvider` used by jobs.
trait ErasedLlmProvider {
    fn categorize_erased<'a>(
        &'a self,
        merchant_name: &'a str,
        amount: Decimal,
        description: Option<&'a str>,
    ) -> BoxFuture<'a, Result<CategorizeResult, ProviderError>>;

    fn propose_correlation_erased<'a>(
        &'a self,
        txn_a: &'a TransactionSummary,
        txn_b: &'a TransactionSummary,
    ) -> BoxFuture<'a, Result<CorrelationResult, ProviderError>>;
}

impl<T: budget_providers::LlmProvider + Sync> ErasedLlmProvider for T {
    fn categorize_erased<'a>(
        &'a self,
        merchant_name: &'a str,
        amount: Decimal,
        description: Option<&'a str>,
    ) -> BoxFuture<'a, Result<CategorizeResult, ProviderError>> {
        Box::pin(self.categorize(merchant_name, amount, description))
    }

    fn propose_correlation_erased<'a>(
        &'a self,
        txn_a: &'a TransactionSummary,
        txn_b: &'a TransactionSummary,
    ) -> BoxFuture<'a, Result<CorrelationResult, ProviderError>> {
        Box::pin(self.propose_correlation(txn_a, txn_b))
    }
}

// ---------------------------------------------------------------------------
// Job definitions
// ---------------------------------------------------------------------------

/// Fetch new transactions from a bank provider for a specific account.
#[derive(Debug, Serialize, Deserialize)]
pub struct SyncJob {
    /// The domain `AccountId` (UUID string) to sync.
    pub account_id: String,
}

/// Run categorization rules and LLM on all uncategorized transactions.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct CategorizeJob;

/// Attempt to correlate uncorrelated transactions (transfers, reimbursements).
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct CorrelateJob;

/// Recompute budget month boundaries and assign transactions to months.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct BudgetRecomputeJob;

/// A no-op job for health checks and testing the job queue.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct NoOpJob;

/// Handler for the no-op test job.
///
/// # Errors
///
/// Currently infallible, but returns `Result` to match the apalis handler contract.
#[allow(clippy::unused_async)] // apalis requires async handlers
pub async fn handle_noop_job(_job: NoOpJob) -> Result<(), BoxDynError> {
    tracing::info!("no-op job executed");
    Ok(())
}
