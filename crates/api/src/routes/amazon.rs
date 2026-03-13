use std::path::{Path, PathBuf};

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use budget_amazon::{AmazonCookie, CookieStore};
use budget_core::models::{AmazonAccount, AmazonAccountId};
use budget_db::AmazonEnrichmentStats;
use budget_jobs::schedule_queries::{self, AccountType, RunStatus, ScheduleRun, TriggerReason};

use crate::routes::AppError;
use crate::state::AppState;

/// Build the Amazon sub-router.
///
/// Mounts:
/// - `GET /accounts` -- list Amazon accounts
/// - `POST /accounts` -- create account
/// - `DELETE /accounts/{id}` -- delete account
/// - `PATCH /accounts/{id}` -- update account label
/// - `POST /accounts/{id}/cookies` -- upload cookies for account
/// - `GET /accounts/{id}/status` -- cookie status + stats for account
/// - `POST /accounts/{id}/sync` -- trigger sync for account
/// - `GET /enrichment/{transaction_id}` -- Amazon enrichment for a bank transaction
/// - `GET /matches` -- list matched bank transactions with Amazon details
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/accounts", get(list_accounts).post(create_account))
        .route(
            "/accounts/{id}",
            axum::routing::delete(delete_account).patch(update_account),
        )
        .route("/accounts/{id}/cookies", post(upload_cookies))
        .route("/accounts/{id}/status", get(account_status))
        .route("/accounts/{id}/sync", post(trigger_sync))
        .route("/enrichment/{transaction_id}", get(get_enrichment))
        .route("/matches", get(list_matches))
}

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct CreateAccountRequest {
    label: String,
}

#[derive(Deserialize)]
struct UpdateAccountRequest {
    label: String,
}

#[derive(Serialize)]
struct AccountStatus {
    account: AmazonAccount,
    cookies_valid: Option<bool>,
    cookies_expiry: Option<String>,
    cookies_days_remaining: Option<i64>,
    stats: Option<AmazonEnrichmentStats>,
}

#[derive(Serialize)]
struct MatchedTransaction {
    bank_transaction_id: Uuid,
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
// Helpers
// ---------------------------------------------------------------------------

/// Resolve the cookies file path for a given account.
fn cookies_path_for(dir: &Path, account_id: AmazonAccountId) -> PathBuf {
    dir.join(format!("{account_id}.json"))
}

// ---------------------------------------------------------------------------
// Account CRUD handlers
// ---------------------------------------------------------------------------

/// List all Amazon accounts.
async fn list_accounts(
    State(state): State<AppState>,
) -> Result<Json<Vec<AmazonAccount>>, AppError> {
    let accounts = state.db.list_amazon_accounts().await?;
    Ok(Json(accounts))
}

/// Create a new Amazon account.
async fn create_account(
    State(state): State<AppState>,
    Json(body): Json<CreateAccountRequest>,
) -> Result<(StatusCode, Json<AmazonAccount>), AppError> {
    let account = AmazonAccount {
        id: AmazonAccountId::new(),
        label: body.label,
    };
    state.db.insert_amazon_account(&account).await?;
    Ok((StatusCode::CREATED, Json(account)))
}

/// Delete an Amazon account and all its data.
async fn delete_account(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<AmazonAccountId>,
) -> Result<StatusCode, AppError> {
    state.db.get_amazon_account(id).await?.ok_or_else(|| {
        AppError(
            StatusCode::NOT_FOUND,
            format!("amazon account {id} not found"),
        )
    })?;

    state.db.delete_amazon_account(id).await?;

    // Remove cookies file if it exists
    let path = cookies_path_for(&state.amazon_config.cookies_dir, id);
    let _ = std::fs::remove_file(&path);

    Ok(StatusCode::NO_CONTENT)
}

/// Update an Amazon account's label.
async fn update_account(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<AmazonAccountId>,
    Json(body): Json<UpdateAccountRequest>,
) -> Result<Json<AmazonAccount>, AppError> {
    state.db.get_amazon_account(id).await?.ok_or_else(|| {
        AppError(
            StatusCode::NOT_FOUND,
            format!("amazon account {id} not found"),
        )
    })?;

    state
        .db
        .update_amazon_account_label(id, &body.label)
        .await?;

    let updated = state.db.get_amazon_account(id).await?.ok_or_else(|| {
        AppError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "account disappeared".to_owned(),
        )
    })?;

    Ok(Json(updated))
}

