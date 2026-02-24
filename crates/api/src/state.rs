use apalis::prelude::*;
use apalis_sqlite::SqliteStorage;
use budget_jobs::{BudgetRecomputeJob, CategorizeJob, CorrelateJob, SyncJob};
use sqlx::SqlitePool;

/// Wrapper around a `SqlitePool` that provides a typed `push` method for
/// enqueueing jobs without exposing the full `SqliteStorage` type parameters.
///
/// The inner pool is Arc-based, so cloning is cheap.
pub struct JobStorage<T> {
    pool: SqlitePool,
    _marker: std::marker::PhantomData<T>,
}

impl<T> Clone for JobStorage<T> {
    fn clone(&self) -> Self {
        // SqlitePool is Arc-based, so cloning is cheap
        Self {
            pool: self.pool.clone(),
            _marker: std::marker::PhantomData,
        }
    }
}

impl<T> JobStorage<T> {
    /// Create a new job storage backed by the given pool.
    #[must_use]
    pub fn new(pool: &SqlitePool) -> Self {
        // clone() justified: SqlitePool is Arc-based
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
            /// Creates a fresh `SqliteStorage` for the push operation. This is
            /// inexpensive because the storage only wraps an Arc'd pool.
            ///
            /// # Errors
            ///
            /// Returns a stringified error if the job cannot be enqueued.
            pub async fn push(&self, job: $job) -> Result<(), String> {
                let mut storage = SqliteStorage::<$job, _, _>::new(&self.pool);
                storage.push(job).await.map_err(|e| e.to_string())
            }
        }
    };
}

impl_push!(SyncJob);
impl_push!(CategorizeJob);
impl_push!(CorrelateJob);
impl_push!(BudgetRecomputeJob);

/// Shared application state passed to all route handlers via axum's State extractor.
///
/// Each `JobStorage` wraps a cloned `SqlitePool` handle internally,
/// so cloning `AppState` is cheap and does not duplicate connections.
#[derive(Clone)]
pub struct AppState {
    /// Database connection pool for all queries.
    pub pool: SqlitePool,
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
}
