use std::sync::Arc;

use apalis::prelude::*;
use apalis_sqlite::SqliteStorage;
use axum::middleware;
use axum::routing::get;
use axum::{Json, Router};
use budget_jobs::{
    BankProviderFactory, BudgetRecomputeJob, CategorizeJob, CorrelateJob, LlmClient, NoOpJob,
    SyncJob,
};
use budget_providers::{
    EnableBankingAuth, EnableBankingClient, EnableBankingConfig, GeminiProvider, MockLlmProvider,
};
use sqlx::SqlitePool;
use tracing_subscriber::EnvFilter;

use api::auth;
use api::routes;
use api::state::{AppState, JobStorage};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if let Some(cmd) = std::env::args().nth(1) {
        return dispatch_subcommand(&cmd);
    }

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

    // Provider wrappers for apalis Data injection
    let (enable_banking_auth, eb_config) = init_enable_banking(&config);
    let bank_factory = BankProviderFactory::new(eb_config);
    let llm = init_llm_provider(&config);

    // Workers for each job type (backend first, then data injection)
    // clone() on pool and storages is justified: they are Arc-based handles
    let sync_worker = WorkerBuilder::new("budget-sync")
        .backend(SqliteStorage::<SyncJob, _, _>::new(&pool))
        .data(pool.clone())
        .data(bank_factory)
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
        secret_key: config.secret_key,
        sync_storage: JobStorage::new(&pool),
        categorize_storage: JobStorage::new(&pool),
        correlate_storage: JobStorage::new(&pool),
        recompute_storage: JobStorage::new(&pool),
        enable_banking_auth,
        redirect_url: config.redirect_url,
    };

    // Protected API routes require a valid bearer token
    let api_routes = Router::new()
        .nest("/accounts", routes::accounts::router())
        .nest("/transactions", routes::transactions::router())
        .nest("/categories", routes::categories::router())
        .nest("/rules", routes::rules::router())
        .nest("/budgets", routes::budgets::router())
        .nest("/projects", routes::projects::router())
        .nest("/jobs", routes::jobs::router())
        .nest("/connections", routes::connections::router())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_bearer_token,
        ));

    // Callback is unauthenticated — mounted before the /api nest
    let app = Router::new()
        .route("/health", get(health))
        .merge(routes::connections::callback_router())
        .nest("/api", api_routes)
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

fn dispatch_subcommand(cmd: &str) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        "config" => {
            let path = budget_core::config_path()?;
            let exists = path.exists();
            println!("{}", path.display());
            if !exists {
                eprintln!("(file does not exist yet — will be created on first run)");
            }
            Ok(())
        }
        other => {
            eprintln!("unknown command: {other}");
            eprintln!("usage: budget [config]");
            std::process::exit(1);
        }
    }
}

/// Health check endpoint (unauthenticated).
async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok"}))
}

/// Build the LLM provider from config. Uses Gemini when an API key is
/// configured, otherwise falls back to the mock provider.
fn init_llm_provider(config: &budget_core::Config) -> LlmClient {
    match config.gemini_api_key.as_ref() {
        Some(api_key) if !api_key.is_empty() => {
            tracing::info!(model = %config.llm_model, "using Gemini LLM provider");
            LlmClient::new(GeminiProvider::new(
                api_key.clone(),
                config.llm_model.clone(),
            ))
        }
        _ => {
            tracing::info!("no Gemini API key configured, using mock LLM provider");
            LlmClient::new(MockLlmProvider::new())
        }
    }
}

/// Build the Enable Banking auth provider and config from settings.
///
/// Returns `(None, None)` if credentials are missing or the private key
/// cannot be read. The auth handle is for the OAuth redirect flow; the
/// config is for the `BankProviderFactory` to create data-fetching clients.
fn init_enable_banking(
    config: &budget_core::Config,
) -> (Option<Arc<EnableBankingAuth>>, Option<EnableBankingConfig>) {
    let Some((app_id, key_path)) = config
        .enable_banking_app_id
        .as_ref()
        .zip(config.enable_banking_private_key_path.as_ref())
    else {
        return (None, None);
    };

    match std::fs::read(key_path) {
        Ok(pem) => {
            let eb_config = EnableBankingConfig::new(app_id.clone(), pem);
            // clone() justified: EnableBankingConfig is small and we need two
            // independent owners (auth client and provider factory)
            let auth_client = EnableBankingClient::new(eb_config.clone());
            tracing::info!("Enable Banking provider configured");
            (
                Some(Arc::new(EnableBankingAuth::new(auth_client))),
                Some(eb_config),
            )
        }
        Err(e) => {
            tracing::warn!(path = %key_path, error = %e, "failed to read Enable Banking private key, provider disabled");
            (None, None)
        }
    }
}
