//! CSV import endpoint for accounts without API access (e.g. Amex EU).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use serde::Serialize;

use budget_core::models::AccountId;

use crate::routes::AppError;
use crate::state::AppState;

#[derive(Serialize)]
struct ImportResponse {
    imported: usize,
    duplicates: usize,
    failed: usize,
}

/// Build the import sub-router.
///
/// Mounts:
/// - `POST /{id}/import` — import a CSV file for the given account
pub fn router() -> Router<AppState> {
    Router::new().route("/{id}/import", post(import_csv))
}

/// Import an Amex CSV export for a specific account.
///
/// Expects `Content-Type: text/csv` with the raw CSV body.
/// Deduplication is handled by the database upsert on `provider_transaction_id`.
async fn import_csv(
    State(state): State<AppState>,
    Path(id): Path<AccountId>,
    body: String,
) -> Result<Json<ImportResponse>, AppError> {
    // Verify account exists
    state
        .db
        .get_account(id)
        .await?
        .ok_or_else(|| AppError(StatusCode::NOT_FOUND, format!("account {id} not found")))?;

    // Parse CSV
    let provider_txns = budget_providers::parse_amex_csv(&body)
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, format!("CSV parse error: {e}")))?;

    // Count how many already exist before importing (for accurate duplicate count)
    let provider_ids: Vec<&str> = provider_txns
        .iter()
        .map(|t| t.provider_transaction_id.as_str())
        .collect();
    let existing = state
        .db
        .count_existing_provider_ids(id, &provider_ids)
        .await
        .map_err(|e| {
            AppError(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to check existing transactions: {e}"),
            )
        })?;
    let existing = usize::try_from(existing).unwrap_or(0);

    // Import via shared upsert pipeline
    let result = budget_jobs::import_provider_transactions(id, &provider_txns, &state.db)
        .await
        .map_err(|e| {
            AppError(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("import failed: {e}"),
            )
        })?;

    // newly imported = successful upserts minus pre-existing duplicates
    let duplicates = existing.min(result.imported);
    let imported = result.imported - duplicates;

    Ok(Json(ImportResponse {
        imported,
        duplicates,
        failed: result.failed.len(),
    }))
}
