use std::collections::HashMap;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use budget_core::models::{
    CategoryId, CategoryMethod, CorrelationType, MatchField, Rule, RuleId, RuleType,
};
use budget_core::rules::{compile_rule_pattern, evaluate_categorization_rules};

use crate::routes::AppError;
use crate::state::AppState;

/// Request body for creating or updating a rule.
#[derive(Deserialize)]
pub struct CreateRule {
    /// Rule type: "categorization" or "correlation".
    pub rule_type: String,
    /// Field to match against: "merchant", "description", or "`amount_range`".
    pub match_field: String,
    /// Pattern string (exact match, regex, or range expression).
    pub match_pattern: String,
    /// Target category UUID (for categorization rules).
    pub target_category_id: Option<String>,
    /// Target correlation type: "transfer" or "reimbursement".
    pub target_correlation_type: Option<String>,
    /// Higher-priority rules are evaluated first.
    pub priority: i32,
}

/// Build the rules sub-router.
///
/// Mounts:
/// - `GET /` -- list all rules
/// - `POST /` -- create a new rule
/// - `PUT /{id}` -- update an existing rule
/// - `DELETE /{id}` -- delete a rule
///
/// # Errors
///
/// Individual handlers return `AppError` on failure.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", axum::routing::put(update).delete(remove))
        .route("/generate", axum::routing::post(generate))
        .route("/apply", axum::routing::post(apply))
}

