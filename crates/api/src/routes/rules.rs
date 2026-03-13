use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use budget_core::models::{
    CategoryId, CategoryMethod, CorrelationType, MatchField, Priority, Rule, RuleCondition, RuleId,
    RuleType, TransactionId,
};
use budget_core::rules::{compile_rule, evaluate_categorization_rules, matches_rule};
use budget_jobs::CategorizeJob;

use crate::routes::AppError;
use crate::state::AppState;

/// A single condition in a rule request body.
#[derive(Deserialize)]
pub struct ConditionRequest {
    pub field: MatchField,
    pub pattern: String,
}

/// Request body for creating or updating a rule.
#[derive(Deserialize)]
pub struct CreateRule {
    pub rule_type: RuleType,
    pub conditions: Vec<ConditionRequest>,
    pub target_category_id: Option<CategoryId>,
    pub target_correlation_type: Option<CorrelationType>,
    pub priority: Priority,
}

/// Request body for previewing a rule, extending `CreateRule` with an optional
/// transaction ID to always include in the match set (even if it is no longer
/// rule-eligible, e.g. because the user just manually categorized it).
#[derive(Deserialize)]
pub struct PreviewRule {
    #[serde(flatten)]
    pub rule: CreateRule,
    pub include_transaction_id: Option<TransactionId>,
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
        .route("/preview", axum::routing::post(preview))
}

/// Convert a `CreateRule` body into a domain `Rule`, reusing the shared logic
/// for both create and update handlers.
fn into_rule(body: CreateRule, id: RuleId) -> Result<Rule, AppError> {
    if body.conditions.is_empty() {
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            "conditions must not be empty".to_owned(),
        ));
    }

    let conditions = body
        .conditions
        .into_iter()
        .map(|c| RuleCondition {
            field: c.field,
            pattern: c.pattern,
        })
        .collect();

    Ok(Rule {
        id,
        rule_type: body.rule_type,
        conditions,
        target_category_id: body.target_category_id,
        target_correlation_type: body.target_correlation_type,
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
    let rule = into_rule(body, RuleId::new())?;
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
    Path(id): Path<RuleId>,
    Json(body): Json<CreateRule>,
) -> Result<Json<Rule>, AppError> {
    let rule = into_rule(body, id)?;
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
    Path(id): Path<RuleId>,
) -> Result<StatusCode, AppError> {
    state.db.delete_rule(id).await?;
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

    let mut eligible = state.db.get_rule_eligible_transactions().await?;
    tracing::info!(
        rules = compiled_rules.len(),
        eligible = eligible.len(),
        "applying categorization rules"
    );

    let txn_ids: Vec<uuid::Uuid> = eligible.iter().map(|t| *t.id.as_uuid()).collect();
    let mut amazon_titles = state
        .db
        .get_amazon_item_titles_for_transactions(&txn_ids)
        .await?;
    for txn in &mut eligible {
        if let Some(titles) = amazon_titles.remove(txn.id.as_uuid()) {
            txn.amazon_item_titles = titles;
        }
    }

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
                .update_transaction_category(txn.id, category_id, CategoryMethod::Rule, None)
                .await?;
            categorized_count += 1;
        }
    }

    tracing::info!(categorized_count, "rule application complete");
    Ok(Json(ApplyRulesResponse { categorized_count }))
}

// ---------------------------------------------------------------------------
// Preview
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct PreviewMatch {
    id: String,
    merchant_name: String,
    posted_date: NaiveDate,
    amount: Decimal,
}

#[derive(Serialize)]
struct PreviewResponse {
    match_count: u32,
    sample: Vec<PreviewMatch>,
}

