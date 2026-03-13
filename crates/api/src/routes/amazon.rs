use std::path::PathBuf;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use budget_amazon::{AmazonCookie, CookieStore};
use budget_db::AmazonEnrichmentStats;

use crate::routes::AppError;
use crate::state::AppState;

/// Build the Amazon sub-router.
///
/// Mounts:
/// - `POST /cookies` -- upload cookie JSON, validate, save to disk
/// - `GET /status` -- configuration status, cookie validity, match stats
/// - `POST /sync` -- trigger Amazon enrichment job
/// - `GET /matches` -- list matched bank transactions with Amazon details
/// - `GET /enrichment/{transaction_id}` -- Amazon enrichment for a bank transaction
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/cookies", post(upload_cookies))
        .route("/status", get(status))
        .route("/sync", post(trigger_sync))
        .route("/matches", get(list_matches))
        .route("/enrichment/{transaction_id}", get(get_enrichment))
}

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct AmazonStatus {
    cookies_valid: Option<bool>,
    cookies_expiry: Option<String>,
    cookies_days_remaining: Option<i64>,
    stats: Option<AmazonEnrichmentStats>,
}

#[derive(Serialize)]
struct MatchedTransaction {
    bank_transaction_id: Uuid,
    confidence: String,
    orders: Vec<budget_amazon::AmazonOrder>,
}

#[derive(Deserialize)]
struct CookiesPayload {
    cookies: CookiesInput,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum CookiesInput {
    /// Pre-parsed JSON array of cookie objects.
    Parsed(Vec<AmazonCookie>),
    /// Raw text — auto-detected as JSON array or Netscape cookies.txt.
    Raw(String),
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Upload Amazon cookies.
///
/// Accepts a JSON body with `cookies` as either:
/// - a JSON array of cookie objects
/// - a raw string in JSON or Netscape cookies.txt format
///
/// Validates that auth tokens are present and not expired, then saves to disk.
///
/// # Errors
///
/// Returns `AppError` if Amazon is not configured, cookies are invalid, or
/// the file cannot be written.
async fn upload_cookies(
    State(state): State<AppState>,
    Json(payload): Json<CookiesPayload>,
) -> Result<Json<serde_json::Value>, AppError> {
    let cookies = match payload.cookies {
        CookiesInput::Parsed(c) => c,
        CookiesInput::Raw(text) => CookieStore::parse_cookies_auto(&text).map_err(|e| {
            AppError(
                StatusCode::BAD_REQUEST,
                format!("failed to parse cookies: {e}"),
            )
        })?,
    };

    let store = CookieStore::from_cookies(cookies, state.amazon_config.cookies_path.clone());

    if store.is_expired() {
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            "uploaded cookies are expired or missing auth tokens".to_owned(),
        ));
    }

    store.save().map_err(|e| {
        tracing::error!(error = %e, "failed to save Amazon cookies");
        AppError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to save cookies".to_owned(),
        )
    })?;

    let expiry = store.earliest_expiry();
    Ok(Json(serde_json::json!({
        "saved": true,
        "expires": expiry.map(|e| e.to_rfc3339()),
    })))
}

/// Get Amazon enrichment status.
///
/// # Errors
///
/// Returns `AppError` on database failure.
async fn status(State(app): State<AppState>) -> Result<Json<AmazonStatus>, AppError> {
    let (cookies_valid, cookies_expiry, cookies_days_remaining) =
        match CookieStore::load(&app.amazon_config.cookies_path) {
            Ok(store) => {
                let valid = !store.is_expired();
                let expiry = store.earliest_expiry();
                let days = expiry.map(|e| (e - chrono::Utc::now()).num_days());
                (Some(valid), expiry.map(|e| e.to_rfc3339()), days)
            }
            Err(_) => (None, None, None),
        };

    let stats = app.db.amazon_enrichment_stats().await.ok();

    Ok(Json(AmazonStatus {
        cookies_valid,
        cookies_expiry,
        cookies_days_remaining,
        stats,
    }))
}

/// Trigger an Amazon enrichment sync.
///
/// Runs the enrichment job synchronously and returns the result.
/// Returns 202 Accepted with the enrichment summary.
///
/// # Errors
///
/// Returns `AppError` if Amazon is not configured or the job fails.
async fn trigger_sync(State(state): State<AppState>) -> Result<Json<serde_json::Value>, AppError> {
    let enrich_config = budget_jobs::enrich::AmazonEnrichConfig {
        cookies_path: state.amazon_config.cookies_path.clone(),
    };

    let result = budget_jobs::enrich::run_amazon_enrich(&state.db, &enrich_config)
        .await
        .map_err(|e| {
            AppError(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("enrichment failed: {e}"),
            )
        })?;

    Ok(Json(serde_json::json!({
        "transactions_fetched": result.transactions_fetched,
        "orders_fetched": result.orders_fetched,
        "matches_created": result.matches_created,
    })))
}

/// List bank transactions that have been matched to Amazon transactions.
///
/// # Errors
///
/// Returns `AppError` on database failure.
async fn list_matches(
    State(state): State<AppState>,
) -> Result<Json<Vec<MatchedTransaction>>, AppError> {
    let matched_ids = state.db.get_matched_bank_transaction_ids().await?;

    let mut results = Vec::new();
    for id in matched_ids {
        if let Some(enrichment) = state.db.get_amazon_enrichment_for_transaction(id).await? {
            results.push(MatchedTransaction {
                bank_transaction_id: id,
                confidence: enrichment.confidence,
                orders: enrichment.orders,
            });
        }
    }

    Ok(Json(results))
}

/// Get Amazon enrichment details for a specific bank transaction.
///
/// # Errors
///
/// Returns `AppError` if the transaction has no Amazon match or on database failure.
async fn get_enrichment(
    State(state): State<AppState>,
    Path(transaction_id): Path<Uuid>,
) -> Result<Json<MatchedTransaction>, AppError> {
    let enrichment = state
        .db
        .get_amazon_enrichment_for_transaction(transaction_id)
        .await?
        .ok_or_else(|| {
            AppError(
                StatusCode::NOT_FOUND,
                "no Amazon enrichment for this transaction".to_owned(),
            )
        })?;

    Ok(Json(MatchedTransaction {
        bank_transaction_id: transaction_id,
        confidence: enrichment.confidence,
        orders: enrichment.orders,
    }))
}

/// Amazon configuration extracted from the app config.
#[derive(Clone)]
pub struct AmazonConfig {
    pub cookies_path: PathBuf,
}
