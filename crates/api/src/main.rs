use std::sync::Arc;

use apalis::prelude::*;
use apalis_sqlite::SqliteStorage;
use apalis_workflow::Workflow;
use axum::middleware;
use axum::routing::get;
use axum::{Json, Router};
use budget_core::db::Db;
use budget_jobs::{
    BankProviderFactory, BudgetRecomputeJob, CategorizeJob, CategorizeTransactionJob, CorrelateJob,
    CorrelateTransactionJob, LlmClient, NoOpJob, SyncJob, pipeline,
};
use budget_providers::{
    EnableBankingAuth, EnableBankingClient, EnableBankingConfig, GeminiProvider, MockBankProvider,
    MockLlmProvider,
};
use sqlx::SqlitePool;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

use api::auth;
use api::routes;
use api::state::{AppState, JobStorage, PipelineStorage};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if let Some(cmd) = std::env::args().nth(1) {
        return dispatch_subcommand(&cmd);
    }

    let config = budget_core::load_config()?;
    init_tracing(&config);
    tracing::info!(port = config.server_port, db = %config.database_url, "starting budget server");

    let db = Db::connect(&config.database_url).await?;

    run_migrations(&db).await?;
    tracing::info!("migrations applied");

    // Provider wrappers for apalis Data injection
    let (enable_banking_auth, eb_config) = init_enable_banking(&config);
    let bank_factory = BankProviderFactory::new(eb_config)
        .with_fallback(budget_jobs::BankClient::new(MockBankProvider::new()));
    let llm = init_llm_provider(&config);

    let pool = db.pool();
    let state = AppState {
        db: db.clone(),
        secret_key: config.secret_key,
        sync_storage: JobStorage::new(pool),
        categorize_storage: JobStorage::new(pool),
        correlate_storage: JobStorage::new(pool),
        recompute_storage: JobStorage::new(pool),
        pipeline_storage: PipelineStorage::new(pool),
        enable_banking_auth,
        host: config
            .host
            .unwrap_or_else(|| format!("http://localhost:{}", config.server_port)),
    };

    let frontend_dir = config.frontend_dir.map_or_else(
        || std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../frontend"),
        std::path::PathBuf::from,
    );
    let app = build_router(state, &frontend_dir);
    let workers = build_workers(&db, bank_factory, llm);
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", config.server_port)).await?;
    tracing::info!(port = config.server_port, "listening");

    tokio::select! {
        res = axum::serve(listener, app) => {
            if let Err(e) = res { tracing::error!(%e, "server error"); }
        }
        res = workers => {
            if let Err(e) = res { tracing::error!(%e, "worker error"); }
        }
        // clone() justified: SqlitePool is Arc-based
        () = reclaim_stale_jobs_loop(pool.clone()) => {}
    }

    Ok(())
}

