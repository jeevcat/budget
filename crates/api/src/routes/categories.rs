use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use budget_core::models::{BudgetMode, BudgetType, Category, CategoryId, CategoryName};

use crate::routes::AppError;
use crate::state::AppState;

/// Request body for creating a new category.
#[derive(Deserialize)]
pub struct CreateCategory {
    /// Category display name.
    pub name: String,
    /// Optional parent category UUID for nesting.
    pub parent_id: Option<String>,
    /// Budget mode: "monthly", "annual", or "project". Null for no budget.
    pub budget_mode: Option<String>,
    /// Budget type: "fixed" or "variable". Null defaults to variable.
    pub budget_type: Option<String>,
    /// Budget amount as a decimal string (e.g. "500.00").
    pub budget_amount: Option<String>,
    /// Project start date (YYYY-MM-DD). Only used when `budget_mode` is "project".
    pub project_start_date: Option<String>,
    /// Project end date (YYYY-MM-DD). Only used when `budget_mode` is "project".
    pub project_end_date: Option<String>,
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
/// - `PUT /{id}` -- update a category
/// - `DELETE /{id}` -- delete a category
///
/// # Errors
///
/// Individual handlers return `AppError` on failure.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/suggestions", get(suggestions))
        .route("/{id}", axum::routing::put(update).delete(remove))
}

/// A category with its transaction count.
#[derive(Serialize)]
struct CategoryWithCount {
    #[serde(flatten)]
    category: Category,
    transaction_count: i64,
}

/// List all categories with transaction counts.
///
/// # Errors
///
/// Returns `AppError` if the database query fails.
async fn list(State(state): State<AppState>) -> Result<Json<Vec<CategoryWithCount>>, AppError> {
    let (categories, direct_counts) = tokio::try_join!(
        state.db.list_categories(),
        state.db.category_transaction_counts(),
    )?;

    // Parent categories should show the sum of their own direct count plus
    // all children's counts, so the number reflects the entire subtree.
    let mut counts = direct_counts.clone();
    for cat in &categories {
        if let Some(parent_id) = cat.parent_id {
            let child_count = direct_counts.get(&cat.id).copied().unwrap_or(0);
            if child_count > 0 {
                *counts.entry(parent_id).or_insert(0) += child_count;
            }
        }
    }

    let result = categories
        .into_iter()
        .map(|c| {
            let transaction_count = counts.get(&c.id).copied().unwrap_or(0);
            CategoryWithCount {
                category: c,
                transaction_count,
            }
        })
        .collect();
    Ok(Json(result))
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
    let histogram = state.db.get_suggestion_histogram().await?;
    let entries = histogram
        .into_iter()
        .map(|(category_name, count)| SuggestionEntry {
            category_name,
            count,
        })
        .collect();
    Ok(Json(entries))
}

/// Parsed budget fields from a create/update request.
struct BudgetFields {
    mode: Option<BudgetMode>,
    budget_type: Option<BudgetType>,
    amount: Option<Decimal>,
    start: Option<NaiveDate>,
    end: Option<NaiveDate>,
}

/// Parse optional budget fields from a create/update request body.
fn parse_budget_fields(body: &CreateCategory) -> Result<BudgetFields, AppError> {
    let mode = body
        .budget_mode
        .as_deref()
        .map(|s| {
            s.parse::<BudgetMode>()
                .map_err(|e: budget_core::error::Error| {
                    AppError(StatusCode::BAD_REQUEST, e.to_string())
                })
        })
        .transpose()?;

    let amount = body
        .budget_amount
        .as_deref()
        .map(|s| {
            s.parse::<Decimal>()
                .map_err(|e| AppError(StatusCode::BAD_REQUEST, e.to_string()))
        })
        .transpose()?;

    let start = body
        .project_start_date
        .as_deref()
        .map(|s| {
            NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .map_err(|e| AppError(StatusCode::BAD_REQUEST, e.to_string()))
        })
        .transpose()?;

    let end = body
        .project_end_date
        .as_deref()
        .map(|s| {
            NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .map_err(|e| AppError(StatusCode::BAD_REQUEST, e.to_string()))
        })
        .transpose()?;

    let budget_type = body
        .budget_type
        .as_deref()
        .map(|s| {
            s.parse::<BudgetType>()
                .map_err(|e: budget_core::error::Error| {
                    AppError(StatusCode::BAD_REQUEST, e.to_string())
                })
        })
        .transpose()?;

    Ok(BudgetFields {
        mode,
        budget_type,
        amount,
        start,
        end,
    })
}

/// Create a new category.
///
/// # Errors
///
/// Returns 400 if `parent_id` is present but not a valid UUID, or if budget
/// fields are invalid.
/// Returns `AppError` if the database insert fails.
async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateCategory>,
) -> Result<(StatusCode, Json<Category>), AppError> {
    let budget = parse_budget_fields(&body)?;

    let name = CategoryName::new(body.name)
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, e.to_string()))?;

    let parent_id = body
        .parent_id
        .as_deref()
        .map(|s| {
            Uuid::parse_str(s)
                .map(CategoryId::from_uuid)
                .map_err(|e| AppError(StatusCode::BAD_REQUEST, e.to_string()))
        })
        .transpose()?;

    let category = Category {
        id: CategoryId::new(),
        name,
        parent_id,
        budget_mode: budget.mode,
        budget_type: budget.budget_type,
        budget_amount: budget.amount,
        project_start_date: budget.start,
        project_end_date: budget.end,
    };

    state.db.insert_category(&category).await?;
    Ok((StatusCode::CREATED, Json(category)))
}

/// Update an existing category by ID.
///
/// # Errors
///
/// Returns 400 if the ID or any field value is invalid.
/// Returns `AppError` if the database update fails.
async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<CreateCategory>,
) -> Result<Json<Category>, AppError> {
    let uuid =
        Uuid::parse_str(&id).map_err(|e| AppError(StatusCode::BAD_REQUEST, e.to_string()))?;

    let budget = parse_budget_fields(&body)?;

    let name = CategoryName::new(body.name)
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, e.to_string()))?;

    let parent_id = body
        .parent_id
        .as_deref()
        .map(|s| {
            Uuid::parse_str(s)
                .map(CategoryId::from_uuid)
                .map_err(|e| AppError(StatusCode::BAD_REQUEST, e.to_string()))
        })
        .transpose()?;

    let category = Category {
        id: CategoryId::from_uuid(uuid),
        name,
        parent_id,
        budget_mode: budget.mode,
        budget_type: budget.budget_type,
        budget_amount: budget.amount,
        project_start_date: budget.start,
        project_end_date: budget.end,
    };

    state.db.update_category(&category).await?;
    Ok(Json(category))
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

    state.db.delete_category(category_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
