use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use utoipa_axum::{router::OpenApiRouter, routes};

use budget_core::models::{
    Categorization, CategoryId, CategoryMethod, RuleType, Transaction, TransactionId,
};
use budget_core::rules::compile_rule;
use budget_jobs::CategorizeTransactionJob;
use budget_providers::RuleContext;

use crate::routes::AppError;
use crate::state::AppState;

/// Request body for categorizing a transaction.
#[derive(Deserialize, utoipa::ToSchema)]
pub struct CategorizeRequest {
    /// The category UUID to assign.
    pub category_id: CategoryId,
}

/// Build the transactions sub-router.
///
/// Mounts:
/// - `GET /` -- list all transactions
/// - `GET /{id}` -- fetch a single transaction
/// - `POST /{id}/categorize` -- assign a category to a transaction
/// - `POST /{id}/generate-rule` -- generate rule proposals for a transaction
///
/// # Errors
///
/// Individual handlers return `AppError` on failure.
pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list))
        .routes(routes!(get_one))
        .routes(routes!(categorize, uncategorize))
        .routes(routes!(generate_rule))
        .routes(routes!(skip_correlation))
}

/// Page size for transaction listing (1–200, default 50).
#[derive(Debug, Clone, Copy, Serialize, utoipa::ToSchema)]
#[schema(value_type = i64)]
#[serde(transparent)]
struct PageLimit(i64);

impl PageLimit {
    const DEFAULT: Self = Self(50);
    const MIN: i64 = 1;
    const MAX: i64 = 200;

    fn new(n: i64) -> Result<Self, String> {
        if !(Self::MIN..=Self::MAX).contains(&n) {
            return Err(format!(
                "limit must be between {} and {}, got {n}",
                Self::MIN,
                Self::MAX,
            ));
        }
        Ok(Self(n))
    }

    fn get(self) -> i64 {
        self.0
    }
}

impl Default for PageLimit {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl<'de> Deserialize<'de> for PageLimit {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let n = i64::deserialize(deserializer)?;
        Self::new(n).map_err(serde::de::Error::custom)
    }
}

/// Row offset for transaction listing (>= 0, default 0).
#[derive(Debug, Clone, Copy, Default, Serialize, utoipa::ToSchema)]
#[schema(value_type = i64)]
#[serde(transparent)]
struct PageOffset(i64);

impl PageOffset {
    fn new(n: i64) -> Result<Self, String> {
        if n < 0 {
            return Err(format!("offset must be non-negative, got {n}"));
        }
        Ok(Self(n))
    }

    fn get(self) -> i64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for PageOffset {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let n = i64::deserialize(deserializer)?;
        Self::new(n).map_err(serde::de::Error::custom)
    }
}

/// Query parameters for paginated transaction listing.
#[derive(Deserialize, utoipa::IntoParams)]
struct ListQuery {
    #[serde(default)]
    limit: PageLimit,
    #[serde(default)]
    offset: PageOffset,
    search: Option<String>,
    category_id: Option<String>,
    account_id: Option<String>,
    category_method: Option<String>,
}

/// Paginated response wrapper for transaction listings.
#[derive(Serialize, utoipa::ToSchema)]
struct TransactionPage {
    items: Vec<Transaction>,
    total: i64,
    limit: i64,
    offset: i64,
}