fn dispatch_subcommand(cmd: &str) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        "config" => {
            let config_path = budget_core::config_path()?;
            println!(
                "config: {} {}",
                config_path.display(),
                if config_path.exists() {
                    ""
                } else {
                    "(not found)"
                }
            );

            match budget_core::load_config() {
                Ok(config) => {
                    let host = config.host.as_deref().map_or_else(
                        || format!("http://localhost:{} (default)", config.server_port),
                        str::to_owned,
                    );
                    println!("host:     {host}");
                    println!(
                        "callback: {}/api/connections/callback",
                        host.trim_end_matches(" (default)")
                    );
                    if let Some(ref log_path) = config.log_path {
                        let exists = std::path::Path::new(log_path).exists();
                        println!(
                            "log:      {log_path} {}",
                            if exists { "" } else { "(not yet created)" }
                        );
                    } else {
                        println!("log:      (not configured — logs go to stderr only)");
                    }
                }
                Err(e) => eprintln!("failed to load config: {e}"),
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

/// Run domain migrations (via `Db`), apalis migrations, and reset stale jobs.
///
/// Both migrators share the `_sqlx_migrations` table.  Each must tolerate
/// the other's entries (`ignore_missing`) so that restarts and incremental
/// migrations work on persistent databases.
async fn run_migrations(db: &Db) -> Result<(), Box<dyn std::error::Error>> {
    let pool = db.pool();

    // PRAGMAs + domain migrations
    db.run_migrations().await?;

    let mut apalis_migrator = SqliteStorage::migrations();
    apalis_migrator.set_ignore_missing(true);
    apalis_migrator.run(pool).await?;

    // No workers are active yet, so any "Running" jobs are stale locks from
    // a previous process. Reset them so workers pick them up immediately.
    let reset = sqlx::query(
        "UPDATE Jobs SET status = 'Pending', lock_by = NULL, lock_at = NULL WHERE status = 'Running'",
    )
    .execute(pool)
    .await?;
    if reset.rows_affected() > 0 {
        tracing::info!(count = reset.rows_affected(), "reset stale running jobs");
    }

    Ok(())
}

/// Periodically reset jobs stuck in `Running` with a stale lock.
///
/// A lock older than 5 minutes indicates the worker died without completing.
/// Resetting to `Pending` lets another worker pick the job up.
async fn reclaim_stale_jobs_loop(pool: SqlitePool) {
    const STALE_SECONDS: i64 = 300; // 5 minutes
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
    loop {
        interval.tick().await;
        let cutoff = chrono::Utc::now().timestamp() - STALE_SECONDS;
        match sqlx::query(
            "UPDATE Jobs SET status = 'Pending', lock_by = NULL, lock_at = NULL \
             WHERE status = 'Running' AND lock_at < ?",
        )
        .bind(cutoff)
        .execute(&pool)
        .await
        {
            Ok(res) if res.rows_affected() > 0 => {
                tracing::info!(count = res.rows_affected(), "reclaimed stale jobs");
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed to reclaim stale jobs");
            }
            _ => {}
        }
    }
}

/// Health check endpoint (unauthenticated).
async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok"}))
}

/// Set up tracing with stderr output and an optional log file.
fn init_tracing(config: &budget_core::Config) {
    let default_filter = "budget=debug,tower_http=debug,info";
    let stderr_layer = tracing_subscriber::fmt::layer().with_filter(
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter)),
    );

    let file_layer = config.log_path.as_ref().and_then(|path| {
        let parent = std::path::Path::new(path).parent()?;
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!("failed to create log directory {}: {e}", parent.display());
            return None;
        }
        match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            Ok(file) => Some(
                tracing_subscriber::fmt::layer()
                    .with_ansi(false)
                    .with_writer(file)
                    .with_filter(EnvFilter::new(default_filter)),
            ),
            Err(e) => {
                eprintln!("failed to open log file {path}: {e}");
                None
            }
        }
    });

    tracing_subscriber::registry()
        .with(stderr_layer)
        .with(file_layer)
        .init();
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