// ---------------------------------------------------------------------------
// Per-account operations
// ---------------------------------------------------------------------------

/// Upload Amazon cookies for a specific account.
async fn upload_cookies(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<AmazonAccountId>,
    Json(payload): Json<CookiesPayload>,
) -> Result<Json<serde_json::Value>, AppError> {
    state.db.get_amazon_account(id).await?.ok_or_else(|| {
        AppError(
            StatusCode::NOT_FOUND,
            format!("amazon account {id} not found"),
        )
    })?;

    let cookies = match payload.cookies {
        CookiesInput::Parsed(c) => c,
        CookiesInput::Raw(text) => CookieStore::parse_cookies_auto(&text).map_err(|e| {
            AppError(
                StatusCode::BAD_REQUEST,
                format!("failed to parse cookies: {e}"),
            )
        })?,
    };

    let path = cookies_path_for(&state.amazon_config.cookies_dir, id);
    let store = CookieStore::from_cookies(cookies, path);

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

/// Get Amazon account status: cookie validity and enrichment stats.
async fn account_status(
    State(app): State<AppState>,
    AxumPath(id): AxumPath<AmazonAccountId>,
) -> Result<Json<AccountStatus>, AppError> {
    let account = app.db.get_amazon_account(id).await?.ok_or_else(|| {
        AppError(
            StatusCode::NOT_FOUND,
            format!("amazon account {id} not found"),
        )
    })?;

    let path = cookies_path_for(&app.amazon_config.cookies_dir, id);
    let (cookies_valid, cookies_expiry, cookies_days_remaining) = match CookieStore::load(&path) {
        Ok(store) => {
            let valid = !store.is_expired();
            let expiry = store.earliest_expiry();
            let days = expiry.map(|e| (e - chrono::Utc::now()).num_days());
            (Some(valid), expiry.map(|e| e.to_rfc3339()), days)
        }
        Err(_) => (None, None, None),
    };

    let stats = app.db.amazon_enrichment_stats(id).await.ok();

    Ok(Json(AccountStatus {
        account,
        cookies_valid,
        cookies_expiry,
        cookies_days_remaining,
        stats,
    }))
}

/// Enqueue an Amazon enrichment sync for a specific account.
///
/// Returns 202 Accepted immediately; the sync runs asynchronously via the job queue.
async fn trigger_sync(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<AmazonAccountId>,
) -> Result<StatusCode, AppError> {
    state.db.get_amazon_account(id).await?.ok_or_else(|| {
        AppError(
            StatusCode::NOT_FOUND,
            format!("amazon account {id} not found"),
        )
    })?;

    let run_id = uuid::Uuid::new_v4();
    let now = chrono::Utc::now();
    let account_uuid: uuid::Uuid = id.into();

    let run = ScheduleRun {
        id: run_id,
        account_id: account_uuid,
        account_type: AccountType::Amazon,
        status: RunStatus::Running,
        trigger_reason: TriggerReason::Manual,
        attempt: 1,
        started_at: Some(now),
        finished_at: None,
        next_run_at: None,
        error_message: None,
        created_at: now,
    };
    schedule_queries::insert_schedule_run(&state.apalis_pool, &run)
        .await
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    state
        .amazon_sync_storage
        .push(budget_jobs::AmazonSyncJob {
            account_id: id,
            schedule_run_id: Some(run_id.to_string()),
        })
        .await
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;

    Ok(StatusCode::ACCEPTED)
}

// ---------------------------------------------------------------------------
// Account-agnostic handlers
// ---------------------------------------------------------------------------

/// List bank transactions that have been matched to Amazon transactions.
async fn list_matches(
    State(state): State<AppState>,
) -> Result<Json<Vec<MatchedTransaction>>, AppError> {
    let matched_ids = state.db.get_matched_bank_transaction_ids().await?;

    let mut results = Vec::new();
    for id in matched_ids {
        if let Some(enrichment) = state.db.get_amazon_enrichment_for_transaction(id).await? {
            results.push(MatchedTransaction {
                bank_transaction_id: id,
                orders: enrichment.orders,
            });
        }
    }

    Ok(Json(results))
}

/// Get Amazon enrichment details for a specific bank transaction.
async fn get_enrichment(
    State(state): State<AppState>,
    AxumPath(transaction_id): AxumPath<Uuid>,
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
        orders: enrichment.orders,
    }))
}

/// Amazon configuration extracted from the app config.
#[derive(Clone)]
pub struct AmazonConfig {
    pub cookies_dir: PathBuf,
}
