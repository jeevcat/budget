use std::num::NonZeroU32;
use std::sync::Arc;

use budget_core::db::Db;
use budget_jobs::{
    ApalisPool, CategorizeJob, CorrelateJob, JobStorage, LlmClient, PipelineStorage, SyncJob,
};
use budget_providers::EnableBankingAuth;

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
    /// Storage for enqueuing full-sync pipeline workflows.
    pub pipeline_storage: PipelineStorage,
    /// Typed pool for apalis job queries (list, counts, reclaim).
    pub apalis_pool: ApalisPool,
    /// Enable Banking auth provider (None if not configured).
    pub enable_banking_auth: Option<Arc<EnableBankingAuth>>,
    /// LLM provider for rule generation.
    pub llm: LlmClient,
    /// Expected number of salary transactions per month for budget boundary detection.
    pub expected_salary_count: NonZeroU32,
    /// Public base URL (e.g. `https://budget.example.com`). Derived from
    /// `server_port` when not explicitly configured.
    pub host: String,
}
