use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use budget_core::models::{
    CategoryId, CategoryMethod, CorrelationType, MatchField, Rule, RuleCondition, RuleId, RuleType,
};
use budget_core::rules::{compile_rule, evaluate_categorization_rules};
use budget_jobs::CategorizeJob;

use crate::routes::AppError;
use crate::state::AppState;

/// A single condition in a rule request body.
#[derive(Deserialize)]
pub struct ConditionRequest {
    pub field: String,
    pub pattern: String,
}

/// Request body for creating or updating a rule.
#[derive(Deserialize)]
pub struct CreateRule {
    pub rule_type: String,
    pub conditions: Vec<ConditionRequest>,
    pub target_category_id: Option<String>,
    pub target_correlation_type: Option<String>,
    pub priority: i32,
}

/// Build the rules sub-router.
///
/// Mounts:
/// - `GET /` -- list all rules
/// - `POST /` -- create a new rule
/// - `PUT /{id}` -- update an existing rule
/// - `DELETE /{id}` -- delete a rule
/// - `POST /apply` -- apply all categorization rules to uncategorized transactions
///
/// # Errors
///
/// Individual handlers return `AppError` on failure.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", axum::routing::put(update).delete(remove))
        .route("/apply", axum::routing::post(apply))
}

/// Parse a `CreateRule` body into domain types, reusing the shared parsing
/// logic for both create and update handlers.
fn parse_rule_body(body: &CreateRule, id: RuleId) -> Result<Rule, AppError> {
    let rule_type: RuleType = body
        .rule_type
        .parse()
        .map_err(|e: budget_core::error::Error| AppError(StatusCode::BAD_REQUEST, e.to_string()))?;

    if body.conditions.is_empty() {
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            "conditions must not be empty".to_owned(),
        ));
    }

    let conditions: Vec<RuleCondition> = body
        .conditions
        .iter()
        .map(|c| {
            let field: MatchField = c.field.parse().map_err(|e: budget_core::error::Error| {
                AppError(StatusCode::BAD_REQUEST, e.to_string())
            })?;
            Ok(RuleCondition {
                field,
                pattern: c.pattern.clone(),
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?;

    let target_category_id = body
        .target_category_id
        .as_deref()
        .map(|s| {
            Uuid::parse_str(s)
                .map(CategoryId::from_uuid)
                .map_err(|e| AppError(StatusCode::BAD_REQUEST, e.to_string()))
        })
        .transpose()?;

    let target_correlation_type = body
        .target_correlation_type
        .as_deref()
        .map(|s| {
            s.parse::<CorrelationType>()
                .map_err(|e: budget_core::error::Error| {
                    AppError(StatusCode::BAD_REQUEST, e.to_string())
                })
        })
        .transpose()?;

    Ok(Rule {
        id,
        rule_type,
        conditions,
        target_category_id,
        target_correlation_type,
        priority: body.priority,
    })
}

/// List all rules.
///
/// # Errors
///
/// Returns `AppError` if the database query fails.
async fn list(State(state): State<AppState>) -> Result<Json<Vec<Rule>>, AppError> {
    let rules = state.db.list_rules().await?;
    Ok(Json(rules))
}

/// Create a new rule.
///
/// # Errors
///
/// Returns 400 if any field value is invalid.
/// Returns `AppError` if the database insert fails.
async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateRule>,
) -> Result<(StatusCode, Json<Rule>), AppError> {
    let rule = parse_rule_body(&body, RuleId::new())?;
    state.db.insert_rule(&rule).await?;

    // Re-evaluate eligible transactions against the new rule
    state
        .categorize_storage
        .push(CategorizeJob)
        .await
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;

    Ok((StatusCode::CREATED, Json(rule)))
}

/// Update an existing rule by ID.
///
/// # Errors
///
/// Returns 400 if the ID or any field value is invalid.
/// Returns `AppError` if the database update fails.
async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<CreateRule>,
) -> Result<Json<Rule>, AppError> {
    let uuid =
        Uuid::parse_str(&id).map_err(|e| AppError(StatusCode::BAD_REQUEST, e.to_string()))?;
    let rule = parse_rule_body(&body, RuleId::from_uuid(uuid))?;
    state.db.update_rule(&rule).await?;
    Ok(Json(rule))
}

/// Delete a rule by its UUID path parameter.
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
    state.db.delete_rule(RuleId::from_uuid(uuid)).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Apply
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ApplyRulesResponse {
    categorized_count: u32,
}

/// Apply all categorization rules to uncategorized transactions.
///
/// Loads and compiles rules, evaluates them against every uncategorized
/// transaction, and assigns the first matching category.
///
/// # Errors
///
/// Returns `AppError` on database failures.
async fn apply(State(state): State<AppState>) -> Result<Json<ApplyRulesResponse>, AppError> {
    let raw_rules = state
        .db
        .list_rules_by_type(RuleType::Categorization)
        .await?;
    let compiled_rules: Vec<_> = raw_rules
        .iter()
        .filter_map(|rule| match compile_rule(rule) {
            Ok(compiled) => Some(compiled),
            Err(e) => {
                tracing::warn!(rule_id = %rule.id, error = %e, "skipping rule with invalid pattern");
                None
            }
        })
        .collect();

    let eligible = state.db.get_rule_eligible_transactions().await?;
    tracing::info!(
        rules = compiled_rules.len(),
        eligible = eligible.len(),
        "applying categorization rules"
    );

    let mut categorized_count: u32 = 0;

    for txn in &eligible {
        if let Some(category_id) = evaluate_categorization_rules(txn, &compiled_rules) {
            tracing::debug!(
                txn_id = %txn.id,
                merchant = %txn.merchant_name,
                category_id = %category_id,
                "rule matched"
            );
            state
                .db
                .update_transaction_category(txn.id, category_id, CategoryMethod::Rule)
                .await?;
            categorized_count += 1;
        }
    }

    tracing::info!(categorized_count, "rule application complete");
    Ok(Json(ApplyRulesResponse { categorized_count }))
}