/// List transactions with pagination and optional filters.
///
/// Query parameters:
/// - `limit` — page size (default 50, max 200)
/// - `offset` — number of rows to skip (default 0)
/// - `search` — case-insensitive substring match on merchant or description
/// - `category_id` — filter by category UUID, or `__none` for uncategorized
/// - `account_id` — filter by account UUID
/// - `category_method` — filter by method (`manual`, `rule`, `llm`), or `__none`
///
/// # Errors
///
/// Returns `AppError` if the database query fails.
#[utoipa::path(get, path = "/", tag = "transactions", params(ListQuery), responses((status = 200, body = TransactionPage)), security(("bearer_token" = [])))]
async fn list(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Result<Json<TransactionPage>, AppError> {
    let limit = query.limit.get();
    let offset = query.offset.get();
    let search = query.search.as_deref().unwrap_or("");
    let category_id = query.category_id.as_deref().unwrap_or("");
    let account_id = query.account_id.as_deref().unwrap_or("");
    let category_method = query.category_method.as_deref().unwrap_or("");

    let (items, total) = state
        .db
        .list_transactions_paginated(
            limit,
            offset,
            search,
            category_id,
            account_id,
            category_method,
        )
        .await?;

    Ok(Json(TransactionPage {
        items,
        total,
        limit,
        offset,
    }))
}

/// Fetch a single transaction by ID.
///
/// # Errors
///
/// Returns 400 if the ID is not a valid UUID.
/// Returns 404 if no transaction exists with the given ID.
#[utoipa::path(get, path = "/{id}", tag = "transactions", params(("id" = TransactionId, Path, description = "Transaction UUID")), responses((status = 200, body = Transaction)), security(("bearer_token" = [])))]
async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<TransactionId>,
) -> Result<Json<Transaction>, AppError> {
    let txn = state
        .db
        .get_transaction_by_id(id)
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
#[utoipa::path(post, path = "/{id}/categorize", tag = "transactions", params(("id" = TransactionId, Path, description = "Transaction UUID")), request_body = CategorizeRequest, responses((status = 204)), security(("bearer_token" = [])))]
async fn categorize(
    State(state): State<AppState>,
    Path(id): Path<TransactionId>,
    Json(body): Json<CategorizeRequest>,
) -> Result<StatusCode, AppError> {
    if state.db.category_has_children(body.category_id).await? {
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            "Cannot assign transactions to a parent category; use a leaf category instead"
                .to_string(),
        ));
    }

    state
        .db
        .update_transaction_category(id, body.category_id, CategoryMethod::Manual, None)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Clear the category on a transaction so rules can re-evaluate it.
