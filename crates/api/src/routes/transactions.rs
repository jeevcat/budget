use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use budget_core::models::{CategoryId, CategoryMethod, RuleType, Transaction, TransactionId};
use budget_core::rules::compile_rule;
use budget_jobs::CategorizeJob;
use budget_providers::{MatchField, RuleContext};

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
/// - `GET /{id}` -- fetch a single transaction
/// - `POST /{id}/categorize` -- assign a category to a transaction
/// - `POST /{id}/generate-rule` -- generate rule proposals for a transaction
///
/// # Errors
///
/// Individual handlers return `AppError` on failure.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list))
        .route("/uncategorized", get(uncategorized))
        .route("/{id}", get(get_one))
        .route("/{id}/categorize", post(categorize).delete(uncategorize))
        .route("/{id}/generate-rule", post(generate_rule))
        .route("/{id}/skip-correlation", post(skip_correlation))
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

/// Fetch a single transaction by ID.
///
/// # Errors
///
/// Returns 400 if the ID is not a valid UUID.
/// Returns 404 if no transaction exists with the given ID.
async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Transaction>, AppError> {
    let uuid =
        Uuid::parse_str(&id).map_err(|e| AppError(StatusCode::BAD_REQUEST, e.to_string()))?;
    let txn = state
        .db
        .get_transaction_by_id(TransactionId::from_uuid(uuid))
        .await?
        .ok_or_else(|| AppError(StatusCode::NOT_FOUND, "transaction not found".to_owned()))?;
    Ok(Json(txn))
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
        .update_transaction_category(txn_id, category_id, CategoryMethod::Manual, None)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Clear the category on a transaction so rules can re-evaluate it.
///
/// # Errors
///
/// Returns 400 if the transaction ID is not a valid UUID.
/// Returns `AppError` if the database update fails.
async fn uncategorize(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let txn_uuid =
        Uuid::parse_str(&id).map_err(|e| AppError(StatusCode::BAD_REQUEST, e.to_string()))?;
    let txn_id = TransactionId::from_uuid(txn_uuid);

    state.db.clear_transaction_category(txn_id).await?;
    state
        .categorize_storage
        .push(CategorizeJob)
        .await
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Skip Correlation
// ---------------------------------------------------------------------------

/// Request body for the skip-correlation endpoint.
#[derive(Deserialize)]
pub struct SkipCorrelationRequest {
    pub skip: bool,
}

/// Set or clear the `skip_correlation` flag on a transaction.
///
/// # Errors
///
/// Returns 400 if the ID is not a valid UUID.
/// Returns `AppError` if the database update fails.
async fn skip_correlation(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<SkipCorrelationRequest>,
) -> Result<StatusCode, AppError> {
    let txn_uuid =
        Uuid::parse_str(&id).map_err(|e| AppError(StatusCode::BAD_REQUEST, e.to_string()))?;
    let txn_id = TransactionId::from_uuid(txn_uuid);

    state.db.set_skip_correlation(txn_id, body.skip).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Generate Rule
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct GenerateRuleProposal {
    match_field: String,
    match_pattern: String,
    explanation: String,
}

#[derive(Serialize)]
struct GenerateRuleResponse {
    target_category_id: String,
    category_name: String,
    proposals: Vec<GenerateRuleProposal>,
}

/// Generate rule proposals for a single categorized transaction.
///
/// The transaction must have a category assigned via a method other than
/// "rule" (i.e. manual or LLM). Returns 3 pattern suggestions at varying
/// specificity.
///
/// # Errors
///
/// Returns 404 if the transaction does not exist.
/// Returns 400 if the transaction is uncategorized or was categorized by a rule.
/// Returns `AppError` on database or LLM failures.
async fn generate_rule(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<GenerateRuleResponse>, AppError> {
    let txn_uuid =
        Uuid::parse_str(&id).map_err(|e| AppError(StatusCode::BAD_REQUEST, e.to_string()))?;
    let txn_id = TransactionId::from_uuid(txn_uuid);

    let txn = state
        .db
        .get_transaction_by_id(txn_id)
        .await?
        .ok_or_else(|| AppError(StatusCode::NOT_FOUND, "transaction not found".to_owned()))?;

    let category_id = txn.category_id.ok_or_else(|| {
        AppError(
            StatusCode::BAD_REQUEST,
            "transaction is not categorized".to_owned(),
        )
    })?;

    if txn.category_method == Some(CategoryMethod::Rule) {
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            "transaction was categorized by a rule".to_owned(),
        ));
    }

    let category = state
        .db
        .get_category(category_id)
        .await?
        .ok_or_else(|| AppError(StatusCode::NOT_FOUND, "category not found".to_owned()))?;

    let sibling_merchants = state.db.get_sibling_merchants(category_id).await?;

    let existing_rules = state
        .db
        .list_rules_by_type(RuleType::Categorization)
        .await?;
    let existing_rule_patterns: Vec<String> = existing_rules
        .iter()
        .filter(|r| r.target_category_id == Some(category_id))
        .flat_map(|r| {
            r.conditions
                .iter()
                .map(|c| format!("{}: {}", c.field, c.pattern))
        })
        .collect();

    let context = RuleContext {
        merchant_name: txn.merchant_name,
        description: txn.description,
        amount: txn.amount,
        posted_date: txn.posted_date,
        category_name: category.name.clone(),
        sibling_merchants,
        existing_rule_patterns,
        counterparty_name: txn.counterparty_name,
        counterparty_iban: txn.counterparty_iban,
        counterparty_bic: txn.counterparty_bic,
        bank_transaction_code: txn.bank_transaction_code,
    };

    let proposed = state
        .llm
        .propose_rules(&context)
        .await
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, format!("LLM error: {e}")))?;

    let proposals = validate_proposals(proposed, category_id);

    Ok(Json(GenerateRuleResponse {
        target_category_id: category_id.to_string(),
        category_name: category.name,
        proposals,
    }))
}