/// Build the axum router with all API routes, auth middleware, and static file serving.
fn build_router(state: AppState, frontend_dir: &std::path::Path) -> Router {
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

    Router::new()
        .route("/health", get(health))
        .merge(routes::connections::callback_router())
        .nest("/api", api_routes)
        .fallback_service(
            ServeDir::new(frontend_dir)
                .append_index_html_on_directories(true)
                .fallback(tower_http::services::ServeFile::new(
                    frontend_dir.join("index.html"),
                )),
        )
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Create and run all apalis workers (individual jobs + pipeline workflow).
///
/// Returns a future that resolves when any worker errors out.
async fn build_workers(
    db: &Db,
    bank_factory: BankProviderFactory,
    llm: LlmClient,
) -> Result<(), Box<dyn std::error::Error>> {
    let pool = db.pool();

    // Pipeline workflow: sync -> categorize fan-out -> correlate fan-out -> recompute
    let pipeline_backend = SqliteStorage::<Vec<u8>, _, _>::new(pool);
    let pipeline_workflow = workflow_for(&pipeline_backend)
        .and_then(pipeline::step_sync)
        .and_then(pipeline::step_categorize)
        .and_then(pipeline::step_correlate)
        .and_then(pipeline::step_recompute);
    let pipeline_worker = WorkerBuilder::new("budget-pipeline")
        .backend(pipeline_backend)
        .data(db.clone())
        .data(bank_factory.clone())
        .build(pipeline_workflow);

    // clone() on db is justified: Db wraps an Arc-based SqlitePool
    let sync_worker = WorkerBuilder::new("budget-sync")
        .backend(SqliteStorage::<SyncJob, _, _>::new(pool))
        .data(db.clone())
        .data(bank_factory)
        .build(budget_jobs::handle_sync_job);

    // Fan-out handlers: apply rules, enqueue per-transaction LLM jobs
    let categorize_worker = WorkerBuilder::new("budget-categorize")
        .backend(SqliteStorage::<CategorizeJob, _, _>::new(pool))
        .data(db.clone())
        .build(budget_jobs::handle_categorize_job);

    let correlate_worker = WorkerBuilder::new("budget-correlate")
        .backend(SqliteStorage::<CorrelateJob, _, _>::new(pool))
        .data(db.clone())
        .build(budget_jobs::handle_correlate_job);

    // Per-transaction handlers: call LLM for individual transactions
    let categorize_txn_worker = WorkerBuilder::new("budget-categorize-txn")
        .backend(SqliteStorage::<CategorizeTransactionJob, _, _>::new(pool))
        .data(db.clone())
        .data(llm.clone())
        .build(budget_jobs::handle_categorize_transaction_job);

    let correlate_txn_worker = WorkerBuilder::new("budget-correlate-txn")
        .backend(SqliteStorage::<CorrelateTransactionJob, _, _>::new(pool))
        .data(db.clone())
        .data(llm)
        .build(budget_jobs::handle_correlate_transaction_job);

    let recompute_worker = WorkerBuilder::new("budget-recompute")
        .backend(SqliteStorage::<BudgetRecomputeJob, _, _>::new(pool))
        .data(db.clone())
        .build(budget_jobs::handle_recompute_job);

    let noop_worker = WorkerBuilder::new("budget-no-op")
        .backend(SqliteStorage::<NoOpJob, _, _>::new(pool))
        .build(budget_jobs::handle_noop_job);

    tracing::info!("job queue initialized");

    tokio::select! {
        res = sync_worker.run() => {
            if let Err(e) = res { tracing::error!(%e, "sync worker error"); }
        }
        res = categorize_worker.run() => {
            if let Err(e) = res { tracing::error!(%e, "categorize worker error"); }
        }
        res = categorize_txn_worker.run() => {
            if let Err(e) = res { tracing::error!(%e, "categorize-txn worker error"); }
        }
        res = correlate_worker.run() => {
            if let Err(e) = res { tracing::error!(%e, "correlate worker error"); }
        }
        res = correlate_txn_worker.run() => {
            if let Err(e) = res { tracing::error!(%e, "correlate-txn worker error"); }
        }
        res = recompute_worker.run() => {
            if let Err(e) = res { tracing::error!(%e, "recompute worker error"); }
        }
        res = noop_worker.run() => {
            if let Err(e) = res { tracing::error!(%e, "noop worker error"); }
        }
        res = pipeline_worker.run() => {
            if let Err(e) = res { tracing::error!(%e, "pipeline worker error"); }
        }
    }

    Ok(())
}

/// Create a [`Workflow`] whose Backend type parameter is inferred from a
/// reference to the concrete backend. This avoids naming the full
/// `SqliteStorage<Vec<u8>, JsonCodec<Vec<u8>>, SqliteFetcher>` type.
fn workflow_for<T, B>(_backend: &B) -> Workflow<T, T, B> {
    Workflow::new("full-sync-pipeline")
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
