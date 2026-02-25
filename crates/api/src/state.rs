use std::sync::Arc;

use apalis::prelude::*;
use apalis_workflow::WorkflowSink;
use budget_core::db::Db;
use budget_jobs::{
    ApalisPool, BudgetRecomputeJob, CategorizeJob, CategorizeTransactionJob, CorrelateJob,
    CorrelateTransactionJob, LlmClient, SyncJob,
};
use budget_providers::EnableBankingAuth;

/// Wrapper around an `ApalisPool` that provides a typed `push` method for
/// enqueueing jobs without exposing the full storage type parameters.
///
/// The inner pool is Arc-based, so cloning is cheap.
pub struct JobStorage<T> {
    pool: ApalisPool,
    _marker: std::marker::PhantomData<T>,
}

impl<T> Clone for JobStorage<T> {
    fn clone(&self) -> Self {
        // Pool is Arc-based, so cloning is cheap
        Self {
            pool: self.pool.clone(),
            _marker: std::marker::PhantomData,
        }
    }
}

impl<T> JobStorage<T> {
    /// Create a new job storage backed by the given pool.
    #[must_use]
    pub fn new(pool: &ApalisPool) -> Self {
        // clone() justified: pool is Arc-based
        Self {
            pool: pool.clone(),
            _marker: std::marker::PhantomData,
        }
    }
}

macro_rules! impl_push {
    ($job:ty) => {
        impl JobStorage<$job> {
            /// Push a job into the queue.
            ///
            /// Creates a fresh storage backend for the push operation. This is
            /// inexpensive because the storage only wraps an Arc'd pool.
            ///
            /// # Errors
            ///
            /// Returns a stringified error if the job cannot be enqueued.
            pub async fn push(&self, job: $job) -> Result<(), String> {
                let mut storage = apalis_postgres::PostgresStorage::new(&self.pool);
                storage.push(job).await.map_err(|e| e.to_string())
            }
        }
    };
}

impl_push!(SyncJob);
impl_push!(CategorizeJob);
impl_push!(CategorizeTransactionJob);
impl_push!(CorrelateJob);
impl_push!(CorrelateTransactionJob);
impl_push!(BudgetRecomputeJob);

/// Storage wrapper for pushing jobs into the full-sync pipeline workflow.
///
/// Internally creates a storage backend and uses `WorkflowSink::push_start`
/// to enqueue the initial pipeline step.
#[derive(Clone)]
pub struct PipelineStorage {
    pool: ApalisPool,
}

impl PipelineStorage {
    /// Create a new pipeline storage backed by the given pool.
    #[must_use]
    pub fn new(pool: &ApalisPool) -> Self {
        // clone() justified: pool is Arc-based
        Self { pool: pool.clone() }
    }

    /// Start a full-sync pipeline for the given account.
    ///
    /// # Errors
    ///
    /// Returns a stringified error if the job cannot be enqueued.
    pub async fn push_start(&self, account_id: String) -> Result<(), String> {
        let mut storage = apalis_postgres::PostgresStorage::<Vec<u8>>::new(&self.pool);
        storage
            .push_start(account_id)
            .await
            .map_err(|e| e.to_string())
    }
}

/// Shared application state passed to all route handlers via axum's State extractor.
///
/// Each `JobStorage` wraps a cloned pool handle internally,
/// so cloning `AppState` is cheap and does not duplicate connections.
#[derive(Clone)]
pub struct AppState {
    /// Database handle for all domain queries.
    pub db: Db,
    /// Static bearer token for API authentication.
    pub secret_key: String,
    /// Job queue storage for bank account sync jobs.
    pub sync_storage: JobStorage<SyncJob>,
    /// Job queue storage for transaction categorization jobs.
    pub categorize_storage: JobStorage<CategorizeJob>,
    /// Job queue storage for transaction correlation jobs.
    pub correlate_storage: JobStorage<CorrelateJob>,
    /// Job queue storage for budget recomputation jobs.
    pub recompute_storage: JobStorage<BudgetRecomputeJob>,
    /// Storage for enqueuing full-sync pipeline workflows.
    pub pipeline_storage: PipelineStorage,
    /// Typed pool for apalis job queries (list, counts, reclaim).
    pub apalis_pool: ApalisPool,
    /// Enable Banking auth provider (None if not configured).
    pub enable_banking_auth: Option<Arc<EnableBankingAuth>>,
    /// LLM provider for rule generation.
    pub llm: LlmClient,
    /// Public base URL (e.g. `https://budget.example.com`). Derived from
    /// `server_port` when not explicitly configured.
    pub host: String,
}
