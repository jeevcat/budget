use std::path::{Path, PathBuf};

use axum::Json;
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use utoipa_axum::{router::OpenApiRouter, routes};
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
pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_accounts, create_account))
        .routes(routes!(delete_account, update_account))
        .routes(routes!(upload_cookies))
        .routes(routes!(account_status))
        .routes(routes!(trigger_sync))
        .routes(routes!(get_enrichment))
        .routes(routes!(list_matches))
}

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

#[derive(Deserialize, utoipa::ToSchema)]
struct CreateAccountRequest {
    label: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
struct UpdateAccountRequest {
    label: String,
}

#[derive(Serialize, utoipa::ToSchema)]
struct AccountStatus {
    account: AmazonAccount,
    cookies_valid: Option<bool>,
    cookies_expiry: Option<String>,
    cookies_days_remaining: Option<i64>,
    stats: Option<AmazonEnrichmentStats>,
}

#[derive(Serialize, utoipa::ToSchema)]
struct MatchedTransaction {
    bank_transaction_id: Uuid,
    orders: Vec<budget_amazon::AmazonOrder>,
}

#[derive(Deserialize, utoipa::ToSchema)]
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

impl utoipa::PartialSchema for CookiesInput {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        // Accepts either a JSON array of cookie objects or a raw string
        utoipa::openapi::schema::ObjectBuilder::new()
            .description(Some(
                "Pre-parsed JSON array of cookies or raw Netscape cookies.txt string",
            ))
            .into()
    }
}

impl utoipa::ToSchema for CookiesInput {}

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
#[utoipa::path(get, path = "/accounts", tag = "amazon", responses((status = 200, body = Vec<AmazonAccount>)), security(("bearer_token" = [])))]
async fn list_accounts(
    State(state): State<AppState>,
) -> Result<Json<Vec<AmazonAccount>>, AppError> {
    let accounts = state.db.list_amazon_accounts().await?;
    Ok(Json(accounts))
}

/// Create a new Amazon account.
#[utoipa::path(post, path = "/accounts", tag = "amazon", request_body = CreateAccountRequest, responses((status = 201, body = AmazonAccount)), security(("bearer_token" = [])))]
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
#[utoipa::path(delete, path = "/accounts/{id}", tag = "amazon", params(("id" = AmazonAccountId, Path, description = "Amazon account UUID")), responses((status = 204)), security(("bearer_token" = [])))]
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
#[utoipa::path(patch, path = "/accounts/{id}", tag = "amazon", params(("id" = AmazonAccountId, Path, description = "Amazon account UUID")), request_body = UpdateAccountRequest, responses((status = 200, body = AmazonAccount)), security(("bearer_token" = [])))]
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
#[utoipa::path(post, path = "/accounts/{id}/cookies", tag = "amazon", params(("id" = AmazonAccountId, Path, description = "Amazon account UUID")), request_body = CookiesPayload, responses((status = 200, body = serde_json::Value)), security(("bearer_token" = [])))]
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
#[utoipa::path(get, path = "/accounts/{id}/status", tag = "amazon", params(("id" = AmazonAccountId, Path, description = "Amazon account UUID")), responses((status = 200, body = AccountStatus)), security(("bearer_token" = [])))]
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
#[utoipa::path(post, path = "/accounts/{id}/sync", tag = "amazon", params(("id" = AmazonAccountId, Path, description = "Amazon account UUID")), responses((status = 202)), security(("bearer_token" = [])))]
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
#[utoipa::path(get, path = "/matches", tag = "amazon", responses((status = 200, body = Vec<MatchedTransaction>)), security(("bearer_token" = [])))]
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
#[utoipa::path(get, path = "/enrichment/{transaction_id}", tag = "amazon", params(("transaction_id" = Uuid, Path, description = "Bank transaction UUID")), responses((status = 200, body = MatchedTransaction)), security(("bearer_token" = [])))]
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
