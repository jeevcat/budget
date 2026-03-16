use axum::extract::State;
use axum::http::StatusCode;
use axum::http::header::SET_COOKIE;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use budget_core::models::SecretKey;
use serde::Deserialize;

use crate::state::AppState;

/// Cookie name for the auth token.
const COOKIE_NAME: &str = "budget_session";

/// JSON 401 response with a machine-readable `reason` for debuggability.
fn unauthorized(reason: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({
            "error": "unauthorized",
            "reason": reason,
        })),
    )
        .into_response()
}

/// Middleware that validates the request against the configured `secret_key`.
///
/// Checks (in order):
/// 1. `Authorization: Bearer <token>` header
/// 2. `budget_session` `HttpOnly` cookie
///
/// Returns a JSON body with a `reason` field on failure so that
/// authentication problems are easy to diagnose from logs or curl.
pub async fn require_bearer_token(
    State(state): State<AppState>,
    request: axum::extract::Request,
    next: Next,
) -> Response {
    if state.secret_key.as_ref().is_empty() {
        return unauthorized("not_configured");
    }

    // Check Authorization header first
    if let Some(value) = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        && let Some(token) = value.strip_prefix("Bearer ")
    {
        if SecretKey::new(token).ok().as_ref() == Some(&state.secret_key) {
            return next.run(request).await;
        }
        return unauthorized("invalid_token");
    }

    // Fall back to cookie
    if let Some(cookie_header) = request
        .headers()
        .get("cookie")
        .and_then(|v| v.to_str().ok())
    {
        for cookie in cookie_header.split(';') {
            let cookie = cookie.trim();
            if let Some(value) = cookie.strip_prefix("budget_session=") {
                if SecretKey::new(value).ok().as_ref() == Some(&state.secret_key) {
                    return next.run(request).await;
                }
                return unauthorized("invalid_token");
            }
        }
    }

    unauthorized("missing_token")
}

#[derive(Deserialize, utoipa::ToSchema)]
struct LoginRequest {
    #[schema(value_type = String)]
    token: SecretKey,
}

/// Build the auth sub-router (unauthenticated endpoints).
///
/// Mounts:
/// - `POST /api/login` -- validate token and set `HttpOnly` cookie
/// - `POST /api/logout` -- clear the auth cookie
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/login", post(login))
        .route("/logout", post(logout))
}

/// Validate the provided token and set an `HttpOnly` session cookie.
#[utoipa::path(post, path = "/login", tag = "auth", request_body = LoginRequest, responses((status = 200)))]
async fn login(State(state): State<AppState>, Json(body): Json<LoginRequest>) -> Response {
    if state.secret_key.as_ref().is_empty() {
        return unauthorized("not_configured");
    }
    if state.secret_key != body.token {
        return unauthorized("invalid_token");
    }

    let cookie = format!(
        "{COOKIE_NAME}={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age=31536000",
        token = body.token,
    );

    (
        [(SET_COOKIE, cookie)],
        Json(serde_json::json!({"ok": true})),
    )
        .into_response()
}

/// Clear the auth cookie.
#[utoipa::path(post, path = "/logout", tag = "auth", responses((status = 200)))]
async fn logout() -> Response {
    let cookie = format!("{COOKIE_NAME}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0");
    (
        [(SET_COOKIE, cookie)],
        Json(serde_json::json!({"ok": true})),
    )
        .into_response()
}
