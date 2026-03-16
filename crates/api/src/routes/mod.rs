pub mod accounts;
pub mod amazon;
pub mod budgets;
pub mod categories;
pub mod connections;
pub mod import;
pub mod jobs;
pub mod rules;
pub mod transactions;

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use budget_db::DbError;
use serde::Serialize;

/// JSON body returned for all error responses.
#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

/// Unified error type for route handlers.
///
/// Wraps an HTTP status code and a human-readable error message.
/// Handlers return `Result<T, AppError>` so that `?` propagation
/// produces appropriate HTTP responses automatically.
pub struct AppError(pub StatusCode, pub String);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (self.0, Json(ErrorBody { error: self.1 })).into_response()
    }
}

impl From<DbError> for AppError {
    fn from(e: DbError) -> Self {
        match e {
            DbError::UniqueViolation => Self(StatusCode::CONFLICT, "already exists".to_owned()),
            other => {
                tracing::error!(error = %other, "database error");
                Self(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "database error".to_owned(),
                )
            }
        }
    }
}
