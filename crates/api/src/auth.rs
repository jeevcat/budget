use axum::extract::State;
use axum::http::StatusCode;
use axum::http::header::SET_COOKIE;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;

use crate::state::AppState;

/// Cookie name for the auth token.
const COOKIE_NAME: &str = "budget_session";

/// Middleware that validates the request against the configured `secret_key`.
///
/// Checks (in order):
/// 1. `Authorization: Bearer <token>` header
/// 2. `budget_session` `HttpOnly` cookie
///
/// # Errors
///
/// Returns `401 Unauthorized` if no valid credential is found.
pub async fn require_bearer_token(
    State(state): State<AppState>,
    request: axum::extract::Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Check Authorization header first
    if let Some(value) = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        && let Some(token) = value.strip_prefix("Bearer ")
    {
        if token == state.secret_key {
            return Ok(next.run(request).await);
        }
        return Err(StatusCode::UNAUTHORIZED);
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
                if value == state.secret_key {
                    return Ok(next.run(request).await);
                }
                return Err(StatusCode::UNAUTHORIZED);
            }
        }
    }

    Err(StatusCode::UNAUTHORIZED)
}

#[derive(Deserialize)]
struct LoginRequest {
    token: String,
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
async fn login(
    State(state): State<AppState>,
    Json(body): Json<LoginRequest>,
) -> Result<Response, StatusCode> {
    if body.token != state.secret_key {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let cookie = format!(
        "{COOKIE_NAME}={token}; `HttpOnly`; SameSite=Strict; Path=/; Max-Age=31536000",
        token = body.token,
    );

    Ok((
        [(SET_COOKIE, cookie)],
        Json(serde_json::json!({"ok": true})),
    )
        .into_response())
}

/// Clear the auth cookie.
async fn logout() -> Response {
    let cookie = format!("{COOKIE_NAME}=; `HttpOnly`; SameSite=Strict; Path=/; Max-Age=0");
    (
        [(SET_COOKIE, cookie)],
        Json(serde_json::json!({"ok": true})),
    )
        .into_response()
}