/// Dry-run a rule against eligible transactions and return the match count
/// plus a small sample of matching transactions.
///
/// # Errors
///
/// Returns 400 if the rule body is invalid or the pattern fails to compile.
/// Returns `AppError` on database failures.
async fn preview(
    State(state): State<AppState>,
    Json(body): Json<PreviewRule>,
) -> Result<Json<PreviewResponse>, AppError> {
    let include_id = body.include_transaction_id;
    let rule = into_rule(body.rule, RuleId::new())?;
    let compiled =
        compile_rule(&rule).map_err(|e| AppError(StatusCode::BAD_REQUEST, e.to_string()))?;

    let mut transactions = match rule.rule_type {
        RuleType::Categorization => state.db.get_rule_eligible_transactions().await?,
        RuleType::Correlation => state.db.get_uncorrelated_transactions().await?,
    };

    // Include a specific transaction even if it is not rule-eligible (e.g. it
    // was just manually categorized and the user is generating a rule from it).
    if let Some(include_id) = include_id
        && !transactions.iter().any(|t| t.id == include_id)
        && let Some(txn) = state.db.get_transaction_by_id(include_id).await?
    {
        transactions.push(txn);
    }

    // Enrich with Amazon item titles so AmazonItemTitle rules can match
    if rule.rule_type == RuleType::Categorization {
        let txn_ids: Vec<uuid::Uuid> = transactions.iter().map(|t| *t.id.as_uuid()).collect();
        let mut amazon_titles = state
            .db
            .get_amazon_item_titles_for_transactions(&txn_ids)
            .await?;
        for txn in &mut transactions {
            if let Some(titles) = amazon_titles.remove(txn.id.as_uuid()) {
                txn.amazon_item_titles = titles;
            }
        }
    }

    let mut match_count: u32 = 0;
    let mut sample = Vec::with_capacity(5);

    for txn in &transactions {
        if matches_rule(txn, &compiled) {
            match_count += 1;
            if sample.len() < 5 {
                sample.push(PreviewMatch {
                    id: txn.id.to_string(),
                    merchant_name: txn.merchant_name.clone(),
                    posted_date: txn.posted_date,
                    amount: txn.amount,
                });
            }
        }
    }

    Ok(Json(PreviewResponse {
        match_count,
        sample,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_create_rule_categorization() {
        let id = uuid::Uuid::new_v4();
        let json = format!(
            r#"{{
                "rule_type": "categorization",
                "conditions": [{{"field": "merchant", "pattern": "^ALDI"}}],
                "target_category_id": "{id}",
                "target_correlation_type": null,
                "priority": 10
            }}"#,
        );
        let rule: CreateRule = serde_json::from_str(&json).unwrap();
        assert_eq!(rule.rule_type, RuleType::Categorization);
        assert_eq!(rule.conditions[0].field, MatchField::Merchant);
        assert_eq!(*rule.target_category_id.unwrap().as_uuid(), id);
        assert!(rule.target_correlation_type.is_none());
    }

    #[test]
    fn deserialize_create_rule_correlation() {
        let json = r#"{
            "rule_type": "correlation",
            "conditions": [{"field": "counterparty_iban", "pattern": "^DE"}],
            "target_category_id": null,
            "target_correlation_type": "transfer",
            "priority": 5
        }"#;
        let rule: CreateRule = serde_json::from_str(json).unwrap();
        assert_eq!(rule.rule_type, RuleType::Correlation);
        assert_eq!(
            rule.target_correlation_type,
            Some(CorrelationType::Transfer)
        );
    }

    #[test]
    fn deserialize_create_rule_rejects_invalid_rule_type() {
        let json = r#"{
            "rule_type": "bogus",
            "conditions": [{"field": "merchant", "pattern": "x"}],
            "priority": 0
        }"#;
        assert!(serde_json::from_str::<CreateRule>(json).is_err());
    }

    #[test]
    fn deserialize_create_rule_rejects_invalid_match_field() {
        let json = r#"{
            "rule_type": "categorization",
            "conditions": [{"field": "nonexistent", "pattern": "x"}],
            "priority": 0
        }"#;
        assert!(serde_json::from_str::<CreateRule>(json).is_err());
    }

    #[test]
    fn deserialize_create_rule_rejects_invalid_category_id() {
        let json = r#"{
            "rule_type": "categorization",
            "conditions": [{"field": "merchant", "pattern": "x"}],
            "target_category_id": "not-a-uuid",
            "priority": 0
        }"#;
        assert!(serde_json::from_str::<CreateRule>(json).is_err());
    }

    #[test]
    fn deserialize_create_rule_rejects_invalid_correlation_type() {
        let json = r#"{
            "rule_type": "correlation",
            "conditions": [{"field": "merchant", "pattern": "x"}],
            "target_correlation_type": "invalid",
            "priority": 0
        }"#;
        assert!(serde_json::from_str::<CreateRule>(json).is_err());
    }

    #[test]
    fn deserialize_all_match_fields() {
        for (name, expected) in [
            ("merchant", MatchField::Merchant),
            ("description", MatchField::Description),
            ("amount_range", MatchField::AmountRange),
            ("counterparty_name", MatchField::CounterpartyName),
            ("counterparty_iban", MatchField::CounterpartyIban),
            ("counterparty_bic", MatchField::CounterpartyBic),
            ("bank_transaction_code", MatchField::BankTransactionCode),
            ("amazon_item_title", MatchField::AmazonItemTitle),
        ] {
            let json = format!(
                r#"{{"rule_type":"categorization","conditions":[{{"field":"{name}","pattern":"x"}}],"priority":0}}"#,
            );
            let rule: CreateRule = serde_json::from_str(&json).unwrap();
            assert_eq!(rule.conditions[0].field, expected);
        }
    }

    #[test]
    fn deserialize_preview_rule_with_include_id() {
        let txn_id = uuid::Uuid::new_v4();
        let json = format!(
            r#"{{
                "rule_type": "categorization",
                "conditions": [{{"field": "merchant", "pattern": "x"}}],
                "priority": 0,
                "include_transaction_id": "{txn_id}"
            }}"#,
        );
        let preview: PreviewRule = serde_json::from_str(&json).unwrap();
        assert_eq!(*preview.include_transaction_id.unwrap().as_uuid(), txn_id);
    }

    #[test]
    fn into_rule_rejects_empty_conditions() {
        let body = CreateRule {
            rule_type: RuleType::Categorization,
            conditions: vec![],
            target_category_id: None,
            target_correlation_type: None,
            priority: Priority::new(0).unwrap(),
        };
        assert!(into_rule(body, RuleId::new()).is_err());
    }
}
