use apalis::prelude::*;
use apalis_sqlite::SqliteStorage;
use axum::routing::get;
use axum::{Json, Router};
use budget_jobs::{
    BankClient, BudgetRecomputeJob, CategorizeJob, CorrelateJob, LlmClient, NoOpJob, SyncJob,
};
use budget_providers::{MockBankProvider, MockLlmProvider};
use sqlx::SqlitePool;
use tracing_subscriber::EnvFilter;

use api::routes;
use api::state::{AppState, JobStorage};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config = budget_core::load_config()?;
    tracing::info!(port = config.server_port, db = %config.database_url, "starting budget server");

    let pool = SqlitePool::connect(&config.database_url).await?;

    // Apalis migrations first (creates job queue tables)
    SqliteStorage::setup(&pool).await?;

    // Application migrations (ignore apalis-owned entries already in _sqlx_migrations)
    let mut migrator = sqlx::migrate!("../../migrations");
    migrator.set_ignore_missing(true);
    migrator.run(&pool).await?;
    tracing::info!("migrations applied");

    // Type-erased provider wrappers for apalis Data injection
    // clone() on BankClient/LlmClient is cheap: they wrap Arc internally
    let bank = BankClient::new(MockBankProvider::new());
    let llm = LlmClient::new(MockLlmProvider::new());

    // Workers for each job type (backend first, then data injection)
    // clone() on pool and storages is justified: they are Arc-based handles
    let sync_worker = WorkerBuilder::new("budget-sync")
        .backend(SqliteStorage::<SyncJob, _, _>::new(&pool))
        .data(pool.clone())
        .data(bank)
        .build(budget_jobs::handle_sync_job);

    let categorize_worker = WorkerBuilder::new("budget-categorize")
        .backend(SqliteStorage::<CategorizeJob, _, _>::new(&pool))
        .data(pool.clone())
        .data(llm.clone())
        .build(budget_jobs::handle_categorize_job);

    let correlate_worker = WorkerBuilder::new("budget-correlate")
        .backend(SqliteStorage::<CorrelateJob, _, _>::new(&pool))
        .data(pool.clone())
        .data(llm)
        .build(budget_jobs::handle_correlate_job);

    let recompute_worker = WorkerBuilder::new("budget-recompute")
        .backend(SqliteStorage::<BudgetRecomputeJob, _, _>::new(&pool))
        .data(pool.clone())
        .build(budget_jobs::handle_recompute_job);

    let noop_worker = WorkerBuilder::new("budget-no-op")
        .backend(SqliteStorage::<NoOpJob, _, _>::new(&pool))
        .build(budget_jobs::handle_noop_job);

    tracing::info!("job queue initialized");

    let state = AppState {
        pool: pool.clone(),
        sync_storage: JobStorage::new(&pool),
        categorize_storage: JobStorage::new(&pool),
        correlate_storage: JobStorage::new(&pool),
        recompute_storage: JobStorage::new(&pool),
    };

    let app = Router::new()
        .route("/health", get(health))
        .nest("/api/accounts", routes::accounts::router())
        .nest("/api/transactions", routes::transactions::router())
        .nest("/api/categories", routes::categories::router())
        .nest("/api/rules", routes::rules::router())
        .nest("/api/budgets", routes::budgets::router())
        .nest("/api/projects", routes::projects::router())
        .nest("/api/jobs", routes::jobs::router())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", config.server_port)).await?;
    tracing::info!(port = config.server_port, "listening");

    tokio::select! {
        res = axum::serve(listener, app) => {
            if let Err(e) = res { tracing::error!(%e, "server error"); }
        }
        res = sync_worker.run() => {
            if let Err(e) = res { tracing::error!(%e, "sync worker error"); }
        }
        res = categorize_worker.run() => {
            if let Err(e) = res { tracing::error!(%e, "categorize worker error"); }
        }
        res = correlate_worker.run() => {
            if let Err(e) = res { tracing::error!(%e, "correlate worker error"); }
        }
        res = recompute_worker.run() => {
            if let Err(e) = res { tracing::error!(%e, "recompute worker error"); }
        }
        res = noop_worker.run() => {
            if let Err(e) = res { tracing::error!(%e, "noop worker error"); }
        }
    }

    Ok(())
}

/// Health check endpoint (unauthenticated).
async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok"}))
}
