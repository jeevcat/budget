use std::pin::Pin;
use std::sync::Arc;

use apalis::prelude::*;
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use budget_providers::{
    CategorizeResult, CorrelationResult, EnableBankingConfig, EnableBankingProvider, ProposedRule,
    ProviderError, TransactionSummary,
};

pub mod categorize;
pub mod correlate;
pub mod pipeline;
pub mod queries;
pub mod recompute;
pub mod sync;

// ---------------------------------------------------------------------------
// Apalis pool type alias
//
// Apalis storage backends need a concrete pool type (SqlitePool or PgPool)
// rather than AnyPool. This alias selects the right one based on the
// compile-time feature flag.
// ---------------------------------------------------------------------------

/// Concrete pool type for apalis storage, selected at compile time.
/// When both features are active (e.g. `--all-features`), sqlite wins.
#[cfg(feature = "sqlite")]
pub type ApalisPool = sqlx::SqlitePool;

/// Concrete pool type for apalis storage, selected at compile time.
#[cfg(all(feature = "postgres", not(feature = "sqlite")))]
pub type ApalisPool = sqlx::postgres::PgPool;

/// Apalis jobs table name — differs between backends.
#[cfg(feature = "sqlite")]
pub const JOBS_TABLE: &str = "Jobs";

/// Apalis jobs table name — differs between backends.
#[cfg(all(feature = "postgres", not(feature = "sqlite")))]
pub const JOBS_TABLE: &str = "apalis.jobs";

pub use categorize::{handle_categorize_job, handle_categorize_transaction_job};
pub use correlate::{handle_correlate_job, handle_correlate_transaction_job};
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
        since: Option<NaiveDate>,
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
        since: Option<NaiveDate>,
    ) -> BoxFuture<'a, Result<Vec<budget_providers::Transaction>, ProviderError>>;
}

impl<T: budget_providers::BankProvider + Sync> ErasedBankProvider for T {
    fn fetch_transactions_erased<'a>(
        &'a self,
        account_id: &'a budget_providers::AccountId,
        since: Option<NaiveDate>,
    ) -> BoxFuture<'a, Result<Vec<budget_providers::Transaction>, ProviderError>> {
        Box::pin(self.fetch_transactions(account_id, since))
    }
}

// ---------------------------------------------------------------------------
// Bank provider factory
//
// The sync job needs to construct the right `BankProvider` based on which bank
// connection an account belongs to. Rather than injecting a single static
// provider, we inject a factory that creates a provider per-connection.
// ---------------------------------------------------------------------------

/// Factory for creating bank providers based on a connection's provider type.
///
/// Holds the configuration needed to construct provider-specific clients
/// (e.g. Enable Banking API credentials). Injected into the sync worker via
/// `Data<BankProviderFactory>`.
///
/// For testing, a `fallback` provider handles accounts without a connection.
#[derive(Clone)]
pub struct BankProviderFactory {
    eb_config: Option<EnableBankingConfig>,
    fallback: Option<BankClient>,
}

impl BankProviderFactory {
    /// Create a factory with optional Enable Banking credentials.
    #[must_use]
    pub fn new(eb_config: Option<EnableBankingConfig>) -> Self {
        Self {
            eb_config,
            fallback: None,
        }
    }

    /// Set a fallback provider for accounts without a connection (testing).
    #[must_use]
    pub fn with_fallback(mut self, fallback: BankClient) -> Self {
        self.fallback = Some(fallback);
        self
    }

