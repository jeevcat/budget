use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use budget_core::db;
use budget_core::models::{Category, CategoryId};

use crate::routes::AppError;
use crate::state::AppState;

/// Request body for creating a new category.
#[derive(Deserialize)]
pub struct CreateCategory {
    /// Category display name.
    pub name: String,
    /// Optional parent category UUID for nesting.
    pub parent_id: Option<String>,
}

/// A single entry in the LLM suggestion histogram.
#[derive(Serialize)]
pub struct SuggestionEntry {
    pub category_name: String,
    pub count: i64,
}

/// Build the categories sub-router.
///
/// Mounts:
/// - `GET /` -- list all categories
/// - `POST /` -- create a new category
/// - `GET /suggestions` -- histogram of LLM-suggested categories
/// - `DELETE /{id}` -- delete a category
///
/// # Errors
///
/// Individual handlers return `AppError` on failure.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/suggestions", get(suggestions))
        .route("/{id}", axum::routing::delete(remove))
}

/// List all categories.
///
/// # Errors
///
/// Returns `AppError` if the database query fails.
async fn list(State(state): State<AppState>) -> Result<Json<Vec<Category>>, AppError> {
    let categories = db::list_categories(&state.pool).await?;
    Ok(Json(categories))
}

/// Histogram of LLM-suggested category names for uncategorized transactions.
///
/// Returns entries sorted by count descending, allowing the user to see
/// which categories the LLM recommends most and create them.
///
/// # Errors
///
/// Returns `AppError` if the database query fails.
async fn suggestions(
    State(state): State<AppState>,
) -> Result<Json<Vec<SuggestionEntry>>, AppError> {
    let histogram = db::get_suggestion_histogram(&state.pool).await?;
    let entries = histogram
        .into_iter()
        .map(|(category_name, count)| SuggestionEntry {
            category_name,
            count,
        })
        .collect();
    Ok(Json(entries))
}

/// Create a new category.
///
/// # Errors
///
/// Returns 400 if `parent_id` is present but not a valid UUID.
/// Returns `AppError` if the database insert fails.
async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateCategory>,
) -> Result<(StatusCode, Json<Category>), AppError> {
    let parent_id = body
        .parent_id
        .map(|s| {
            Uuid::parse_str(&s)
                .map(CategoryId::from_uuid)
                .map_err(|e| AppError(StatusCode::BAD_REQUEST, e.to_string()))
        })
        .transpose()?;

    let category = Category {
        id: CategoryId::new(),
        name: body.name,
        parent_id,
    };

    db::insert_category(&state.pool, &category).await?;
    Ok((StatusCode::CREATED, Json(category)))
}

/// Delete a category by its UUID path parameter.
///
/// # Errors
///
/// Returns 400 if the ID is not a valid UUID.
/// Returns `AppError` if the database delete fails.
async fn remove(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let uuid =
        Uuid::parse_str(&id).map_err(|e| AppError(StatusCode::BAD_REQUEST, e.to_string()))?;
    let category_id = CategoryId::from_uuid(uuid);

    db::delete_category(&state.pool, category_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
