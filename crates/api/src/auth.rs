use axum::extract::State;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;

use crate::state::AppState;

/// Middleware that validates the `Authorization: Bearer <token>` header
/// against the configured `secret_key`.
///
/// # Errors
///
/// Returns `401 Unauthorized` if the header is missing, malformed, or
/// does not match the expected key.
pub async fn require_bearer_token(
    State(state): State<AppState>,
    request: axum::extract::Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let header = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok());

    match header {
        Some(value) if value.starts_with("Bearer ") => {
            let token = &value["Bearer ".len()..];
            if token == state.secret_key {
                Ok(next.run(request).await)
            } else {
                Err(StatusCode::UNAUTHORIZED)
            }
        }
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}
