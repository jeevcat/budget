use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use budget_core::models::{BudgetConfig, Category, CategoryId, CategoryName};

use crate::routes::AppError;
use crate::state::AppState;

/// Request body for creating or updating a category.
///
/// All fields are parsed at deserialization time — no manual string parsing
/// needed in handlers.
#[derive(Deserialize)]
pub struct CreateCategory {
    /// Category display name (validated by `CategoryName`).
    pub name: CategoryName,
    /// Optional parent category UUID for nesting.
    pub parent_id: Option<CategoryId>,
    /// Budget configuration. Deserialized from flat fields via `BudgetConfig`.
    #[serde(flatten)]
    pub budget: BudgetConfig,
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

/// Create a new category.
///
/// # Errors
///
/// Returns 400 if the request body fails deserialization (invalid name,
/// UUID, enum, decimal, or date).
/// Returns `AppError` if the database insert fails.
async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateCategory>,
) -> Result<(StatusCode, Json<Category>), AppError> {
    let category = Category {
        id: CategoryId::new(),
        name: body.name,
        parent_id: body.parent_id,
        budget: body.budget,
    };

    state.db.insert_category(&category).await?;
    Ok((StatusCode::CREATED, Json(category)))
}

/// Update an existing category by ID.
///
/// # Errors
///
/// Returns 400 if the ID is not a valid UUID or the body fails deserialization.
/// Returns `AppError` if the database update fails.
async fn update(
    State(state): State<AppState>,
    Path(id): Path<CategoryId>,
    Json(body): Json<CreateCategory>,
) -> Result<Json<Category>, AppError> {
    let category = Category {
        id,
        name: body.name,
        parent_id: body.parent_id,
        budget: body.budget,
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
    Path(id): Path<CategoryId>,
) -> Result<StatusCode, AppError> {
    state.db.delete_category(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use budget_core::models::BudgetMode;

    #[test]
    fn deserialize_create_category_minimal() {
        let json = r#"{"name": "Groceries"}"#;
        let cat: CreateCategory = serde_json::from_str(json).unwrap();
        assert_eq!(cat.name, "Groceries");
        assert!(cat.parent_id.is_none());
        assert_eq!(cat.budget, BudgetConfig::None);
    }

    #[test]
    fn deserialize_create_category_with_monthly_budget() {
        let json = r#"{
            "name": "Food",
            "budget_mode": "monthly",
            "budget_type": "variable",
            "budget_amount": "500.00"
        }"#;
        let cat: CreateCategory = serde_json::from_str(json).unwrap();
        assert_eq!(cat.name, "Food");
        assert_eq!(cat.budget.mode(), Some(BudgetMode::Monthly));
        assert_eq!(
            cat.budget.amount(),
            Some(rust_decimal::Decimal::new(50000, 2))
        );
    }

    #[test]
    fn deserialize_create_category_with_parent_id() {
        let id = uuid::Uuid::new_v4();
        let json = format!(r#"{{"name": "Dining", "parent_id": "{id}"}}"#);
        let cat: CreateCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(*cat.parent_id.unwrap().as_uuid(), id);
    }

    #[test]
    fn deserialize_create_category_with_null_parent() {
        let json = r#"{"name": "Cash", "parent_id": null}"#;
        let cat: CreateCategory = serde_json::from_str(json).unwrap();
        assert!(cat.parent_id.is_none());
    }

    #[test]
    fn deserialize_create_category_with_project_budget() {
        let json = r#"{
            "name": "Renovation",
            "budget_mode": "project",
            "budget_amount": "10000.00",
            "project_start_date": "2026-01-01",
            "project_end_date": "2026-06-30"
        }"#;
        let cat: CreateCategory = serde_json::from_str(json).unwrap();
        assert_eq!(cat.budget.mode(), Some(BudgetMode::Project));
    }

    #[test]
    fn deserialize_create_category_with_salary() {
        let json = r#"{"name": "Salary", "budget_mode": "salary"}"#;
        let cat: CreateCategory = serde_json::from_str(json).unwrap();
        assert_eq!(cat.budget, BudgetConfig::Salary);
    }

    #[test]
    fn deserialize_create_category_with_transfer() {
        let json = r#"{"name": "Investments", "budget_mode": "transfer"}"#;
        let cat: CreateCategory = serde_json::from_str(json).unwrap();
        assert_eq!(cat.budget, BudgetConfig::Transfer);
    }

    #[test]
    fn deserialize_create_category_rejects_invalid_name() {
        let json = r#"{"name": ""}"#;
        assert!(serde_json::from_str::<CreateCategory>(json).is_err());
    }

    #[test]
    fn deserialize_create_category_rejects_colon_name() {
        let json = r#"{"name": "Food:Dining"}"#;
        assert!(serde_json::from_str::<CreateCategory>(json).is_err());
    }

    #[test]
    fn deserialize_create_category_rejects_invalid_parent_id() {
        let json = r#"{"name": "Food", "parent_id": "not-a-uuid"}"#;
        assert!(serde_json::from_str::<CreateCategory>(json).is_err());
    }

    #[test]
    fn deserialize_create_category_rejects_invalid_budget_mode() {
        let json = r#"{"name": "Food", "budget_mode": "invalid"}"#;
        assert!(serde_json::from_str::<CreateCategory>(json).is_err());
    }

    #[test]
    fn deserialize_create_category_rejects_invalid_budget_type() {
        let json = r#"{"name": "Food", "budget_mode": "monthly", "budget_type": "bogus"}"#;
        assert!(serde_json::from_str::<CreateCategory>(json).is_err());
    }

    #[test]
    fn deserialize_create_category_rejects_invalid_amount() {
        let json = r#"{"name": "Food", "budget_mode": "monthly", "budget_amount": "abc"}"#;
        assert!(serde_json::from_str::<CreateCategory>(json).is_err());
    }

    #[test]
    fn deserialize_create_category_rejects_invalid_date() {
        let json = r#"{
            "name": "Reno",
            "budget_mode": "project",
            "budget_amount": "1000",
            "project_start_date": "not-a-date"
        }"#;
        assert!(serde_json::from_str::<CreateCategory>(json).is_err());
    }

    #[test]
    fn deserialize_create_category_null_budget_fields() {
        let json = r#"{
            "name": "Misc",
            "budget_mode": null,
            "budget_type": null,
            "budget_amount": null,
            "project_start_date": null,
            "project_end_date": null
        }"#;
        let cat: CreateCategory = serde_json::from_str(json).unwrap();
        assert_eq!(cat.budget, BudgetConfig::None);
    }
}
