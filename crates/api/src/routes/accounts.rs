use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use utoipa_axum::{router::OpenApiRouter, routes};

use budget_core::models::{
    Account, AccountId, AccountOrigin, AccountType, BalanceSnapshot, BalanceSnapshotId,
    CurrencyCode,
};
use budget_core::projection::{self, ForecastPoint, NetWorthPoint};

use tracing::{debug, warn};

use crate::routes::AppError;
use crate::state::AppState;

/// Request body for creating a new account.
#[derive(Deserialize, utoipa::ToSchema)]
pub struct CreateAccount {
    /// Identifier from the bank provider (opaque string).
    pub provider_account_id: String,
    /// Human-readable account name.
    pub name: String,
    /// Financial institution name.
    pub institution: String,
    /// Account type (parsed from `snake_case` at deserialization).
    pub account_type: AccountType,
    /// ISO 4217 currency code.
    pub currency: CurrencyCode,
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
pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list, create))
        .routes(routes!(net_worth))
        .routes(routes!(net_worth_projection))
        .routes(routes!(get_by_id, update_nickname))
        .routes(routes!(list_balances, create_balance))
}

/// List all accounts.
///
/// # Errors
///
/// Returns `AppError` if the database query fails.
#[utoipa::path(get, path = "/", tag = "accounts", responses((status = 200, body = Vec<Account>)), security(("bearer_token" = [])))]
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
#[utoipa::path(post, path = "/", tag = "accounts", request_body = CreateAccount, responses((status = 201, body = Account)), security(("bearer_token" = [])))]
async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateAccount>,
) -> Result<(StatusCode, Json<Account>), AppError> {
    let account = Account {
        id: AccountId::new(),
        provider_account_id: body.provider_account_id,
        name: body.name,
        nickname: None,
        institution: body.institution,
        account_type: body.account_type,
        currency: body.currency,
        origin: AccountOrigin::Manual,
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
#[utoipa::path(get, path = "/{id}", tag = "accounts", params(("id" = AccountId, Path, description = "Account UUID")), responses((status = 200, body = Account)), security(("bearer_token" = [])))]
async fn get_by_id(
    State(state): State<AppState>,
    Path(id): Path<AccountId>,
) -> Result<Json<Account>, AppError> {
    let account = state
        .db
        .get_account(id)
        .await?
        .ok_or_else(|| AppError(StatusCode::NOT_FOUND, format!("account {id} not found")))?;

    Ok(Json(account))
}

/// Request body for updating an account nickname.
#[derive(Deserialize, utoipa::ToSchema)]
pub struct UpdateNickname {
    /// New nickname, or null to clear it.
    pub nickname: Option<String>,
}

/// Set or clear the user-defined nickname for an account.
///
/// # Errors
///
/// Returns 400 if the ID is not a valid UUID, or 404 if the account
/// does not exist.
#[utoipa::path(patch, path = "/{id}", tag = "accounts", params(("id" = AccountId, Path, description = "Account UUID")), request_body = UpdateNickname, responses((status = 200, body = Account)), security(("bearer_token" = [])))]
async fn update_nickname(
    State(state): State<AppState>,
    Path(id): Path<AccountId>,
    Json(body): Json<UpdateNickname>,
) -> Result<Json<Account>, AppError> {
    // Verify the account exists
    state
        .db
        .get_account(id)
        .await?
        .ok_or_else(|| AppError(StatusCode::NOT_FOUND, format!("account {id} not found")))?;

    state
        .db
        .update_account_nickname(id, body.nickname.as_deref())
        .await?;

    let updated = state.db.get_account(id).await?.ok_or_else(|| {
        AppError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "account disappeared".to_owned(),
        )
    })?;

    Ok(Json(updated))
}

// ---------------------------------------------------------------------------
// Net worth
// ---------------------------------------------------------------------------

/// Per-account balance entry in the net worth response.
#[derive(Serialize, utoipa::ToSchema)]
struct AccountBalance {
    account_id: AccountId,
    account_name: String,
    account_type: AccountType,
    #[schema(value_type = String)]
    current: Decimal,
    currency: CurrencyCode,
    snapshot_at: DateTime<Utc>,
}

/// Aggregated net worth across all accounts.
#[derive(Serialize, utoipa::ToSchema)]
struct NetWorth {
    #[schema(value_type = String)]
    total: Decimal,
    currency: CurrencyCode,
    accounts: Vec<AccountBalance>,
}

/// Get current net worth: latest balance per account, summed.
///
/// Only includes accounts that have at least one balance snapshot.
/// All balances are assumed to be in the same currency (no FX conversion).
///
/// # Errors
///
/// Returns `AppError` if the database query fails.
#[utoipa::path(get, path = "/net-worth", tag = "accounts", responses((status = 200, body = NetWorth)), security(("bearer_token" = [])))]
async fn net_worth(State(state): State<AppState>) -> Result<Json<NetWorth>, AppError> {
    let snapshots = state.db.get_latest_balance_per_account().await?;
    let accounts_by_id: std::collections::HashMap<AccountId, Account> = state
        .db
        .list_accounts()
        .await?
        .into_iter()
        .map(|a| (a.id, a))
        .collect();

    let mut total = Decimal::ZERO;
    let mut accounts = Vec::with_capacity(snapshots.len());

    for s in snapshots {
        total += s.current;
        if let Some(account) = accounts_by_id.get(&s.account_id) {
            accounts.push(AccountBalance {
                account_id: s.account_id,
                account_name: account
                    .nickname
                    .clone()
                    .unwrap_or_else(|| account.name.clone()),
                account_type: account.account_type,
                current: s.current,
                currency: s.currency,
                snapshot_at: s.snapshot_at,
            });
        }
    }

    // Sort by balance descending for a natural presentation
    accounts.sort_by(|a, b| b.current.cmp(&a.current));

    let currency = accounts
        .first()
        .map_or_else(|| "EUR".parse().expect("valid"), |a| a.currency.clone());

    Ok(Json(NetWorth {
        total,
        currency,
        accounts,
    }))
}

// ---------------------------------------------------------------------------
// Net worth projection
// ---------------------------------------------------------------------------

/// Query parameters for net worth projection.
#[derive(Deserialize, utoipa::IntoParams)]
pub struct ProjectionParams {
    /// Forecast horizon in months (default 12, max 24).
    pub months: Option<u32>,
    /// Confidence interval width, 0.0–1.0 (default 0.8).
    pub interval_width: Option<f64>,
}

/// Response body for net worth projection.
#[derive(Serialize, utoipa::ToSchema)]
struct ProjectionResponse {
    history: Vec<NetWorthPoint>,
    forecast: Vec<ForecastPoint>,
    message: Option<String>,
}

/// Project net worth forward using Prophet.
///
/// Returns historical daily series plus forecasted values with confidence
/// bands. When insufficient data exists, returns empty arrays with an
/// explanatory message rather than an error status.
///
/// # Errors
///
/// Returns `AppError` if the database query fails.
#[utoipa::path(get, path = "/net-worth/projection", tag = "accounts", params(ProjectionParams), responses((status = 200, body = ProjectionResponse)), security(("bearer_token" = [])))]
async fn net_worth_projection(
    State(state): State<AppState>,
    Query(params): Query<ProjectionParams>,
) -> Result<Json<ProjectionResponse>, AppError> {
    let months = params.months.unwrap_or(12).min(24);
    let interval_width = params.interval_width.unwrap_or(0.8).clamp(0.0, 1.0);

    let snapshots = state.db.list_all_balance_snapshots().await?;
    debug!(
        snapshot_count = snapshots.len(),
        "running net worth projection on blocking thread"
    );

    let result = tokio::task::spawn_blocking(move || {
        projection::project_net_worth(&snapshots, months, interval_width)
    })
    .await
    .map_err(|e| {
        warn!("projection task panicked: {e}");
        AppError(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("projection task failed: {e}"),
        )
    })?;

    match result {
        Ok(proj) => {
            debug!(
                history_len = proj.history.len(),
                forecast_len = proj.forecast.len(),
                "projection complete"
            );
            Ok(Json(ProjectionResponse {
                history: proj.history,
                forecast: proj.forecast,
                message: None,
            }))
        }
        Err(e) => {
            debug!("projection unavailable: {e}");
            Ok(Json(ProjectionResponse {
                history: Vec::new(),
                forecast: Vec::new(),
                message: Some(e.to_string()),
            }))
        }
    }
}

// ---------------------------------------------------------------------------
// Balance snapshot endpoints
// ---------------------------------------------------------------------------

/// Request body for creating a manual balance snapshot.
#[derive(Deserialize, utoipa::ToSchema)]
pub struct CreateBalanceSnapshot {
    #[schema(value_type = String)]
    pub current: Decimal,
    #[schema(value_type = Option<String>)]
    pub available: Option<Decimal>,
    pub currency: Option<CurrencyCode>,
    pub snapshot_at: Option<DateTime<Utc>>,
}

/// Query parameters for listing balance snapshots.
#[derive(Deserialize, utoipa::IntoParams)]
pub struct ListBalancesParams {
    pub limit: Option<i64>,
}

/// Create a manual balance snapshot for an account.
///
/// # Errors
///
/// Returns 404 if the account does not exist.
#[utoipa::path(post, path = "/{id}/balances", tag = "accounts", params(("id" = AccountId, Path, description = "Account UUID")), request_body = CreateBalanceSnapshot, responses((status = 201, body = BalanceSnapshot)), security(("bearer_token" = [])))]
async fn create_balance(
    State(state): State<AppState>,
    Path(id): Path<AccountId>,
    Json(body): Json<CreateBalanceSnapshot>,
) -> Result<(StatusCode, Json<BalanceSnapshot>), AppError> {
    let account = state
        .db
        .get_account(id)
        .await?
        .ok_or_else(|| AppError(StatusCode::NOT_FOUND, format!("account {id} not found")))?;

    let snapshot = BalanceSnapshot {
        id: BalanceSnapshotId::new(),
        account_id: account.id,
        current: body.current,
        available: body.available,
        currency: body.currency.unwrap_or(account.currency),
        snapshot_at: body.snapshot_at.unwrap_or_else(Utc::now),
    };

    state.db.insert_balance_snapshot(&snapshot).await?;
    Ok((StatusCode::CREATED, Json(snapshot)))
}

/// List balance snapshots for an account, newest first.
///
/// # Errors
///
/// Returns `AppError` if the database query fails.
#[utoipa::path(get, path = "/{id}/balances", tag = "accounts", params(("id" = AccountId, Path, description = "Account UUID"), ListBalancesParams), responses((status = 200, body = Vec<BalanceSnapshot>)), security(("bearer_token" = [])))]
async fn list_balances(
    State(state): State<AppState>,
    Path(id): Path<AccountId>,
    Query(params): Query<ListBalancesParams>,
) -> Result<Json<Vec<BalanceSnapshot>>, AppError> {
    let snapshots = state.db.list_balance_snapshots(id, params.limit).await?;
    Ok(Json(snapshots))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_create_account() {
        let json = r#"{
            "provider_account_id": "abc123",
            "name": "Main Checking",
            "institution": "Bank of Test",
            "account_type": "checking",
            "currency": "EUR"
        }"#;
        let acc: CreateAccount = serde_json::from_str(json).unwrap();
        assert_eq!(acc.account_type, AccountType::Checking);
    }

    #[test]
    fn deserialize_create_account_credit_card() {
        let json = r#"{
            "provider_account_id": "x",
            "name": "Visa",
            "institution": "Bank",
            "account_type": "credit_card",
            "currency": "USD"
        }"#;
        let acc: CreateAccount = serde_json::from_str(json).unwrap();
        assert_eq!(acc.account_type, AccountType::CreditCard);
    }

    #[test]
    fn deserialize_create_account_rejects_invalid_type() {
        let json = r#"{
            "provider_account_id": "x",
            "name": "Test",
            "institution": "Bank",
            "account_type": "invalid_type",
            "currency": "EUR"
        }"#;
        assert!(serde_json::from_str::<CreateAccount>(json).is_err());
    }

    #[test]
    fn deserialize_all_account_types() {
        for (name, expected) in [
            ("checking", AccountType::Checking),
            ("savings", AccountType::Savings),
            ("credit_card", AccountType::CreditCard),
            ("investment", AccountType::Investment),
            ("loan", AccountType::Loan),
            ("other", AccountType::Other),
        ] {
            let json = format!(
                r#"{{"provider_account_id":"x","name":"t","institution":"b","account_type":"{name}","currency":"EUR"}}"#,
            );
            let acc: CreateAccount = serde_json::from_str(&json).unwrap();
            assert_eq!(acc.account_type, expected);
        }
    }
}
