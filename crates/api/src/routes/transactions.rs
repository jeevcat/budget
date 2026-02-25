use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use uuid::Uuid;

use budget_core::models::{CategoryId, CategoryMethod, Transaction, TransactionId};

use crate::routes::AppError;
use crate::state::AppState;

/// Request body for categorizing a transaction.
#[derive(Deserialize)]
pub struct CategorizeRequest {
    /// The category UUID to assign.
    pub category_id: String,
}

/// Build the transactions sub-router.
///
/// Mounts:
/// - `GET /` -- list all transactions
/// - `GET /uncategorized` -- list uncategorized transactions
/// - `POST /{id}/categorize` -- assign a category to a transaction
///
/// # Errors
///
/// Individual handlers return `AppError` on failure.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list))
        .route("/uncategorized", get(uncategorized))
        .route("/{id}/categorize", post(categorize))
}

/// List all transactions across all accounts.
///
/// # Errors
///
/// Returns `AppError` if the database query fails.
async fn list(State(state): State<AppState>) -> Result<Json<Vec<Transaction>>, AppError> {
    let transactions = state.db.list_transactions().await?;
    Ok(Json(transactions))
}

/// List transactions that have not been assigned a category.
///
/// # Errors
///
/// Returns `AppError` if the database query fails.
async fn uncategorized(State(state): State<AppState>) -> Result<Json<Vec<Transaction>>, AppError> {
    let transactions = state.db.get_uncategorized_transactions().await?;
    Ok(Json(transactions))
}

/// Assign a category to a single transaction.
///
/// # Errors
///
/// Returns 400 if the transaction ID or category ID is not a valid UUID.
/// Returns `AppError` if the database update fails.
async fn categorize(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<CategorizeRequest>,
) -> Result<StatusCode, AppError> {
    let txn_uuid =
        Uuid::parse_str(&id).map_err(|e| AppError(StatusCode::BAD_REQUEST, e.to_string()))?;
    let cat_uuid = Uuid::parse_str(&body.category_id)
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, e.to_string()))?;

    let txn_id = TransactionId::from_uuid(txn_uuid);
    let category_id = CategoryId::from_uuid(cat_uuid);

    state
        .db
        .update_transaction_category(txn_id, category_id, CategoryMethod::Manual)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
