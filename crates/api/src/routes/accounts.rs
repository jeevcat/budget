use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;

use budget_core::models::{Account, AccountId, AccountType, CurrencyCode};

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
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", get(get_by_id).patch(update_nickname))
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
    let account = Account {
        id: AccountId::new(),
        provider_account_id: body.provider_account_id,
        name: body.name,
        nickname: None,
        institution: body.institution,
        account_type: body.account_type,
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
#[derive(Deserialize)]
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
