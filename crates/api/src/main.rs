use apalis::prelude::*;
use apalis_sqlite::SqliteStorage;
use axum::{Json, Router, routing::get};
use sqlx::SqlitePool;
use tracing_subscriber::EnvFilter;

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

    let storage = SqliteStorage::new(&pool);

    let worker = WorkerBuilder::new("budget-no-op")
        .backend(storage)
        .build(budget_jobs::handle_noop_job);

    tracing::info!("job queue initialized");

    let app = Router::new().route("/health", get(health));

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", config.server_port)).await?;
    tracing::info!(port = config.server_port, "listening");

    tokio::select! {
        res = axum::serve(listener, app) => {
            if let Err(e) = res { tracing::error!(%e, "server error"); }
        }
        res = worker.run() => {
            if let Err(e) = res { tracing::error!(%e, "worker error"); }
        }
    }

    Ok(())
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok"}))
}
