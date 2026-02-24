use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::Deserialize;
use uuid::Uuid;

use budget_core::db;
use budget_core::models::{CategoryId, Project, ProjectId};

use crate::routes::AppError;
use crate::state::AppState;

/// Request body for creating or updating a project.
#[derive(Deserialize)]
pub struct CreateProject {
    /// Project display name.
    pub name: String,
    /// Category UUID this project is linked to.
    pub category_id: String,
    /// Start date in ISO 8601 format (YYYY-MM-DD).
    pub start_date: String,
    /// Optional end date in ISO 8601 format.
    pub end_date: Option<String>,
    /// Optional total budget as a decimal string.
    pub budget_amount: Option<String>,
}

/// Build the projects sub-router.
///
/// Mounts:
/// - `GET /` -- list all projects
/// - `POST /` -- create a new project
/// - `PUT /{id}` -- update an existing project
/// - `DELETE /{id}` -- delete a project
///
/// # Errors
///
/// Individual handlers return `AppError` on failure.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", axum::routing::put(update).delete(remove))
}

/// Parse a `CreateProject` body into a domain `Project`.
fn parse_project_body(body: &CreateProject, id: ProjectId) -> Result<Project, AppError> {
    let category_uuid = Uuid::parse_str(&body.category_id)
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, e.to_string()))?;

    let start_date = NaiveDate::parse_from_str(&body.start_date, "%Y-%m-%d")
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, e.to_string()))?;

    let end_date = body
        .end_date
        .as_deref()
        .map(|s| {
            NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .map_err(|e| AppError(StatusCode::BAD_REQUEST, e.to_string()))
        })
        .transpose()?;

    let budget_amount = body
        .budget_amount
        .as_deref()
        .map(|s| {
            s.parse::<Decimal>()
                .map_err(|e| AppError(StatusCode::BAD_REQUEST, e.to_string()))
        })
        .transpose()?;

    Ok(Project {
        id,
        name: body.name.clone(),
        category_id: CategoryId::from_uuid(category_uuid),
        start_date,
        end_date,
        budget_amount,
    })
}

/// List all projects.
///
/// # Errors
///
/// Returns `AppError` if the database query fails.
async fn list(State(state): State<AppState>) -> Result<Json<Vec<Project>>, AppError> {
    let projects = db::list_projects(&state.pool).await?;
    Ok(Json(projects))
}

/// Create a new project.
///
/// # Errors
///
/// Returns 400 if any field value is invalid.
/// Returns `AppError` if the database insert fails.
async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateProject>,
) -> Result<(StatusCode, Json<Project>), AppError> {
    let project = parse_project_body(&body, ProjectId::new())?;
    db::insert_project(&state.pool, &project).await?;
    Ok((StatusCode::CREATED, Json(project)))
}

/// Update an existing project by ID.
///
/// # Errors
///
/// Returns 400 if the ID or any field value is invalid.
/// Returns `AppError` if the database update fails.
async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<CreateProject>,
) -> Result<Json<Project>, AppError> {
    let uuid =
        Uuid::parse_str(&id).map_err(|e| AppError(StatusCode::BAD_REQUEST, e.to_string()))?;
    let project = parse_project_body(&body, ProjectId::from_uuid(uuid))?;
    db::update_project(&state.pool, &project).await?;
    Ok(Json(project))
}

/// Delete a project by its UUID path parameter.
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
    db::delete_project(&state.pool, ProjectId::from_uuid(uuid)).await?;
    Ok(StatusCode::NO_CONTENT)
}