    /// Create a `BankClient` for the given provider name.
    ///
    /// `provider` is the `connection.provider` value from the database
    /// (e.g. `"enable_banking"`), or `None` for accounts without a connection.
    ///
    /// # Errors
    ///
    /// Returns an error if the provider is unsupported or not configured.
    pub fn create(&self, provider: Option<&str>) -> Result<BankClient, String> {
        match provider {
            Some("enable_banking") => {
                // clone() justified: EnableBankingConfig is small (app ID + PEM
                // bytes + base URL), and this runs at most once per sync job
                let config = self
                    .eb_config
                    .clone()
                    .ok_or("Enable Banking provider not configured")?;
                let client = budget_providers::EnableBankingClient::new(config);
                Ok(BankClient::new(EnableBankingProvider::new(
                    client,
                    String::new(),
                    vec![],
                )))
            }
            Some(other) => Err(format!("unsupported bank provider: {other}")),
            None => self
                .fallback
                .clone()
                .ok_or_else(|| "account has no connection".to_owned()),
        }
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
    /// When `existing_categories` is non-empty, the LLM is instructed to
    /// prefer these known names rather than inventing new ones.
    ///
    /// # Errors
    ///
    /// Propagates any [`ProviderError`] from the underlying provider.
    pub async fn categorize(
        &self,
        merchant_name: &str,
        amount: Decimal,
        description: Option<&str>,
        existing_categories: &[String],
    ) -> Result<CategorizeResult, ProviderError> {
        self.inner
            .categorize_erased(merchant_name, amount, description, existing_categories)
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

    /// Ask the LLM to propose a deterministic rule from multiple merchant
    /// examples that share the same category.
    ///
    /// # Errors
    ///
    /// Propagates any [`ProviderError`] from the underlying provider.
    pub async fn propose_rule(
        &self,
        merchant_examples: &[String],
        user_category: &str,
    ) -> Result<ProposedRule, ProviderError> {
        self.inner
            .propose_rule_erased(merchant_examples, user_category)
            .await
    }
}

// Manual dyn-compatible mirror of the subset of `LlmProvider` used by jobs.
trait ErasedLlmProvider {
    fn categorize_erased<'a>(
        &'a self,
        merchant_name: &'a str,
        amount: Decimal,
        description: Option<&'a str>,
        existing_categories: &'a [String],
    ) -> BoxFuture<'a, Result<CategorizeResult, ProviderError>>;

    fn propose_correlation_erased<'a>(
        &'a self,
        txn_a: &'a TransactionSummary,
        txn_b: &'a TransactionSummary,
    ) -> BoxFuture<'a, Result<CorrelationResult, ProviderError>>;

    fn propose_rule_erased<'a>(
        &'a self,
        merchant_examples: &'a [String],
        user_category: &'a str,
    ) -> BoxFuture<'a, Result<ProposedRule, ProviderError>>;
}

impl<T: budget_providers::LlmProvider + Sync> ErasedLlmProvider for T {
    fn categorize_erased<'a>(
        &'a self,
        merchant_name: &'a str,
        amount: Decimal,
        description: Option<&'a str>,
        existing_categories: &'a [String],
    ) -> BoxFuture<'a, Result<CategorizeResult, ProviderError>> {
        Box::pin(self.categorize(merchant_name, amount, description, existing_categories))
    }

    fn propose_correlation_erased<'a>(
        &'a self,
        txn_a: &'a TransactionSummary,
        txn_b: &'a TransactionSummary,
    ) -> BoxFuture<'a, Result<CorrelationResult, ProviderError>> {
        Box::pin(self.propose_correlation(txn_a, txn_b))
    }

    fn propose_rule_erased<'a>(
        &'a self,
        merchant_examples: &'a [String],
        user_category: &'a str,
    ) -> BoxFuture<'a, Result<ProposedRule, ProviderError>> {
        Box::pin(self.propose_rule(merchant_examples, user_category))
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

/// Categorize a single transaction via LLM (enqueued by the fan-out job).
#[derive(Debug, Serialize, Deserialize)]
pub struct CategorizeTransactionJob {
    pub transaction_id: String,
}

/// Correlate a single transaction via LLM (enqueued by the fan-out job).
#[derive(Debug, Serialize, Deserialize)]
pub struct CorrelateTransactionJob {
    pub transaction_id: String,
}

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
