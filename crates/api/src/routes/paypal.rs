use axum::Json;
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use budget_core::models::{PayPalAccount, PayPalAccountId};
use budget_db::PayPalEnrichmentStats;
use budget_jobs::PayPalSyncJob;
use budget_jobs::schedule_queries::{self, AccountType, RunStatus, ScheduleRun, TriggerReason};

use crate::routes::AppError;
use crate::state::AppState;

/// Build the `PayPal` sub-router.
pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_accounts, create_account))
        .routes(routes!(delete_account, update_account))
        .routes(routes!(account_status))
        .routes(routes!(trigger_sync))
        .routes(routes!(get_enrichment))
}

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

#[derive(Deserialize, utoipa::ToSchema)]
struct CreateAccountRequest {
    label: String,
    client_id: String,
    client_secret: String,
    #[serde(default)]
    sandbox: bool,
}

#[derive(Deserialize, utoipa::ToSchema)]
struct UpdateAccountRequest {
    label: String,
}

#[derive(Serialize, utoipa::ToSchema)]
struct AccountStatus {
    account: PayPalAccount,
    stats: PayPalEnrichmentStats,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// List all `PayPal` accounts (without secrets).
#[utoipa::path(get, path = "/accounts", tag = "paypal")]
async fn list_accounts(
    State(state): State<AppState>,
) -> Result<Json<Vec<PayPalAccount>>, AppError> {
    let accounts = state.db.list_paypal_accounts().await?;
    Ok(Json(accounts))
}

/// Create a new `PayPal` account with API credentials.
#[utoipa::path(post, path = "/accounts", tag = "paypal")]
async fn create_account(
    State(state): State<AppState>,
    Json(body): Json<CreateAccountRequest>,
) -> Result<(StatusCode, Json<PayPalAccount>), AppError> {
    let account = PayPalAccount {
        id: PayPalAccountId::new(),
        label: body.label,
        sandbox: body.sandbox,
    };

    state
        .db
        .insert_paypal_account(&account, &body.client_id, &body.client_secret)
        .await?;

    Ok((StatusCode::CREATED, Json(account)))
}

/// Delete a `PayPal` account. Cascades through transactions and matches.
#[utoipa::path(delete, path = "/accounts/{id}", tag = "paypal")]
async fn delete_account(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<Uuid>,
) -> Result<StatusCode, AppError> {
    state
        .db
        .delete_paypal_account(PayPalAccountId::from_uuid(id))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Update a `PayPal` account's label.
#[utoipa::path(patch, path = "/accounts/{id}", tag = "paypal")]
async fn update_account(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<Uuid>,
    Json(body): Json<UpdateAccountRequest>,
) -> Result<StatusCode, AppError> {
    state
        .db
        .update_paypal_account_label(PayPalAccountId::from_uuid(id), &body.label)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Get `PayPal` account status and enrichment stats.
#[utoipa::path(get, path = "/accounts/{id}/status", tag = "paypal")]
async fn account_status(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<Uuid>,
) -> Result<Json<AccountStatus>, AppError> {
    let account_id = PayPalAccountId::from_uuid(id);
    let account = state
        .db
        .get_paypal_account(account_id)
        .await?
        .ok_or_else(|| AppError(StatusCode::NOT_FOUND, "account not found".to_owned()))?;

    let enrichment_stats = state.db.paypal_enrichment_stats(account_id).await?;

    Ok(Json(AccountStatus {
        account,
        stats: enrichment_stats,
    }))
}

/// Trigger a `PayPal` sync for the given account.
#[utoipa::path(post, path = "/accounts/{id}/sync", tag = "paypal")]
async fn trigger_sync(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<Uuid>,
) -> Result<StatusCode, AppError> {
    let account_id = PayPalAccountId::from_uuid(id);

    state
        .db
        .get_paypal_account(account_id)
        .await?
        .ok_or_else(|| AppError(StatusCode::NOT_FOUND, "account not found".to_owned()))?;

    let run_id = Uuid::new_v4();
    let now = chrono::Utc::now();

    let run = ScheduleRun {
        id: run_id,
        account_id: id,
        account_type: AccountType::PayPal,
        status: RunStatus::Running,
        trigger_reason: TriggerReason::Manual,
        attempt: 1,
        started_at: Some(now),
        finished_at: None,
        next_run_at: None,
        error_message: None,
        created_at: now,
    };
    schedule_queries::insert_schedule_run(&state.apalis_pool, &run)
        .await
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    state
        .paypal_sync_storage
        .push(PayPalSyncJob {
            account_id,
            schedule_run_id: Some(run_id.to_string()),
        })
        .await
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;

    Ok(StatusCode::ACCEPTED)
}

// ---------------------------------------------------------------------------
// Enrichment
// ---------------------------------------------------------------------------

#[derive(Serialize, utoipa::ToSchema)]
struct PayPalEnrichmentResponse {
    bank_transaction_id: Uuid,
    merchant_name: Option<String>,
    items: Vec<budget_paypal::PayPalItem>,
}

/// Get `PayPal` enrichment details for a bank transaction.
#[utoipa::path(get, path = "/enrichment/{transaction_id}", tag = "paypal")]
async fn get_enrichment(
    State(state): State<AppState>,
    AxumPath(transaction_id): AxumPath<Uuid>,
) -> Result<Json<PayPalEnrichmentResponse>, AppError> {
    let titles = state
        .db
        .get_paypal_item_titles_for_transactions(&[transaction_id])
        .await?;

    let items_for_txn = titles.get(&transaction_id);
    if items_for_txn.is_none() {
        return Err(AppError(
            StatusCode::NOT_FOUND,
            "no PayPal enrichment for this transaction".to_owned(),
        ));
    }

    let item_titles = items_for_txn.unwrap_or(&Vec::new()).clone();

    Ok(Json(PayPalEnrichmentResponse {
        bank_transaction_id: transaction_id,
        merchant_name: item_titles.first().cloned(),
        items: item_titles
            .into_iter()
            .map(|name| budget_paypal::PayPalItem {
                name: Some(name),
                description: None,
                quantity: None,
                unit_price: None,
                unit_price_currency: None,
            })
            .collect(),
    }))
}
