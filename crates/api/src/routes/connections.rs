use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{delete, get};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use budget_core::db;
use budget_core::models::{
    Account, AccountId, AccountType, Connection, ConnectionId, ConnectionStatus,
};
use budget_providers::{AspspEntry, EnableBankingAuth};

use crate::routes::AppError;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn require_enable_banking(state: &AppState) -> Result<(&Arc<EnableBankingAuth>, &str), AppError> {
    let auth = state.enable_banking_auth.as_ref().ok_or_else(|| {
        AppError(
            StatusCode::NOT_IMPLEMENTED,
            "Enable Banking provider not configured".to_owned(),
        )
    })?;
    let redirect_url = state.redirect_url.as_deref().ok_or_else(|| {
        AppError(
            StatusCode::NOT_IMPLEMENTED,
            "redirect_url not configured".to_owned(),
        )
    })?;
    Ok((auth, redirect_url))
}

/// Generate a 64-hex-char state token from two UUIDs (256 bits CSPRNG).
fn generate_state_token() -> String {
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    format!("{}{}", a.as_simple(), b.as_simple())
}

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct AspspsQuery {
    pub country: Option<String>,
}

#[derive(Deserialize)]
pub struct AuthorizeRequest {
    pub aspsp_name: String,
    pub aspsp_country: String,
    #[serde(default = "default_valid_days")]
    pub valid_days: u32,
}

fn default_valid_days() -> u32 {
    90
}

#[derive(Serialize)]
struct AuthorizeResponse {
    authorization_url: String,
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    pub code: String,
    pub state: String,
}

#[derive(Deserialize)]
struct StateTokenData {
    aspsp_name: String,
    #[allow(dead_code)]
    aspsp_country: String,
    valid_until: String,
    institution_name: String,
}

// ---------------------------------------------------------------------------
// Authenticated routes
// ---------------------------------------------------------------------------

/// Build the connections sub-router (authenticated endpoints).
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list))
        .route("/aspsps", get(search_aspsps))
        .route("/authorize", axum::routing::post(authorize))
        .route("/{id}", delete(revoke))
}

/// Build the callback router (unauthenticated).
pub fn callback_router() -> Router<AppState> {
    Router::new().route("/api/connections/callback", get(callback))
}

/// GET /api/connections/aspsps?country=XX
async fn search_aspsps(
    State(state): State<AppState>,
    Query(query): Query<AspspsQuery>,
) -> Result<Json<Vec<AspspEntry>>, AppError> {
    let (auth, _) = require_enable_banking(&state)?;

    let aspsps = auth
        .get_aspsps(query.country.as_deref())
        .await
        .map_err(|e| AppError(StatusCode::BAD_GATEWAY, e.to_string()))?;

    Ok(Json(aspsps))
}

/// POST /api/connections/authorize
async fn authorize(
    State(state): State<AppState>,
    Json(body): Json<AuthorizeRequest>,
) -> Result<Json<AuthorizeResponse>, AppError> {
    let (auth, redirect_url) = require_enable_banking(&state)?;

    let valid_until = chrono::Utc::now()
        .checked_add_signed(chrono::Duration::days(i64::from(body.valid_days)))
        .ok_or_else(|| AppError(StatusCode::BAD_REQUEST, "invalid valid_days".to_owned()))?
        .format("%Y-%m-%d")
        .to_string();

    let token = generate_state_token();

    let token_data = serde_json::json!({
        "aspsp_name": body.aspsp_name,
        "aspsp_country": body.aspsp_country,
        "valid_until": valid_until,
        "institution_name": body.aspsp_name,
    });

    let expires_at = chrono::Utc::now()
        .checked_add_signed(chrono::Duration::minutes(15))
        .ok_or_else(|| {
            AppError(
                StatusCode::INTERNAL_SERVER_ERROR,
                "time overflow".to_owned(),
            )
        })?
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();

    db::insert_state_token(&state.pool, &token, &token_data.to_string(), &expires_at)
        .await
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let authorization_url = auth
        .start_authorization(
            &body.aspsp_name,
            &body.aspsp_country,
            redirect_url,
            &token,
            &valid_until,
        )
        .await
        .map_err(|e| AppError(StatusCode::BAD_GATEWAY, e.to_string()))?;

    Ok(Json(AuthorizeResponse { authorization_url }))
}

