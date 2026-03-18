/// Errors that can occur when interacting with the `PayPal` API.
#[derive(Debug, thiserror::Error)]
pub enum PayPalError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("OAuth token request failed: {status} {body}")]
    AuthFailed { status: u16, body: String },
    #[error("API error: {status} {body}")]
    Api { status: u16, body: String },
    #[error("rate limited")]
    RateLimited,
    #[error("invalid credentials")]
    InvalidCredentials,
}
