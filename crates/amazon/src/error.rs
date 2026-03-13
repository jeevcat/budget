use thiserror::Error;

#[derive(Debug, Error)]
pub enum AmazonError {
    #[error("cookies expired or missing — re-login required")]
    CookiesExpired,

    #[error("parse error: {0}")]
    Parse(String),

    #[error("rate limited by Amazon — wait before retrying")]
    RateLimited,

    #[error("bot detection triggered — CAPTCHA or challenge page returned")]
    BotDetected,

    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("order not found: {0}")]
    OrderNotFound(String),

    #[error("JWT expired — re-fetch transactions page to refresh")]
    JwtExpired,
}

pub type Result<T> = std::result::Result<T, AmazonError>;