/// Validate proposed rules by compiling their patterns, then convert to API
/// response format. Proposals with invalid patterns are silently dropped.
fn validate_proposals(
    proposed: Vec<budget_providers::ProposedRule>,
    category_id: CategoryId,
) -> Vec<GenerateRuleProposal> {
    proposed
        .into_iter()
        .filter(|p| {
            let match_field_domain = match p.match_field {
                MatchField::Merchant => budget_core::models::MatchField::Merchant,
                MatchField::Description => budget_core::models::MatchField::Description,
                MatchField::CounterpartyName => budget_core::models::MatchField::CounterpartyName,
                MatchField::CounterpartyIban => budget_core::models::MatchField::CounterpartyIban,
                MatchField::CounterpartyBic => budget_core::models::MatchField::CounterpartyBic,
                MatchField::BankTransactionCode => {
                    budget_core::models::MatchField::BankTransactionCode
                }
            };
            let test_rule = budget_core::models::Rule {
                id: budget_core::models::RuleId::new(),
                rule_type: RuleType::Categorization,
                conditions: vec![budget_core::models::RuleCondition {
                    field: match_field_domain,
                    pattern: p.match_pattern.clone(),
                }],
                target_category_id: Some(category_id),
                target_correlation_type: None,
                priority: 0,
            };
            compile_rule(&test_rule).is_ok()
        })
        .map(|p| GenerateRuleProposal {
            match_field: match p.match_field {
                MatchField::Merchant => "merchant".to_owned(),
                MatchField::Description => "description".to_owned(),
                MatchField::CounterpartyName => "counterparty_name".to_owned(),
                MatchField::CounterpartyIban => "counterparty_iban".to_owned(),
                MatchField::CounterpartyBic => "counterparty_bic".to_owned(),
                MatchField::BankTransactionCode => "bank_transaction_code".to_owned(),
            },
            match_pattern: p.match_pattern,
            explanation: p.explanation,
        })
        .collect()
}
