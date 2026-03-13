//! HTTP server binary: configuration, routing, and process orchestration.
//!
//! Wires together all crates: loads config, connects to the database, constructs
//! providers, starts the axum server and apalis workers in parallel. Route handlers
//! live in the `api` library crate (`src/routes/`).

use std::sync::Arc;

use axum::middleware;
use axum::routing::get;
use axum::{Json, Router};
use budget_core::db::Db;
use budget_core::models::Host;
use budget_jobs::{ApalisPool, BankProviderFactory, JobStorage, PipelineStorage};
use budget_providers::{
    EnableBankingAuth, EnableBankingClient, EnableBankingConfig, MockBankProvider,
};
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

use api::auth;
use api::routes;
use api::state::AppState;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if let Some(cmd) = std::env::args().nth(1) {
        return dispatch_subcommand(&cmd);
    }

    let config = budget_core::load_config()?;
    init_tracing(&config);
    tracing::info!(port = config.server_port, db = %config.database_url, "starting budget server");
    if config.secret_key.as_ref().is_empty() {
        tracing::warn!("no secret_key configured — all API requests will be rejected");
    }

    let db = Db::connect(config.database_url.as_ref()).await?;
    let apalis_pool = ApalisPool::connect(config.database_url.as_ref()).await?;

    run_migrations(&db, &apalis_pool).await?;
    tracing::info!("migrations applied");

    // Provider wrappers for apalis Data injection
    let (enable_banking_auth, eb_config) = init_enable_banking(&config);
    let bank_factory = BankProviderFactory::new(eb_config)
        .with_fallback(budget_jobs::BankClient::new(MockBankProvider::new()));
    let llm = budget_jobs::init_llm_provider(&config);

    let state = AppState {
        db: db.clone(),
        secret_key: config.secret_key,
        sync_storage: JobStorage::new(&apalis_pool),
        categorize_storage: JobStorage::new(&apalis_pool),
        correlate_storage: JobStorage::new(&apalis_pool),
        pipeline_storage: PipelineStorage::new(&apalis_pool),
        apalis_pool: apalis_pool.clone(),
        enable_banking_auth,
        // clone() justified: LlmClient wraps an Arc, workers also need their own handle
        llm: llm.clone(),
        expected_salary_count: config.expected_salary_count,
        host: config.host.map_or_else(
            || format!("http://localhost:{}", config.server_port),
            String::from,
        ),
    };

    let frontend_dir = config.frontend_dir.map_or_else(
        || std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../frontend/dist"),
        std::path::PathBuf::from,
    );
    let app = build_router(state, &frontend_dir);
    let workers = budget_jobs::run_workers(&db, &apalis_pool, bank_factory, llm);
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", config.server_port)).await?;
    tracing::info!(port = config.server_port, "listening");

    tokio::select! {
        res = axum::serve(listener, app) => {
            if let Err(e) = res { tracing::error!(%e, "server error"); }
        }
        res = workers => {
            if let Err(e) = res { tracing::error!(%e, "worker error"); }
        }
        () = budget_jobs::reclaim_stale_jobs_loop(&apalis_pool) => {}
        () = budget_jobs::purge_finished_jobs_loop(&apalis_pool) => {}
        () = budget_jobs::scheduler::run_scheduler(&db, &apalis_pool) => {}
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
                    let host = config.host.as_ref().map_or_else(
                        || format!("http://localhost:{} (default)", config.server_port),
                        Host::to_string,
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

/// Run apalis and domain migrations.
async fn run_migrations(
    db: &Db,
    apalis_pool: &ApalisPool,
) -> Result<(), Box<dyn std::error::Error>> {
    budget_jobs::run_migrations(apalis_pool).await?;
    db.run_migrations().await?;
    Ok(())
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

/// Build the axum router with all API routes, auth middleware, and static file serving.
fn build_router(state: AppState, frontend_dir: &std::path::Path) -> Router {
    let api_routes = Router::new()
        .nest(
            "/accounts",
            routes::accounts::router().merge(routes::import::router()),
        )
        .nest("/transactions", routes::transactions::router())
        .nest("/categories", routes::categories::router())
        .nest("/rules", routes::rules::router())
        .nest("/budgets", routes::budgets::router())
        .nest("/jobs", routes::jobs::router())
        .nest("/connections", routes::connections::router())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_bearer_token,
        ));

    let static_service = ServeDir::new(frontend_dir)
        .append_index_html_on_directories(true)
        .fallback(tower_http::services::ServeFile::new(
            frontend_dir.join("index.html"),
        ));

    Router::new()
        .route("/health", get(health))
        .merge(routes::connections::callback_router())
        .nest("/api", auth::router().merge(api_routes))
        .fallback_service(static_service)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
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