/// GET /api/connections/callback?code=X&state=Y  (unauthenticated)
async fn callback(
    State(state): State<AppState>,
    Query(query): Query<CallbackQuery>,
) -> Result<(StatusCode, Json<Connection>), AppError> {
    let (auth, _) = require_enable_banking(&state)?;

    let user_data = db::consume_state_token(&state.pool, &query.state)
        .await
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| {
            AppError(
                StatusCode::BAD_REQUEST,
                "invalid or expired state token".to_owned(),
            )
        })?;

    let token_data: StateTokenData = serde_json::from_str(&user_data).map_err(|e| {
        AppError(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("corrupt state token data: {e}"),
        )
    })?;

    let session = auth
        .exchange_code(&query.code)
        .await
        .map_err(|e| AppError(StatusCode::BAD_GATEWAY, e.to_string()))?;

    let connection_id = ConnectionId::new();

    let connection = Connection {
        id: connection_id,
        provider: "enable_banking".to_owned(),
        provider_session_id: session.session_id,
        institution_name: token_data.institution_name,
        valid_until: token_data.valid_until,
        status: ConnectionStatus::Active,
    };

    db::insert_connection(&state.pool, &connection)
        .await
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Create accounts for each session account, preserving existing UUIDs
    for session_account in &session.accounts {
        let existing = db::get_account_by_provider_id(&state.pool, &session_account.uid)
            .await
            .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        let account = Account {
            id: existing.as_ref().map_or_else(AccountId::new, |a| a.id),
            provider_account_id: session_account.uid.clone(),
            name: session_account
                .account_name
                .clone()
                .or_else(|| session_account.product.clone())
                .unwrap_or_else(|| "Unknown Account".to_owned()),
            institution: session_account
                .institution_name
                .clone()
                .unwrap_or_else(|| token_data.aspsp_name.clone()),
            account_type: parse_cash_account_type(session_account.cash_account_type.as_deref()),
            currency: session_account
                .currency
                .clone()
                .unwrap_or_else(|| "EUR".to_owned()),
            connection_id: Some(connection_id),
        };

        db::upsert_account(&state.pool, &account)
            .await
            .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }

    Ok((StatusCode::CREATED, Json(connection)))
}

/// Map Enable Banking `cash_account_type` to our `AccountType`.
fn parse_cash_account_type(cash_type: Option<&str>) -> AccountType {
    match cash_type {
        Some("CACC") => AccountType::Checking,
        Some("SVGS") => AccountType::Savings,
        Some("CARD" | "CCRD") => AccountType::CreditCard,
        Some("LOAN") => AccountType::Loan,
        _ => AccountType::Other,
    }
}

/// GET /api/connections
async fn list(State(state): State<AppState>) -> Result<Json<Vec<Connection>>, AppError> {
    let connections = db::list_connections(&state.pool).await?;
    Ok(Json(connections))
}

/// DELETE /api/connections/{id}
async fn revoke(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let uuid =
        Uuid::parse_str(&id).map_err(|e| AppError(StatusCode::BAD_REQUEST, e.to_string()))?;
    let connection_id = ConnectionId::from_uuid(uuid);

    let connection = db::get_connection(&state.pool, connection_id)
        .await?
        .ok_or_else(|| AppError(StatusCode::NOT_FOUND, format!("connection {id} not found")))?;

    // Best-effort provider revocation
    if let Some(auth) = &state.enable_banking_auth
        && let Err(e) = auth.revoke_session(&connection.provider_session_id).await
    {
        tracing::warn!(
            connection_id = %id,
            error = %e,
            "failed to revoke provider session, marking as revoked locally"
        );
    }

    db::update_connection_status(&state.pool, connection_id, ConnectionStatus::Revoked).await?;

    Ok(StatusCode::NO_CONTENT)
}
