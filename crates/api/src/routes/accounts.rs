use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use uuid::Uuid;

use budget_core::models::{Account, AccountId, AccountType};

use crate::routes::AppError;
use crate::state::AppState;

/// Request body for creating a new account.
#[derive(Deserialize)]
pub struct CreateAccount {
    /// Identifier from the bank provider (opaque string).
    pub provider_account_id: String,
    /// Human-readable account name.
    pub name: String,
    /// Financial institution name.
    pub institution: String,
    /// Account type as a lowercase string (e.g. "checking", "`credit_card`").
    pub account_type: String,
    /// ISO 4217 currency code.
    pub currency: String,
}

/// Build the accounts sub-router.
///
/// Mounts:
/// - `GET /` -- list all accounts
/// - `POST /` -- create a new account
/// - `GET /{id}` -- get a single account by ID
///
/// # Errors
///
/// Individual handlers return `AppError` on failure.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", get(get_by_id))
}

/// List all accounts.
///
/// # Errors
///
/// Returns `AppError` if the database query fails.
async fn list(State(state): State<AppState>) -> Result<Json<Vec<Account>>, AppError> {
    let accounts = state.db.list_accounts().await?;
    Ok(Json(accounts))
}

/// Create a new account from the request body.
///
/// Parses the `account_type` string into the `AccountType` enum and
/// generates a fresh `AccountId`.
///
/// # Errors
///
/// Returns `AppError` if the account type is invalid or the database write fails.
async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateAccount>,
) -> Result<(StatusCode, Json<Account>), AppError> {
    let account_type: AccountType = body
        .account_type
        .parse()
        .map_err(|e: budget_core::error::Error| AppError(StatusCode::BAD_REQUEST, e.to_string()))?;

    let account = Account {
        id: AccountId::new(),
        provider_account_id: body.provider_account_id,
        name: body.name,
        nickname: None,
        institution: body.institution,
        account_type,
        currency: body.currency,
        connection_id: None,
    };

    state.db.upsert_account(&account).await?;
    Ok((StatusCode::CREATED, Json(account)))
}

/// Get a single account by its UUID path parameter.
///
/// # Errors
///
/// Returns 400 if the ID is not a valid UUID, or 404 if the account
/// does not exist.
async fn get_by_id(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Account>, AppError> {
    let uuid =
        Uuid::parse_str(&id).map_err(|e| AppError(StatusCode::BAD_REQUEST, e.to_string()))?;
    let account_id = AccountId::from_uuid(uuid);

    let account = state
        .db
        .get_account(account_id)
        .await?
        .ok_or_else(|| AppError(StatusCode::NOT_FOUND, format!("account {id} not found")))?;

    Ok(Json(account))
}
