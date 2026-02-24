use thiserror::Error;

#[derive(Debug, Clone, Error)]
pub enum ProviderError {
    #[error("connection failed: {0}")]
    ConnectionFailed(String),

    #[error("authentication failed: {0}")]
    AuthenticationFailed(String),

    #[error("rate limited")]
    RateLimited,

    #[error("not found: {0}")]
    NotFound(String),

    #[error("{0}")]
    Other(String),
}
