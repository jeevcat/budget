use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{delete, get};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use budget_core::models::{
    Account, AccountId, AccountType, Connection, ConnectionId, ConnectionStatus,
};
use budget_providers::{AspspEntry, EnableBankingAuth};

use crate::routes::AppError;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn require_enable_banking(state: &AppState) -> Result<&Arc<EnableBankingAuth>, AppError> {
    state.enable_banking_auth.as_ref().ok_or_else(|| {
        AppError(
            StatusCode::NOT_IMPLEMENTED,
            "Enable Banking provider not configured".to_owned(),
        )
    })
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
    #[serde(default = "default_psu_type")]
    pub psu_type: String,
}

fn default_valid_days() -> u32 {
    90
}

fn default_psu_type() -> String {
    "personal".to_owned()
}

#[derive(Serialize)]
struct AuthorizeResponse {
    authorization_url: String,
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    pub code: Option<String>,
    pub state: String,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

#[derive(Deserialize)]
struct StateTokenData {
    aspsp_name: String,
    #[allow(dead_code)]
    aspsp_country: String,
    valid_until: chrono::DateTime<chrono::Utc>,
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
    let auth = require_enable_banking(&state)?;

    let country = query.country.as_deref().map(str::to_uppercase);
    let aspsps = auth
        .get_aspsps(country.as_deref())
        .await
        .map_err(|e| AppError(StatusCode::BAD_GATEWAY, e.to_string()))?;

    Ok(Json(aspsps))
}

/// POST /api/connections/authorize
async fn authorize(
    State(state): State<AppState>,
    Json(body): Json<AuthorizeRequest>,
) -> Result<Json<AuthorizeResponse>, AppError> {
    let auth = require_enable_banking(&state)?;
    let redirect_url = format!("{}/api/connections/callback", state.host);

    let valid_until = chrono::Utc::now()
        .checked_add_signed(chrono::Duration::days(i64::from(body.valid_days)))
        .ok_or_else(|| AppError(StatusCode::BAD_REQUEST, "invalid valid_days".to_owned()))?;

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
        })?;

    state
        .db
        .insert_state_token(&token, &token_data.to_string(), expires_at)
        .await
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let valid_until_str = valid_until.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let authorization_url = auth
        .start_authorization(
            &body.aspsp_name,
            &body.aspsp_country,
            &redirect_url,
            &token,
            &valid_until_str,
            &body.psu_type,
        )
        .await
        .map_err(|e| {
            tracing::error!(
                aspsp_name = %body.aspsp_name,
                aspsp_country = %body.aspsp_country,
                redirect_url = %redirect_url,
                error = %e,
                "Enable Banking authorization failed"
            );
            AppError(StatusCode::BAD_GATEWAY, e.to_string())
        })?;

    Ok(Json(AuthorizeResponse { authorization_url }))
}

/// GET /api/connections/callback?code=X&state=Y  (unauthenticated)
async fn callback(
    State(state): State<AppState>,
    Query(query): Query<CallbackQuery>,
) -> Result<(StatusCode, Json<Connection>), AppError> {
    if let Some(err) = &query.error {
        let desc = query
            .error_description
            .as_deref()
            .unwrap_or("no description");
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            format!("authorization denied: {err} — {desc}"),
        ));
    }

    let code = query.code.as_deref().ok_or_else(|| {
        AppError(
            StatusCode::BAD_REQUEST,
            "missing authorization code".to_owned(),
        )
    })?;

    let auth = require_enable_banking(&state)?;

    let user_data = state
        .db
        .consume_state_token(&query.state)
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
        .exchange_code(code)
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

    state
        .db
        .insert_connection(&connection)
        .await
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Create accounts for each session account, preserving existing UUIDs
    for session_account in &session.accounts {
        let existing = state
            .db
            .get_account_by_provider_id(&session_account.uid)
            .await
            .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        let account = Account {
            id: existing.as_ref().map_or_else(AccountId::new, |a| a.id),
            provider_account_id: session_account.uid.clone(),
            name: session_account
                .name
                .clone()
                .or_else(|| session_account.product.clone())
                .unwrap_or_else(|| "Unknown Account".to_owned()),
            institution: session_account
                .account_servicer
                .as_ref()
                .and_then(|s| s.name.clone())
                .unwrap_or_else(|| token_data.aspsp_name.clone()),
            account_type: parse_cash_account_type(session_account.cash_account_type.as_deref()),
            currency: session_account
                .currency
                .clone()
                .unwrap_or_else(|| "EUR".to_owned()),
            connection_id: Some(connection_id),
        };

        state
            .db
            .upsert_account(&account)
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
    let connections = state.db.list_connections().await?;
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

    let connection = state
        .db
        .get_connection(connection_id)
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

    state
        .db
        .update_connection_status(connection_id, ConnectionStatus::Revoked)
        .await?;

    Ok(StatusCode::NO_CONTENT)
}