///
/// # Errors
///
/// Returns 400 if the transaction ID is not a valid UUID.
/// Returns `AppError` if the database update fails.
#[utoipa::path(delete, path = "/{id}/categorize", tag = "transactions", params(("id" = TransactionId, Path, description = "Transaction UUID")), responses((status = 204)), security(("bearer_token" = [])))]
async fn uncategorize(
    State(state): State<AppState>,
    Path(id): Path<TransactionId>,
) -> Result<StatusCode, AppError> {
    tracing::info!(txn_id = %id, "clearing category and re-enqueuing for categorization");
    state.db.clear_transaction_category(id).await?;
    state
        .categorize_txn_storage
        .push(CategorizeTransactionJob { transaction_id: id })
        .await
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Skip Correlation
// ---------------------------------------------------------------------------

/// Request body for the skip-correlation endpoint.
#[derive(Deserialize, utoipa::ToSchema)]
pub struct SkipCorrelationRequest {
    pub skip: bool,
}

/// Set or clear the `skip_correlation` flag on a transaction.
///
/// # Errors
///
/// Returns 400 if the ID is not a valid UUID.
/// Returns `AppError` if the database update fails.
#[utoipa::path(post, path = "/{id}/skip-correlation", tag = "transactions", params(("id" = TransactionId, Path, description = "Transaction UUID")), request_body = SkipCorrelationRequest, responses((status = 204)), security(("bearer_token" = [])))]
async fn skip_correlation(
    State(state): State<AppState>,
    Path(id): Path<TransactionId>,
    Json(body): Json<SkipCorrelationRequest>,
) -> Result<StatusCode, AppError> {
    state.db.set_skip_correlation(id, body.skip).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Generate Rule
// ---------------------------------------------------------------------------

#[derive(Serialize, utoipa::ToSchema)]
struct GenerateRuleProposal {
    match_field: String,
    match_pattern: String,
    explanation: String,
}

#[derive(Serialize, utoipa::ToSchema)]
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
#[utoipa::path(post, path = "/{id}/generate-rule", tag = "transactions", params(("id" = TransactionId, Path, description = "Transaction UUID")), responses((status = 200, body = GenerateRuleResponse)), security(("bearer_token" = [])))]
async fn generate_rule(
    State(state): State<AppState>,
    Path(id): Path<TransactionId>,
) -> Result<Json<GenerateRuleResponse>, AppError> {
    let txn = state
        .db
        .get_transaction_by_id(id)
        .await?
        .ok_or_else(|| AppError(StatusCode::NOT_FOUND, "transaction not found".to_owned()))?;

    let category_id = txn.categorization.category_id().ok_or_else(|| {
        AppError(
            StatusCode::BAD_REQUEST,
            "transaction is not categorized".to_owned(),
        )
    })?;

    if matches!(txn.categorization, Categorization::Rule(_)) {
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
        .filter(|r| r.target.category_id() == Some(category_id))
        .flat_map(|r| {
            r.conditions
                .iter()
                .map(|c| format!("{}: {}", c.field, c.pattern))
        })
        .collect();

    let amazon_titles = state
        .db
        .get_enrichment_item_titles_for_transactions(&[*txn.id.as_uuid()])
        .await?
        .remove(txn.id.as_uuid())
        .unwrap_or_default();

    let context = RuleContext {
        merchant_name: txn.merchant_name,
        remittance_information: txn.remittance_information,
        amount: txn.amount,
        posted_date: txn.posted_date,
        category_name: category.name.to_string(),
        sibling_merchants,
        existing_rule_patterns,
        counterparty_name: txn.counterparty_name,
        counterparty_iban: txn.counterparty_iban.map(|v| v.to_string()),
        counterparty_bic: txn.counterparty_bic.map(|v| v.to_string()),
        bank_transaction_code: txn.bank_transaction_code,
        enrichment_item_titles: amazon_titles,
    };

    let proposed = state
        .llm
        .propose_rules(&context)
        .await
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, format!("LLM error: {e}")))?;

    let proposals = validate_proposals(proposed, category_id);

    Ok(Json(GenerateRuleResponse {
        target_category_id: category_id.to_string(),
        category_name: category.name.to_string(),
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
            let test_rule = budget_core::models::Rule {
                id: budget_core::models::RuleId::new(),
                target: budget_core::models::RuleTarget::Categorization(category_id),
                conditions: vec![budget_core::models::RuleCondition {
                    field: p.match_field,
                    pattern: p.match_pattern.clone(),
                }],
                priority: budget_core::models::Priority::default(),
            };
            compile_rule(&test_rule).is_ok()
        })
        .map(|p| GenerateRuleProposal {
            match_field: p.match_field.to_string(),
            match_pattern: p.match_pattern,
            explanation: p.explanation,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_categorize_request() {
        let id = uuid::Uuid::new_v4();
        let json = format!(r#"{{"category_id": "{id}"}}"#);
        let req: CategorizeRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(*req.category_id.as_uuid(), id);
    }

    #[test]
    fn deserialize_categorize_request_rejects_invalid_uuid() {
        let json = r#"{"category_id": "not-a-uuid"}"#;
        assert!(serde_json::from_str::<CategorizeRequest>(json).is_err());
    }

    #[test]
    fn deserialize_categorize_request_rejects_missing_field() {
        let json = r"{}";
        assert!(serde_json::from_str::<CategorizeRequest>(json).is_err());
    }

    #[test]
    fn page_limit_defaults_to_50() {
        let q: ListQuery = serde_json::from_str(r"{}").unwrap();
        assert_eq!(q.limit.get(), 50);
    }

    #[test]
    fn page_limit_accepts_valid_range() {
        for n in [1, 50, 200] {
            let json = format!(r#"{{"limit": {n}}}"#);
            let q: ListQuery = serde_json::from_str(&json).unwrap();
            assert_eq!(q.limit.get(), n);
        }
    }

    #[test]
    fn page_limit_rejects_zero() {
        let json = r#"{"limit": 0}"#;
        assert!(serde_json::from_str::<ListQuery>(json).is_err());
    }

    #[test]
    fn page_limit_rejects_negative() {
        let json = r#"{"limit": -1}"#;
        assert!(serde_json::from_str::<ListQuery>(json).is_err());
    }

    #[test]
    fn page_limit_rejects_over_max() {
        let json = r#"{"limit": 201}"#;
        assert!(serde_json::from_str::<ListQuery>(json).is_err());
    }

    #[test]
    fn page_offset_defaults_to_0() {
        let q: ListQuery = serde_json::from_str(r"{}").unwrap();
        assert_eq!(q.offset.get(), 0);
    }

    #[test]
    fn page_offset_accepts_zero_and_positive() {
        for n in [0, 1, 1000] {
            let json = format!(r#"{{"offset": {n}}}"#);
            let q: ListQuery = serde_json::from_str(&json).unwrap();
            assert_eq!(q.offset.get(), n);
        }
    }

    #[test]
    fn page_offset_rejects_negative() {
        let json = r#"{"offset": -1}"#;
        assert!(serde_json::from_str::<ListQuery>(json).is_err());
    }
}