/// Parse a `CreateRule` body into domain types, reusing the shared parsing
/// logic for both create and update handlers.
fn parse_rule_body(body: &CreateRule, id: RuleId) -> Result<Rule, AppError> {
    let rule_type: RuleType = body
        .rule_type
        .parse()
        .map_err(|e: budget_core::error::Error| AppError(StatusCode::BAD_REQUEST, e.to_string()))?;

    let match_field: MatchField = body
        .match_field
        .parse()
        .map_err(|e: budget_core::error::Error| AppError(StatusCode::BAD_REQUEST, e.to_string()))?;

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
        match_field,
        match_pattern: body.match_pattern.clone(),
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
// Generate / Apply
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct SampleMatch {
    id: String,
    merchant_name: String,
    amount: Decimal,
    posted_date: NaiveDate,
}

#[derive(Serialize)]
struct RuleProposal {
    match_field: String,
    match_pattern: String,
    target_category_id: String,
    category_name: String,
    explanation: String,
    merchant_examples: Vec<String>,
    matches_count: usize,
    sample_matches: Vec<SampleMatch>,
}

#[derive(Serialize)]
struct GenerateRulesResponse {
    proposals: Vec<RuleProposal>,
    analyzed_groups: usize,
    filtered_by_existing_rules: usize,
}

/// Batch-generate rule proposals from categorized transactions.
///
/// Groups categorized transactions by (category, merchant), filters out
/// merchants already covered by existing rules, then asks the LLM to propose
/// a regex for each remaining cluster.
///
/// # Errors
///
/// Returns `AppError` on database or LLM failures.
async fn generate(State(state): State<AppState>) -> Result<Json<GenerateRulesResponse>, AppError> {
    let groups = state.db.get_categorized_merchant_groups().await?;
    let analyzed_groups = groups.len();
    tracing::info!(
        analyzed_groups,
        "rule generation: loaded categorized merchant groups"
    );
    for (category_id, merchant_name, count) in &groups {
        tracing::debug!(%category_id, %merchant_name, count, "  group");
    }

    // Load and compile existing categorization rules
    let raw_rules = state
        .db
        .list_rules_by_type(RuleType::Categorization)
        .await?;
    tracing::info!(
        raw_rules = raw_rules.len(),
        "rule generation: loaded existing categorization rules"
    );
    let compiled_rules: Vec<_> = raw_rules
        .iter()
        .filter_map(|rule| compile_rule_pattern(rule).ok())
        .collect();

    // Cluster merchants by category, filtering out those already matched
    let mut filtered_count: usize = 0;
    let mut clusters: HashMap<CategoryId, Vec<String>> = HashMap::new();

    for (category_id, merchant_name, _count) in &groups {
        // Check if any existing rule already matches this merchant
        let already_covered = compiled_rules.iter().any(|compiled| {
            if let budget_core::rules::CompiledPattern::Regex(ref regex) = compiled.pattern
                && compiled.rule.match_field == MatchField::Merchant
            {
                return regex.is_match(merchant_name);
            }
            false
        });

        if already_covered {
            tracing::debug!(%merchant_name, %category_id, "  filtered: already covered by existing rule");
            filtered_count += 1;
            continue;
        }

        clusters
            .entry(*category_id)
            .or_default()
            .push(merchant_name.clone());
    }

    tracing::info!(
        clusters = clusters.len(),
        filtered = filtered_count,
        "rule generation: clustered merchants"
    );

    // Load categories for name lookup
    let categories = state.db.list_categories().await?;
    let cat_map: HashMap<CategoryId, String> =
        categories.into_iter().map(|c| (c.id, c.name)).collect();

    // Load uncategorized transactions once for match previews
    let uncategorized = state.db.get_uncategorized_transactions().await?;
    tracing::info!(
        uncategorized = uncategorized.len(),
        "rule generation: loaded uncategorized transactions for match preview"
    );

    let mut proposals = Vec::new();

    for (category_id, merchants) in &clusters {
        let category_name = match cat_map.get(category_id) {
            Some(name) => name.clone(),
            None => {
                tracing::warn!(%category_id, "rule generation: skipping cluster, category not found");
                continue;
            }
        };

        tracing::info!(
            %category_id,
            %category_name,
            merchant_count = merchants.len(),
            "rule generation: requesting LLM proposal"
        );

        let proposed = state
            .llm
            .propose_rule(merchants, &category_name)
            .await
            .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, format!("LLM error: {e}")))?;

        tracing::info!(
            match_field = ?proposed.match_field,
            match_pattern = %proposed.match_pattern,
            "rule generation: LLM proposed pattern"
        );

        // Validate the proposed regex compiles by building a temporary Rule
        let match_field_domain = match proposed.match_field {
            budget_providers::MatchField::Merchant => MatchField::Merchant,
            budget_providers::MatchField::Description => MatchField::Description,
        };
        let test_rule = Rule {
            id: RuleId::new(),
            rule_type: RuleType::Categorization,
            match_field: match_field_domain,
            match_pattern: proposed.match_pattern.clone(),
            target_category_id: Some(*category_id),
            target_correlation_type: None,
            priority: 0,
        };
        let Ok(compiled) = compile_rule_pattern(&test_rule) else {
            tracing::warn!(
                pattern = %proposed.match_pattern,
                "rule generation: skipping proposal, pattern failed to compile"
            );
            continue;
        };

        // Count and sample matches among uncategorized transactions
        let matching: Vec<_> = uncategorized
            .iter()
            .filter(|txn| {
                evaluate_categorization_rules(txn, std::slice::from_ref(&compiled)).is_some()
            })
            .collect();

        tracing::info!(
            %category_name,
            pattern = %proposed.match_pattern,
            matches = matching.len(),
            "rule generation: proposal ready"
        );

        let sample_matches: Vec<SampleMatch> = matching
            .iter()
            .take(5)
            .map(|txn| SampleMatch {
                id: txn.id.to_string(),
                merchant_name: txn.merchant_name.clone(),
                amount: txn.amount,
                posted_date: txn.posted_date,
            })
            .collect();

        proposals.push(RuleProposal {
            match_field: match_field_domain.to_string(),
            match_pattern: proposed.match_pattern,
            target_category_id: category_id.to_string(),
            category_name,
            explanation: proposed.explanation,
            merchant_examples: merchants.clone(),
            matches_count: matching.len(),
            sample_matches,
        });
    }

    tracing::info!(
        proposals = proposals.len(),
        analyzed_groups,
        filtered = filtered_count,
        "rule generation: complete"
    );

    Ok(Json(GenerateRulesResponse {
        proposals,
        analyzed_groups,
        filtered_by_existing_rules: filtered_count,
    }))
}

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
        .filter_map(|rule| match compile_rule_pattern(rule) {
            Ok(compiled) => Some(compiled),
            Err(e) => {
                tracing::warn!(rule_id = %rule.id, error = %e, "skipping rule with invalid pattern");
                None
            }
        })
        .collect();

    let uncategorized = state.db.get_uncategorized_transactions().await?;
    let mut categorized_count: u32 = 0;

    for txn in &uncategorized {
        if let Some(category_id) = evaluate_categorization_rules(txn, &compiled_rules) {
            state
                .db
                .update_transaction_category(txn.id, category_id, CategoryMethod::Rule)
                .await?;
            categorized_count += 1;
        }
    }

    Ok(Json(ApplyRulesResponse { categorized_count }))
}
