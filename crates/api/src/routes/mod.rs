pub mod accounts;
pub mod budgets;
pub mod categories;
pub mod connections;
pub mod jobs;
pub mod rules;
pub mod transactions;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

/// Unified error type for route handlers.
///
/// Wraps an HTTP status code and a human-readable error message.
/// Handlers return `Result<T, AppError>` so that `?` propagation
/// produces appropriate HTTP responses automatically.
pub struct AppError(pub StatusCode, pub String);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (self.0, self.1).into_response()
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        if let sqlx::Error::Database(ref db_err) = e
            && db_err.is_unique_violation()
        {
            return Self(StatusCode::CONFLICT, "already exists".to_owned());
        }
        tracing::error!(error = %e, "database error");
        Self(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    }
}
