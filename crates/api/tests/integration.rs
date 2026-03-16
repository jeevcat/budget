use std::num::NonZeroU32;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::middleware;
use sqlx::PgPool;
use tower::ServiceExt;
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_scalar::Servable;

use api::auth;
use api::openapi::ApiDoc;
use api::routes;
use api::state::AppState;
use budget_jobs::{JobStorage, PipelineStorage};

use budget_core::models::{
    Account, AccountId, AccountOrigin, AccountType, AmazonAccount, AmazonAccountId,
    BalanceSnapshot, BalanceSnapshotId, BudgetConfig, BudgetType, Categorization, Category,
    CategoryId, CategoryName, Connection, ConnectionId, ConnectionStatus, CurrencyCode, Priority,
    Rule, RuleCondition, RuleTarget, SecretKey, Transaction,
};
use budget_db::Db;

/// Mirror of the paginated response from `GET /api/transactions`.
#[derive(serde::Deserialize)]
struct TransactionPage {
    items: Vec<Transaction>,
    total: i64,
}
use budget_jobs::LlmClient;
use budget_providers::MockLlmProvider;

/// Bearer token used in all test requests.
const TEST_SECRET: &str = "test-secret-key";

// ---------------------------------------------------------------------------
// Shared test setup
// ---------------------------------------------------------------------------

/// Wrap a `sqlx::test`-provided `PgPool` in the full application `Router`
/// with apalis tables and domain migrations applied.
async fn setup(pool: PgPool) -> (Router, Db) {
    let db = Db::from_pool(pool.clone());
    budget_jobs::setup_test_storage(&pool)
        .await
        .expect("apalis setup");
    db.run_migrations().await.expect("domain migrations");

    let state = AppState {
        db: db.clone(),
        secret_key: SecretKey::new(TEST_SECRET).expect("valid test secret"),
        sync_storage: JobStorage::new(&pool),
        categorize_storage: JobStorage::new(&pool),
        categorize_txn_storage: JobStorage::new(&pool),
        correlate_storage: JobStorage::new(&pool),
        pipeline_storage: PipelineStorage::new(&pool),
        amazon_sync_storage: JobStorage::new(&pool),
        apalis_pool: pool,
        enable_banking_auth: None,
        llm: LlmClient::new(MockLlmProvider::new()),
        expected_salary_count: NonZeroU32::new(1).expect("1 is non-zero"),
        host: "http://localhost:3000".to_owned(),
        amazon_config: api::routes::amazon::AmazonConfig {
            cookies_dir: std::path::PathBuf::from("test-amazon-cookies"),
        },
    };

    let (api_routes, openapi) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .nest(
            "/accounts",
            routes::accounts::router().merge(routes::import::router()),
        )
        .nest("/transactions", routes::transactions::router())
        .nest("/categories", routes::categories::router())
        .nest("/rules", routes::rules::router())
        .nest("/budgets", routes::budgets::router())
        .nest("/jobs", routes::jobs::router())
        .nest("/connections", routes::connections::router())
        .nest("/amazon", routes::amazon::router())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_bearer_token,
        ))
        .split_for_parts();

    let app = Router::new()
        .route("/health", axum::routing::get(health))
        .merge(routes::connections::callback_router())
        .nest("/api", auth::router().merge(api_routes))
        .merge(utoipa_scalar::Scalar::with_url("/api/docs", openapi))
        .with_state(state);

    (app, db)
}

/// Health check handler (mirrors main.rs).
async fn health() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({"status": "ok"}))
}

/// Helper: send a request and return status + body bytes.
async fn send(app: Router, request: Request<Body>) -> (StatusCode, Vec<u8>) {
    let response = app.oneshot(request).await.expect("oneshot");
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read body");
    (status, body.to_vec())
}

/// Helper: build a JSON POST request with bearer token.
fn post_json(uri: &str, json: &serde_json::Value) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .method("POST")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {TEST_SECRET}"))
        .body(Body::from(serde_json::to_vec(json).expect("serialize")))
        .expect("build request")
}

/// Helper: build a JSON PUT request with bearer token.
fn put_json(uri: &str, json: &serde_json::Value) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .method("PUT")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {TEST_SECRET}"))
        .body(Body::from(serde_json::to_vec(json).expect("serialize")))
        .expect("build request")
}

/// Helper: build a GET request with bearer token.
fn get(uri: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .method("GET")
        .header("authorization", format!("Bearer {TEST_SECRET}"))
        .body(Body::empty())
        .expect("build request")
}

/// Helper: build a DELETE request with bearer token.
fn delete(uri: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .method("DELETE")
        .header("authorization", format!("Bearer {TEST_SECRET}"))
        .body(Body::empty())
        .expect("build request")
}

/// Helper: build a POST request with empty body and bearer token.
fn post_empty(uri: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .method("POST")
        .header("authorization", format!("Bearer {TEST_SECRET}"))
        .body(Body::empty())
        .expect("build request")
}

/// Helper: build an uncategorized transaction with the given merchant and account.
fn make_uncategorized_txn(account_id: AccountId, merchant: &str, day: u32) -> Transaction {
    Transaction {
        account_id,
        amount: rust_decimal::Decimal::new(1000, 2),
        merchant_name: merchant.to_owned(),
        posted_date: chrono::NaiveDate::from_ymd_opt(2025, 1, day).expect("date"),
        ..Default::default()
    }
}

/// Helper: build a category with no budget config.
fn make_category(name: &str) -> Category {
    Category {
        id: CategoryId::new(),
        name: CategoryName::new(name).expect("valid test category name"),
        parent_id: None,
        budget: budget_core::models::BudgetConfig::None,
    }
}

/// Helper: build a basic transaction.
fn make_txn(account_id: AccountId, merchant: &str, day: u32) -> Transaction {
    Transaction {
        account_id,
        amount: rust_decimal::Decimal::new(1000, 2),
        merchant_name: merchant.to_owned(),
        posted_date: chrono::NaiveDate::from_ymd_opt(2025, 1, day).expect("date"),
        ..Default::default()
    }
}

/// Helper: build a JSON PATCH request with bearer token.
fn patch_json(uri: &str, json: &serde_json::Value) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .method("PATCH")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {TEST_SECRET}"))
        .body(Body::from(serde_json::to_vec(json).expect("serialize")))
        .expect("build request")
}

/// Helper: build a request with text body and bearer token.
fn post_text(uri: &str, text: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .method("POST")
        .header("content-type", "text/plain")
        .header("authorization", format!("Bearer {TEST_SECRET}"))
        .body(Body::from(text.to_owned()))
        .expect("build request")
}

/// Helper: build an account fixture.
fn make_account(name: &str) -> Account {
    Account {
        id: AccountId::new(),
        provider_account_id: format!("prov-{}", uuid::Uuid::new_v4()),
        name: name.to_owned(),
        nickname: None,
        institution: "Test Bank".to_owned(),
        account_type: AccountType::Checking,
        currency: CurrencyCode::new("EUR").unwrap(),
        origin: AccountOrigin::Manual,
    }
}

/// Helper: build a GET request without any auth header.
fn get_unauthenticated(uri: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .method("GET")
        .body(Body::empty())
        .expect("build request")
}

// ===========================================================================
// Accounts
// ===========================================================================

#[sqlx::test]
async fn accounts_list_empty(pool: PgPool) {
    let (app, _db) = setup(pool).await;
    let (status, body) = send(app, get("/api/accounts")).await;

    assert_eq!(status, StatusCode::OK);
    let accounts: Vec<Account> = serde_json::from_slice(&body).expect("parse");
    assert!(accounts.is_empty());
}

#[sqlx::test]
async fn accounts_create_and_list(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    let payload = serde_json::json!({
        "provider_account_id": "prov-123",
        "name": "My Checking",
        "institution": "Test Bank",
        "account_type": "checking",
        "currency": "USD"
    });

    // Clone the router so we can reuse it (Router is Clone)
    let (status, body) = send(app.clone(), post_json("/api/accounts", &payload)).await;
    assert_eq!(status, StatusCode::CREATED);

    let created: Account = serde_json::from_slice(&body).expect("parse created");
    assert_eq!(created.name, "My Checking");
    assert_eq!(created.institution, "Test Bank");
    assert_eq!(created.account_type, AccountType::Checking);
    assert_eq!(created.currency, "USD");

    // List should now contain one account
    let (status, body) = send(app, get("/api/accounts")).await;
    assert_eq!(status, StatusCode::OK);

    let accounts: Vec<Account> = serde_json::from_slice(&body).expect("parse list");
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].name, "My Checking");
}

#[sqlx::test]
async fn accounts_get_nonexistent_returns_404(pool: PgPool) {
    let (app, _db) = setup(pool).await;
    let fake_id = uuid::Uuid::new_v4();

    let (status, _body) = send(app, get(&format!("/api/accounts/{fake_id}"))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn accounts_get_after_creation(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    let payload = serde_json::json!({
        "provider_account_id": "prov-456",
        "name": "Savings Account",
        "institution": "Test Bank",
        "account_type": "savings",
        "currency": "EUR"
    });

    let (status, body) = send(app.clone(), post_json("/api/accounts", &payload)).await;
    assert_eq!(status, StatusCode::CREATED);

    let created: Account = serde_json::from_slice(&body).expect("parse");
    let id = created.id;

    let (status, body) = send(app, get(&format!("/api/accounts/{id}"))).await;
    assert_eq!(status, StatusCode::OK);

    let fetched: Account = serde_json::from_slice(&body).expect("parse");
    assert_eq!(fetched.id, id);
    assert_eq!(fetched.name, "Savings Account");
    assert_eq!(fetched.account_type, AccountType::Savings);
}

#[sqlx::test]
async fn accounts_create_invalid_account_type_returns_422(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    let payload = serde_json::json!({
        "provider_account_id": "prov-789",
        "name": "Bad Account",
        "institution": "Test Bank",
        "account_type": "nonexistent_type",
        "currency": "USD"
    });

    let (status, _body) = send(app, post_json("/api/accounts", &payload)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

// ===========================================================================
// Transactions
// ===========================================================================

#[sqlx::test]
async fn transactions_list_empty(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    let (status, body) = send(app, get("/api/transactions")).await;
    assert_eq!(status, StatusCode::OK);

    let page: TransactionPage = serde_json::from_slice(&body).expect("parse");
    assert!(page.items.is_empty());
    assert_eq!(page.total, 0);
}

#[sqlx::test]
async fn transactions_categorize_success(pool: PgPool) {
    let (app, db) = setup(pool).await;

    let account = Account {
        id: AccountId::new(),
        provider_account_id: "prov-cat-test".to_owned(),
        name: "Cat Test".to_owned(),
        nickname: None,
        institution: "Bank".to_owned(),
        account_type: AccountType::Checking,
        currency: CurrencyCode::new("USD").unwrap(),
        origin: AccountOrigin::Manual,
    };
    db.upsert_account(&account).await.expect("account");

    let category = make_category("Dining");
    db.insert_category(&category).await.expect("category");

    let mut txn = make_txn(account.id, "Restaurant", 1);
    txn.amount = rust_decimal::Decimal::new(1500, 2);
    txn.remittance_information = vec!["Dinner".to_owned()];
    txn.posted_date = chrono::NaiveDate::from_ymd_opt(2025, 2, 1).expect("date");
    db.upsert_transaction(&txn, Some("txn-to-categorize"))
        .await
        .expect("insert");

    let payload = serde_json::json!({
        "category_id": category.id.to_string()
    });

    let (status, _body) = send(
        app,
        post_json(
            &format!("/api/transactions/{}/categorize", txn.id),
            &payload,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify via DB that the category was set
    let updated_txns = db.list_transactions().await.expect("list");
    let updated = updated_txns
        .iter()
        .find(|t| t.id == txn.id)
        .expect("find txn");
    assert_eq!(updated.categorization.category_id(), Some(category.id));
}

#[sqlx::test]
async fn transactions_categorize_invalid_uuid_returns_422(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    let payload = serde_json::json!({
        "category_id": "not-a-uuid"
    });

    // Transaction ID is valid UUID but category_id is not
    let fake_txn_id = uuid::Uuid::new_v4();
    let (status, _body) = send(
        app,
        post_json(
            &format!("/api/transactions/{fake_txn_id}/categorize"),
            &payload,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

// ===========================================================================
// Categories
// ===========================================================================

#[sqlx::test]
async fn categories_create_and_list(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    let payload = serde_json::json!({
        "name": "Entertainment"
    });

    let (status, body) = send(app.clone(), post_json("/api/categories", &payload)).await;
    assert_eq!(status, StatusCode::CREATED);

    let created: Category = serde_json::from_slice(&body).expect("parse");
    assert_eq!(created.name, "Entertainment");
    assert!(created.parent_id.is_none());
    assert!(created.budget.mode().is_none());

    // List should contain the created category
    let (status, body) = send(app, get("/api/categories")).await;
    assert_eq!(status, StatusCode::OK);

    let cats: Vec<Category> = serde_json::from_slice(&body).expect("parse");
    assert_eq!(cats.len(), 1);
    assert_eq!(cats[0].name, "Entertainment");
}

#[sqlx::test]
async fn categories_create_with_budget_fields(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    let payload = serde_json::json!({
        "name": "Groceries",
        "budget_mode": "monthly",
        "budget_amount": "500.00"
    });

    let (status, body) = send(app.clone(), post_json("/api/categories", &payload)).await;
    assert_eq!(status, StatusCode::CREATED);

    let created: Category = serde_json::from_slice(&body).expect("parse");
    assert_eq!(created.name, "Groceries");
    assert_eq!(
        created.budget.mode(),
        Some(budget_core::models::BudgetMode::Monthly)
    );
    assert_eq!(
        created.budget.amount(),
        Some(rust_decimal::Decimal::new(50000, 2))
    );
}

#[sqlx::test]
async fn categories_create_project_with_dates(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    let payload = serde_json::json!({
        "name": "Kitchen Renovation",
        "budget_mode": "project",
        "budget_amount": "10000.00",
        "project_start_date": "2025-03-01",
        "project_end_date": "2025-06-30"
    });

    let (status, body) = send(app, post_json("/api/categories", &payload)).await;
    assert_eq!(status, StatusCode::CREATED);

    let created: Category = serde_json::from_slice(&body).expect("parse");
    assert_eq!(
        created.budget.mode(),
        Some(budget_core::models::BudgetMode::Project)
    );
    if let budget_core::models::BudgetConfig::Project {
        start_date,
        end_date,
        ..
    } = &created.budget
    {
        assert_eq!(
            *start_date,
            chrono::NaiveDate::from_ymd_opt(2025, 3, 1).expect("date")
        );
        assert_eq!(
            *end_date,
            Some(chrono::NaiveDate::from_ymd_opt(2025, 6, 30).expect("date"))
        );
    } else {
        panic!("expected Project budget config");
    }
}

#[sqlx::test]
async fn categories_update_budget_fields(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    // Create with no budget
    let payload = serde_json::json!({ "name": "Transport" });
    let (status, body) = send(app.clone(), post_json("/api/categories", &payload)).await;
    assert_eq!(status, StatusCode::CREATED);
    let created: Category = serde_json::from_slice(&body).expect("parse");
    assert!(created.budget.mode().is_none());

    // Update to monthly budget
    let update = serde_json::json!({
        "name": "Transport",
        "budget_mode": "monthly",
        "budget_amount": "300.00"
    });
    let (status, body) = send(
        app,
        put_json(&format!("/api/categories/{}", created.id), &update),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let updated: Category = serde_json::from_slice(&body).expect("parse");
    assert_eq!(
        updated.budget.mode(),
        Some(budget_core::models::BudgetMode::Monthly)
    );
    assert_eq!(
        updated.budget.amount(),
        Some(rust_decimal::Decimal::new(30000, 2))
    );
}

#[sqlx::test]
async fn categories_delete_returns_204(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    let payload = serde_json::json!({ "name": "ToDelete" });
    let (status, body) = send(app.clone(), post_json("/api/categories", &payload)).await;
    assert_eq!(status, StatusCode::CREATED);

    let created: Category = serde_json::from_slice(&body).expect("parse");

    let (status, _body) = send(
        app.clone(),
        delete(&format!("/api/categories/{}", created.id)),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify it's gone
    let (status, body) = send(app, get("/api/categories")).await;
    assert_eq!(status, StatusCode::OK);

    let cats: Vec<Category> = serde_json::from_slice(&body).expect("parse");
    assert!(cats.is_empty());
}

#[sqlx::test]
async fn categories_create_with_parent(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    // Create parent
    let parent_payload = serde_json::json!({ "name": "Food" });
    let (status, body) = send(app.clone(), post_json("/api/categories", &parent_payload)).await;
    assert_eq!(status, StatusCode::CREATED);
    let parent: Category = serde_json::from_slice(&body).expect("parse parent");

    // Create child with parent_id
    let child_payload = serde_json::json!({
        "name": "Fast Food",
        "parent_id": parent.id.to_string()
    });
    let (status, body) = send(app.clone(), post_json("/api/categories", &child_payload)).await;
    assert_eq!(status, StatusCode::CREATED);

    let child: Category = serde_json::from_slice(&body).expect("parse child");
    assert_eq!(child.name, "Fast Food");
    assert_eq!(child.parent_id, Some(parent.id));

    // List should contain both
    let (status, body) = send(app, get("/api/categories")).await;
    assert_eq!(status, StatusCode::OK);
    let cats: Vec<Category> = serde_json::from_slice(&body).expect("parse list");
    assert_eq!(cats.len(), 2);
}

#[sqlx::test]
async fn categories_create_invalid_budget_mode_returns_422(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    let payload = serde_json::json!({
        "name": "Bad",
        "budget_mode": "weekly"
    });

    let (status, _body) = send(app, post_json("/api/categories", &payload)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

// ===========================================================================
// Rules
// ===========================================================================

#[sqlx::test]
async fn rules_create_and_list(pool: PgPool) {
    let (app, db) = setup(pool).await;

    let category = make_category("Groceries");
    db.insert_category(&category).await.expect("category");

    let payload = serde_json::json!({
        "rule_type": "categorization",
        "conditions": [{"field": "merchant", "pattern": "GROCERY.*"}],
        "target_category_id": category.id.to_string(),
        "priority": 10
    });

    let (status, body) = send(app.clone(), post_json("/api/rules", &payload)).await;
    assert_eq!(status, StatusCode::CREATED);

    let created: Rule = serde_json::from_slice(&body).expect("parse");
    assert_eq!(created.conditions.len(), 1);
    assert_eq!(created.conditions[0].pattern, "GROCERY.*");
    assert_eq!(created.priority, Priority::new(10).unwrap());

    // List should contain the created rule
    let (status, body) = send(app, get("/api/rules")).await;
    assert_eq!(status, StatusCode::OK);

    let rules: Vec<Rule> = serde_json::from_slice(&body).expect("parse");
    assert_eq!(rules.len(), 1);
}

#[sqlx::test]
async fn rules_update(pool: PgPool) {
    let (app, db) = setup(pool).await;

    let category = make_category("Transport");
    db.insert_category(&category).await.expect("category");

    let payload = serde_json::json!({
        "rule_type": "categorization",
        "conditions": [{"field": "merchant", "pattern": "UBER.*"}],
        "target_category_id": category.id.to_string(),
        "priority": 5
    });

    let (_, body) = send(app.clone(), post_json("/api/rules", &payload)).await;
    let created: Rule = serde_json::from_slice(&body).expect("parse");

    let update_payload = serde_json::json!({
        "rule_type": "categorization",
        "conditions": [{"field": "merchant", "pattern": "UBER.*|LYFT.*"}],
        "target_category_id": category.id.to_string(),
        "priority": 20
    });

    let (status, body) = send(
        app,
        put_json(&format!("/api/rules/{}", created.id), &update_payload),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let updated: Rule = serde_json::from_slice(&body).expect("parse");
    assert_eq!(updated.priority, Priority::new(20).unwrap());
    assert_eq!(updated.conditions[0].pattern, "UBER.*|LYFT.*");
    assert_eq!(updated.id, created.id);
}

#[sqlx::test]
async fn rules_delete(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    let payload = serde_json::json!({
        "rule_type": "correlation",
        "conditions": [{"field": "description", "pattern": "TRANSFER.*"}],
        "target_correlation_type": "transfer",
        "priority": 1
    });

    let (status, body) = send(app.clone(), post_json("/api/rules", &payload)).await;
    assert_eq!(status, StatusCode::CREATED);
    let created: Rule = serde_json::from_slice(&body).expect("parse");

    let (status, _body) = send(app.clone(), delete(&format!("/api/rules/{}", created.id))).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify it's gone
    let (status, body) = send(app, get("/api/rules")).await;
    assert_eq!(status, StatusCode::OK);
    let rules: Vec<Rule> = serde_json::from_slice(&body).expect("parse");
    assert!(rules.is_empty());
}

#[sqlx::test]
async fn rules_create_invalid_rule_type_returns_422(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    let payload = serde_json::json!({
        "rule_type": "invalid_type",
        "conditions": [{"field": "merchant", "pattern": "TEST"}],
        "priority": 1
    });

    let (status, _body) = send(app, post_json("/api/rules", &payload)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

// ===========================================================================
// Transactions: Generate Rule
// ===========================================================================

#[sqlx::test]
async fn generate_rule_for_categorized_transaction(pool: PgPool) {
    let (app, db) = setup(pool).await;

    let account = Account {
        id: AccountId::new(),
        provider_account_id: "prov-gen-rule".to_owned(),
        name: "Gen Rule Test".to_owned(),
        nickname: None,
        institution: "Bank".to_owned(),
        account_type: AccountType::Checking,
        currency: CurrencyCode::new("USD").unwrap(),
        origin: AccountOrigin::Manual,
    };
    db.upsert_account(&account).await.expect("account");

    let category = make_category("Groceries");
    db.insert_category(&category).await.expect("category");

    // Manually categorized transaction
    let mut txn = make_txn(account.id, "WHOLE FOODS MARKET", 15);
    txn.categorization = Categorization::Manual(category.id);
    txn.amount = rust_decimal::Decimal::new(7234, 2);
    txn.remittance_information = vec!["Weekly groceries".to_owned()];
    db.upsert_transaction(&txn, None).await.expect("insert txn");
    db.update_transaction_category(
        txn.id,
        category.id,
        budget_core::models::CategoryMethod::Manual,
        None,
    )
    .await
    .expect("categorize");

    let (status, body) = send(
        app,
        post_empty(&format!("/api/transactions/{}/generate-rule", txn.id)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let resp: serde_json::Value = serde_json::from_slice(&body).expect("parse");
    assert_eq!(resp["category_name"], "Groceries");
    assert_eq!(resp["target_category_id"], category.id.to_string());

    let proposals = resp["proposals"].as_array().expect("array");
    // MockLlmProvider returns 3 proposals; some may be filtered if regex is invalid,
    // but at least 1 should survive
    assert!(
        !proposals.is_empty(),
        "expected at least one valid proposal"
    );
    assert!(proposals[0]["match_pattern"].is_string());
    assert!(proposals[0]["explanation"].is_string());
}

#[sqlx::test]
async fn generate_rule_rejects_rule_categorized_transaction(pool: PgPool) {
    let (app, db) = setup(pool).await;

    let account = Account {
        id: AccountId::new(),
        provider_account_id: "prov-gen-rule-rej".to_owned(),
        name: "Rule Rej Test".to_owned(),
        nickname: None,
        institution: "Bank".to_owned(),
        account_type: AccountType::Checking,
        currency: CurrencyCode::new("USD").unwrap(),
        origin: AccountOrigin::Manual,
    };
    db.upsert_account(&account).await.expect("account");

    let category = make_category("Coffee");
    db.insert_category(&category).await.expect("category");

    let mut txn = make_txn(account.id, "STARBUCKS", 10);
    txn.categorization = Categorization::Manual(category.id);
    db.upsert_transaction(&txn, None).await.expect("insert txn");
    db.update_transaction_category(
        txn.id,
        category.id,
        budget_core::models::CategoryMethod::Rule,
        None,
    )
    .await
    .expect("categorize");

    let (status, _body) = send(
        app,
        post_empty(&format!("/api/transactions/{}/generate-rule", txn.id)),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[sqlx::test]
async fn generate_rule_rejects_uncategorized_transaction(pool: PgPool) {
    let (app, db) = setup(pool).await;

    let account = Account {
        id: AccountId::new(),
        provider_account_id: "prov-gen-rule-uncat".to_owned(),
        name: "Uncat Test".to_owned(),
        nickname: None,
        institution: "Bank".to_owned(),
        account_type: AccountType::Checking,
        currency: CurrencyCode::new("USD").unwrap(),
        origin: AccountOrigin::Manual,
    };
    db.upsert_account(&account).await.expect("account");

    let txn = make_txn(account.id, "UNKNOWN SHOP", 10);
    db.upsert_transaction(&txn, None).await.expect("insert txn");

    let (status, _body) = send(
        app,
        post_empty(&format!("/api/transactions/{}/generate-rule", txn.id)),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ===========================================================================
// Rules: Apply
// ===========================================================================

#[sqlx::test]
async fn rules_apply_with_no_rules_categorizes_nothing(pool: PgPool) {
    let (app, db) = setup(pool).await;

    let account = Account {
        id: AccountId::new(),
        provider_account_id: "prov-apply-1".to_owned(),
        name: "Apply Test".to_owned(),
        nickname: None,
        institution: "Bank".to_owned(),
        account_type: AccountType::Checking,
        currency: CurrencyCode::new("USD").unwrap(),
        origin: AccountOrigin::Manual,
    };
    db.upsert_account(&account).await.expect("account");

    let txn = make_txn(account.id, "RANDOM SHOP", 15);
    db.upsert_transaction(&txn, None).await.expect("insert");

    let (status, body) = send(app, post_empty("/api/rules/apply")).await;
    assert_eq!(status, StatusCode::OK);

    let resp: serde_json::Value = serde_json::from_slice(&body).expect("parse");
    assert_eq!(resp["categorized_count"], 0);
}

#[sqlx::test]
async fn rules_apply_categorizes_matching_transactions(pool: PgPool) {
    let (app, db) = setup(pool).await;

    let account = Account {
        id: AccountId::new(),
        provider_account_id: "prov-apply-2".to_owned(),
        name: "Apply Match Test".to_owned(),
        nickname: None,
        institution: "Bank".to_owned(),
        account_type: AccountType::Checking,
        currency: CurrencyCode::new("USD").unwrap(),
        origin: AccountOrigin::Manual,
    };
    db.upsert_account(&account).await.expect("account");

    let category = make_category("Groceries");
    db.insert_category(&category).await.expect("category");

    // Create a rule matching "LIDL"
    let rule = Rule {
        id: budget_core::models::RuleId::new(),
        target: RuleTarget::Categorization(category.id),
        conditions: vec![RuleCondition {
            field: budget_core::models::MatchField::Merchant,
            pattern: "LIDL".to_owned(),
        }],
        priority: Priority::new(10).unwrap(),
    };
    db.insert_rule(&rule).await.expect("rule");

    // Insert two uncategorized matching transactions and one non-matching
    let txn_match_1 = make_uncategorized_txn(account.id, "LIDL GB 1234", 10);
    let txn_match_2 = make_uncategorized_txn(account.id, "LIDL DE 5678", 12);
    let txn_nomatch = make_uncategorized_txn(account.id, "ALDI", 14);

    db.upsert_transaction(&txn_match_1, None)
        .await
        .expect("insert");
    db.upsert_transaction(&txn_match_2, None)
        .await
        .expect("insert");
    db.upsert_transaction(&txn_nomatch, None)
        .await
        .expect("insert");

    let (status, body) = send(app, post_empty("/api/rules/apply")).await;
    assert_eq!(status, StatusCode::OK);

    let resp: serde_json::Value = serde_json::from_slice(&body).expect("parse");
    assert_eq!(resp["categorized_count"], 2);

    // Verify the matching transactions are categorized and the non-matching one is not
    let updated_1 = db
        .get_transaction_by_id(txn_match_1.id)
        .await
        .expect("query")
        .expect("found");
    assert_eq!(updated_1.categorization.category_id(), Some(category.id));

    let updated_2 = db
        .get_transaction_by_id(txn_match_2.id)
        .await
        .expect("query")
        .expect("found");
    assert_eq!(updated_2.categorization.category_id(), Some(category.id));

    let unchanged = db
        .get_transaction_by_id(txn_nomatch.id)
        .await
        .expect("query")
        .expect("found");
    assert_eq!(unchanged.categorization, Categorization::Uncategorized);
}

#[sqlx::test]
async fn rules_apply_skips_already_categorized_transactions(pool: PgPool) {
    let (app, db) = setup(pool).await;

    let account = Account {
        id: AccountId::new(),
        provider_account_id: "prov-apply-3".to_owned(),
        name: "Apply Skip Test".to_owned(),
        nickname: None,
        institution: "Bank".to_owned(),
        account_type: AccountType::Checking,
        currency: CurrencyCode::new("USD").unwrap(),
        origin: AccountOrigin::Manual,
    };
    db.upsert_account(&account).await.expect("account");

    let category_a = make_category("Coffee");
    let category_b = make_category("Tea");
    db.insert_category(&category_a).await.expect("category");
    db.insert_category(&category_b).await.expect("category");

    // Rule matches "STARBUCKS"
    let rule = Rule {
        id: budget_core::models::RuleId::new(),
        target: RuleTarget::Categorization(category_a.id),
        conditions: vec![RuleCondition {
            field: budget_core::models::MatchField::Merchant,
            pattern: "STARBUCKS".to_owned(),
        }],
        priority: Priority::new(10).unwrap(),
    };
    db.insert_rule(&rule).await.expect("rule");

    // Already-categorized transaction — should not be re-categorized
    let mut txn = make_txn(account.id, "STARBUCKS RESERVE", 1);
    txn.categorization = Categorization::Manual(category_b.id);
    txn.amount = rust_decimal::Decimal::new(550, 2);
    txn.remittance_information = vec!["Coffee".to_owned()];
    txn.posted_date = chrono::NaiveDate::from_ymd_opt(2025, 2, 1).expect("date");
    db.upsert_transaction(&txn, None).await.expect("insert");

    let (status, body) = send(app, post_empty("/api/rules/apply")).await;
    assert_eq!(status, StatusCode::OK);

    let resp: serde_json::Value = serde_json::from_slice(&body).expect("parse");
    assert_eq!(resp["categorized_count"], 0);

    // Verify the existing category was preserved
    let fetched = db
        .get_transaction_by_id(txn.id)
        .await
        .expect("query")
        .expect("found");
    assert_eq!(fetched.categorization.category_id(), Some(category_b.id));
}

// ===========================================================================
// Budgets
// ===========================================================================

#[sqlx::test]
async fn budgets_status_returns_404_when_no_months(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    let (status, _body) = send(app, get("/api/budgets/status")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn budgets_months_empty(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    let (status, body) = send(app, get("/api/budgets/months")).await;
    assert_eq!(status, StatusCode::OK);

    let months: Vec<serde_json::Value> = serde_json::from_slice(&body).expect("parse");
    assert!(months.is_empty());
}

#[sqlx::test]
async fn budgets_status_with_current_month(pool: PgPool) {
    let (app, db) = setup(pool).await;

    // Create an account
    let account = Account {
        id: AccountId::new(),
        provider_account_id: "prov-budget-test".to_owned(),
        name: "Budget Test".to_owned(),
        nickname: None,
        institution: "Bank".to_owned(),
        account_type: AccountType::Checking,
        currency: CurrencyCode::new("USD").unwrap(),
        origin: AccountOrigin::Manual,
    };
    db.upsert_account(&account).await.expect("account");

    // Create a Salary category so months can be derived
    let salary_cat = Category {
        budget: budget_core::models::BudgetConfig::Salary,
        ..make_category("Salary")
    };
    db.insert_category(&salary_cat)
        .await
        .expect("salary category");

    // Create a category with a monthly budget
    let payload = serde_json::json!({
        "name": "Food",
        "budget_mode": "monthly",
        "budget_amount": "500.00"
    });
    let (status, body) = send(app.clone(), post_json("/api/categories", &payload)).await;
    assert_eq!(status, StatusCode::CREATED);
    let category: Category = serde_json::from_slice(&body).expect("parse");

    // Insert a salary transaction so budget months are derived
    let today = chrono::Utc::now().date_naive();
    let mut salary_txn = make_txn(account.id, "EMPLOYER INC", 1);
    salary_txn.categorization = Categorization::Manual(salary_cat.id);
    salary_txn.amount = rust_decimal::Decimal::new(500_000, 2);
    salary_txn.posted_date = today - chrono::Duration::days(5);
    db.upsert_transaction(&salary_txn, Some("salary-1"))
        .await
        .expect("insert salary");

    let (status, body) = send(app, get("/api/budgets/status")).await;
    assert_eq!(status, StatusCode::OK);

    let resp: serde_json::Value = serde_json::from_slice(&body).expect("parse");
    let statuses = resp["statuses"].as_array().expect("statuses array");
    assert_eq!(statuses.len(), 1);
    assert_eq!(
        statuses[0]["category_id"].as_str().expect("category_id"),
        category.id.as_uuid().to_string()
    );
}

// ===========================================================================
// Jobs
// ===========================================================================

#[sqlx::test]
async fn jobs_sync_returns_202(pool: PgPool) {
    let (app, _db) = setup(pool).await;
    let account_id = uuid::Uuid::new_v4();

    let (status, _body) = send(app, post_empty(&format!("/api/jobs/sync/{account_id}"))).await;
    assert_eq!(status, StatusCode::ACCEPTED);
}

#[sqlx::test]
async fn jobs_categorize_returns_202(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    let (status, _body) = send(app, post_empty("/api/jobs/categorize")).await;
    assert_eq!(status, StatusCode::ACCEPTED);
}

#[sqlx::test]
async fn jobs_correlate_returns_202(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    let (status, _body) = send(app, post_empty("/api/jobs/correlate")).await;
    assert_eq!(status, StatusCode::ACCEPTED);
}

#[sqlx::test]
async fn jobs_pipeline_returns_202(pool: PgPool) {
    let (app, _db) = setup(pool).await;
    let account_id = uuid::Uuid::new_v4();

    let (status, _body) = send(app, post_empty(&format!("/api/jobs/pipeline/{account_id}"))).await;
    assert_eq!(status, StatusCode::ACCEPTED);
}

#[sqlx::test]
async fn jobs_list_returns_empty_array(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    let (status, body) = send(app, get("/api/jobs")).await;
    assert_eq!(status, StatusCode::OK);

    let jobs: Vec<serde_json::Value> = serde_json::from_slice(&body).expect("parse");
    assert!(jobs.is_empty());
}

#[sqlx::test]
async fn jobs_list_returns_enqueued_job(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    // Enqueue a categorize job
    let (status, _body) = send(app.clone(), post_empty("/api/jobs/categorize")).await;
    assert_eq!(status, StatusCode::ACCEPTED);

    // List jobs and verify it appears
    let (status, body) = send(app, get("/api/jobs")).await;
    assert_eq!(status, StatusCode::OK);

    let jobs: Vec<serde_json::Value> = serde_json::from_slice(&body).expect("parse");
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0]["job_type"], "budget_jobs::CategorizeJob");
    assert_eq!(jobs[0]["status"], "Pending");
    assert!(jobs[0]["run_at"].is_string());
}

// ===========================================================================
// Additional edge case tests
// ===========================================================================

#[sqlx::test]
async fn accounts_get_invalid_uuid_returns_400(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    let (status, _body) = send(app, get("/api/accounts/not-a-uuid")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[sqlx::test]
async fn categories_delete_nonexistent_returns_204(pool: PgPool) {
    // DELETE on a nonexistent row is not an error -- the SQL succeeds with 0 rows affected
    let (app, _db) = setup(pool).await;
    let fake_id = uuid::Uuid::new_v4();

    let (status, _body) = send(app, delete(&format!("/api/categories/{fake_id}"))).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[sqlx::test]
async fn rules_create_correlation_rule(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    let payload = serde_json::json!({
        "rule_type": "correlation",
        "conditions": [{"field": "amount_range", "pattern": "100-500"}],
        "target_correlation_type": "reimbursement",
        "priority": 3
    });

    let (status, body) = send(app, post_json("/api/rules", &payload)).await;
    assert_eq!(status, StatusCode::CREATED);

    let created: Rule = serde_json::from_slice(&body).expect("parse");
    assert_eq!(
        created.target.rule_type(),
        budget_core::models::RuleType::Correlation
    );
    assert_eq!(created.conditions.len(), 1);
    assert_eq!(
        created.conditions[0].field,
        budget_core::models::MatchField::AmountRange
    );
    assert_eq!(
        created.target.correlation_type(),
        Some(budget_core::models::CorrelationType::Reimbursement)
    );
}

#[sqlx::test]
async fn transactions_categorize_with_invalid_txn_id_returns_400(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    let payload = serde_json::json!({
        "category_id": uuid::Uuid::new_v4().to_string()
    });

    let (status, _body) = send(
        app,
        post_json("/api/transactions/not-a-uuid/categorize", &payload),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[sqlx::test]
async fn rules_create_invalid_match_field_returns_422(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    let payload = serde_json::json!({
        "rule_type": "categorization",
        "conditions": [{"field": "bogus_field", "pattern": "TEST"}],
        "priority": 1
    });

    let (status, _body) = send(app, post_json("/api/rules", &payload)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[sqlx::test]
async fn accounts_create_all_types(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    for account_type in &[
        "checking",
        "savings",
        "credit_card",
        "investment",
        "loan",
        "other",
    ] {
        let payload = serde_json::json!({
            "provider_account_id": format!("prov-{account_type}"),
            "name": format!("{account_type} account"),
            "institution": "Test Bank",
            "account_type": account_type,
            "currency": "USD"
        });

        let (status, _body) = send(app.clone(), post_json("/api/accounts", &payload)).await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "failed to create {account_type}"
        );
    }

    // Verify all 6 accounts were created
    let (status, body) = send(app, get("/api/accounts")).await;
    assert_eq!(status, StatusCode::OK);
    let accounts: Vec<Account> = serde_json::from_slice(&body).expect("parse");
    assert_eq!(accounts.len(), 6);
}

// ===========================================================================
// Authentication
// ===========================================================================

#[sqlx::test]
async fn auth_health_is_unauthenticated(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    let (status, body) = send(app, get_unauthenticated("/health")).await;
    assert_eq!(status, StatusCode::OK);

    let parsed: serde_json::Value = serde_json::from_slice(&body).expect("parse");
    assert_eq!(parsed["status"], "ok");
}

#[sqlx::test]
async fn auth_api_rejects_missing_token(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    let (status, _body) = send(app, get_unauthenticated("/api/accounts")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn auth_api_rejects_wrong_token(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    let request = Request::builder()
        .uri("/api/accounts")
        .method("GET")
        .header("authorization", "Bearer wrong-key")
        .body(Body::empty())
        .expect("build request");

    let (status, _body) = send(app, request).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn auth_api_rejects_malformed_header(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    let request = Request::builder()
        .uri("/api/accounts")
        .method("GET")
        .header("authorization", "Basic dXNlcjpwYXNz")
        .body(Body::empty())
        .expect("build request");

    let (status, _body) = send(app, request).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn auth_api_accepts_valid_token(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    let (status, _body) = send(app, get("/api/accounts")).await;
    assert_eq!(status, StatusCode::OK);
}

// ===========================================================================
// Connections
// ===========================================================================

#[sqlx::test]
async fn connections_list_empty(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    let (status, body) = send(app, get("/api/connections")).await;
    assert_eq!(status, StatusCode::OK);

    let connections: Vec<budget_core::models::Connection> =
        serde_json::from_slice(&body).expect("parse");
    assert!(connections.is_empty());
}

#[sqlx::test]
async fn connections_aspsps_returns_501_when_not_configured(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    let (status, _body) = send(app, get("/api/connections/aspsps?country=FI")).await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
}

#[sqlx::test]
async fn connections_callback_rejects_invalid_state(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    // The callback endpoint should return 501 because enable_banking_auth is None.
    // But if it were configured, an invalid state token would return 400.
    let (status, _body) = send(
        app,
        get_unauthenticated("/api/connections/callback?code=test&state=invalid"),
    )
    .await;
    // Without Enable Banking configured, callback returns 501
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
}

#[sqlx::test]
async fn connections_callback_is_unauthenticated(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    // The callback endpoint should NOT return 401, proving it's unauthenticated.
    // It should return 501 (not configured) rather than 401 (unauthorized).
    let (status, _body) = send(
        app,
        get_unauthenticated("/api/connections/callback?code=x&state=y"),
    )
    .await;
    assert_ne!(
        status,
        StatusCode::UNAUTHORIZED,
        "callback must be unauthenticated"
    );
}

// ===========================================================================
// Balance Snapshots
// ===========================================================================

#[sqlx::test]
async fn balance_snapshot_create_and_list(pool: PgPool) {
    let (app, db) = setup(pool).await;

    // Seed an account
    let account = Account {
        id: AccountId::new(),
        provider_account_id: "bal-test-001".to_owned(),
        name: "Balance Test".to_owned(),
        nickname: None,
        institution: "Test Bank".to_owned(),
        account_type: AccountType::Checking,
        currency: CurrencyCode::new("EUR").unwrap(),
        origin: AccountOrigin::Manual,
    };
    db.upsert_account(&account).await.expect("seed account");

    let uri = format!("/api/accounts/{}/balances", account.id);

    // POST a manual balance snapshot (Decimal uses serde-str)
    let payload = serde_json::json!({
        "current": "1500.50",
        "available": "1400.00",
        "currency": "EUR"
    });
    let (status, body) = send(app.clone(), post_json(&uri, &payload)).await;
    assert_eq!(status, StatusCode::CREATED);
    let snapshot: budget_core::models::BalanceSnapshot =
        serde_json::from_slice(&body).expect("parse snapshot");
    assert_eq!(snapshot.account_id, account.id);

    // POST a second snapshot without optional fields
    let payload2 = serde_json::json!({ "current": "2000" });
    let (status2, _) = send(app.clone(), post_json(&uri, &payload2)).await;
    assert_eq!(status2, StatusCode::CREATED);

    // GET the list
    let (status3, body3) = send(app.clone(), get(&uri)).await;
    assert_eq!(status3, StatusCode::OK);
    let snapshots: Vec<budget_core::models::BalanceSnapshot> =
        serde_json::from_slice(&body3).expect("parse list");
    assert_eq!(snapshots.len(), 2);
    // Newest first
    assert_eq!(snapshots[0].current, rust_decimal::Decimal::new(2000, 0));
}

#[sqlx::test]
async fn balance_snapshot_404_for_missing_account(pool: PgPool) {
    let (app, _db) = setup(pool).await;
    let fake_id = AccountId::new();
    let uri = format!("/api/accounts/{fake_id}/balances");
    let payload = serde_json::json!({ "current": "100" });
    let (status, _) = send(app, post_json(&uri, &payload)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn balance_snapshot_list_with_limit(pool: PgPool) {
    let (app, db) = setup(pool).await;

    let account = Account {
        id: AccountId::new(),
        provider_account_id: "bal-limit-001".to_owned(),
        name: "Limit Test".to_owned(),
        nickname: None,
        institution: "Test Bank".to_owned(),
        account_type: AccountType::Savings,
        currency: CurrencyCode::new("USD").unwrap(),
        origin: AccountOrigin::Manual,
    };
    db.upsert_account(&account).await.expect("seed account");

    let uri = format!("/api/accounts/{}/balances", account.id);

    // Insert 3 snapshots
    for amount in ["100", "200", "300"] {
        let payload = serde_json::json!({ "current": amount });
        let (status, _) = send(app.clone(), post_json(&uri, &payload)).await;
        assert_eq!(status, StatusCode::CREATED);
    }

    // GET with limit=2
    let (status, body) = send(app.clone(), get(&format!("{uri}?limit=2"))).await;
    assert_eq!(status, StatusCode::OK);
    let snapshots: Vec<budget_core::models::BalanceSnapshot> =
        serde_json::from_slice(&body).expect("parse list");
    assert_eq!(snapshots.len(), 2);
}

// ===========================================================================
// OpenAPI
// ===========================================================================

#[sqlx::test]
async fn openapi_spec_is_valid_and_complete(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    let (status, body) = send(app, get_unauthenticated("/api/docs")).await;
    assert_eq!(status, StatusCode::OK);

    let html = String::from_utf8(body).expect("valid utf-8");
    let json_start = html
        .find(r#"type="application/json">"#)
        .expect("spec in page")
        + r#"type="application/json">"#.len();
    let json_end = html[json_start..]
        .find("</script>")
        .expect("closing script")
        + json_start;
    let spec: serde_json::Value =
        serde_json::from_str(html[json_start..json_end].trim()).expect("valid JSON");

    assert_eq!(spec["openapi"], "3.1.0");
    assert_eq!(spec["info"]["title"], "Budget API");

    let paths = spec["paths"].as_object().expect("paths object");
    assert!(
        paths.len() >= 35,
        "expected at least 35 paths, got {}",
        paths.len()
    );

    // Spot-check a few key endpoints exist
    assert!(paths.contains_key("/accounts"), "missing /accounts");
    assert!(paths.contains_key("/transactions"), "missing /transactions");
    assert!(paths.contains_key("/categories"), "missing /categories");
    assert!(paths.contains_key("/rules"), "missing /rules");
    assert!(
        paths.contains_key("/budgets/status"),
        "missing /budgets/status"
    );
    assert!(
        paths.contains_key("/amazon/accounts"),
        "missing /amazon/accounts"
    );

    let schemas = spec["components"]["schemas"]
        .as_object()
        .expect("schemas object");
    assert!(
        schemas.len() >= 50,
        "expected at least 50 schemas, got {}",
        schemas.len()
    );

    // Spot-check key schemas
    assert!(schemas.contains_key("Account"), "missing Account schema");
    assert!(
        schemas.contains_key("Transaction"),
        "missing Transaction schema"
    );
    assert!(schemas.contains_key("Category"), "missing Category schema");
    assert!(schemas.contains_key("Rule"), "missing Rule schema");

    // Security scheme is defined
    let security_schemes = &spec["components"]["securitySchemes"];
    assert!(
        security_schemes["bearer_token"].is_object(),
        "missing bearer_token security scheme"
    );
}

// ===========================================================================
// Accounts: net-worth
// ===========================================================================

#[sqlx::test]
async fn accounts_net_worth_empty(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    let (status, body) = send(app, get("/api/accounts/net-worth")).await;
    assert_eq!(status, StatusCode::OK);

    let resp: serde_json::Value = serde_json::from_slice(&body).expect("parse");
    assert_eq!(resp["total"], "0");
    assert!(resp["accounts"].as_array().expect("array").is_empty());
}

#[sqlx::test]
async fn accounts_net_worth_with_balances(pool: PgPool) {
    let (app, db) = setup(pool).await;

    let acct1 = make_account("Checking");
    let acct2 = make_account("Savings");
    db.upsert_account(&acct1).await.expect("acct1");
    db.upsert_account(&acct2).await.expect("acct2");

    let snap1 = BalanceSnapshot {
        id: BalanceSnapshotId::new(),
        account_id: acct1.id,
        current: rust_decimal::Decimal::new(150_050, 2),
        available: None,
        currency: CurrencyCode::new("EUR").unwrap(),
        snapshot_at: chrono::Utc::now(),
    };
    let snap2 = BalanceSnapshot {
        id: BalanceSnapshotId::new(),
        account_id: acct2.id,
        current: rust_decimal::Decimal::new(300_000, 2),
        available: None,
        currency: CurrencyCode::new("EUR").unwrap(),
        snapshot_at: chrono::Utc::now(),
    };
    db.insert_balance_snapshot(&snap1).await.expect("snap1");
    db.insert_balance_snapshot(&snap2).await.expect("snap2");

    let (status, body) = send(app, get("/api/accounts/net-worth")).await;
    assert_eq!(status, StatusCode::OK);

    let resp: serde_json::Value = serde_json::from_slice(&body).expect("parse");
    assert_eq!(resp["total"], "4500.50");
    let accounts = resp["accounts"].as_array().expect("array");
    assert_eq!(accounts.len(), 2);
    // Sorted by balance descending
    assert_eq!(accounts[0]["current"], "3000.00");
}

// ===========================================================================
// Accounts: net-worth/projection
// ===========================================================================

#[sqlx::test]
async fn accounts_net_worth_projection_empty(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    let (status, body) = send(app, get("/api/accounts/net-worth/projection")).await;
    assert_eq!(status, StatusCode::OK);

    let resp: serde_json::Value = serde_json::from_slice(&body).expect("parse");
    // Insufficient data returns empty arrays with a message
    assert!(resp["history"].as_array().expect("array").is_empty());
    assert!(resp["forecast"].as_array().expect("array").is_empty());
    assert!(resp["message"].is_string());
}

#[sqlx::test]
async fn accounts_net_worth_projection_with_params(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    let (status, body) = send(
        app,
        get("/api/accounts/net-worth/projection?months=6&interval_width=0.9"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let resp: serde_json::Value = serde_json::from_slice(&body).expect("parse");
    // With no data, should still return 200 with empty/message
    assert!(resp["history"].is_array());
    assert!(resp["forecast"].is_array());
}

// ===========================================================================
// Accounts: PATCH /{id} (update_nickname)
// ===========================================================================

#[sqlx::test]
async fn accounts_update_nickname(pool: PgPool) {
    let (app, db) = setup(pool).await;

    let account = make_account("Original Name");
    db.upsert_account(&account).await.expect("account");

    // Set a nickname
    let payload = serde_json::json!({ "nickname": "My Primary" });
    let (status, body) = send(
        app.clone(),
        patch_json(&format!("/api/accounts/{}", account.id), &payload),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let updated: Account = serde_json::from_slice(&body).expect("parse");
    assert_eq!(updated.nickname.as_deref(), Some("My Primary"));
    assert_eq!(updated.name, "Original Name");

    // Clear the nickname
    let clear = serde_json::json!({ "nickname": null });
    let (status, body) = send(
        app,
        patch_json(&format!("/api/accounts/{}", account.id), &clear),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let cleared: Account = serde_json::from_slice(&body).expect("parse");
    assert!(cleared.nickname.is_none());
}

#[sqlx::test]
async fn accounts_update_nickname_nonexistent_returns_404(pool: PgPool) {
    let (app, _db) = setup(pool).await;
    let fake_id = uuid::Uuid::new_v4();

    let payload = serde_json::json!({ "nickname": "test" });
    let (status, _body) = send(
        app,
        patch_json(&format!("/api/accounts/{fake_id}"), &payload),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ===========================================================================
// Amazon: accounts CRUD
// ===========================================================================

#[sqlx::test]
async fn amazon_accounts_list_empty(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    let (status, body) = send(app, get("/api/amazon/accounts")).await;
    assert_eq!(status, StatusCode::OK);

    let accounts: Vec<AmazonAccount> = serde_json::from_slice(&body).expect("parse");
    assert!(accounts.is_empty());
}

#[sqlx::test]
async fn amazon_accounts_create_and_list(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    let payload = serde_json::json!({ "label": "My Amazon DE" });
    let (status, body) = send(app.clone(), post_json("/api/amazon/accounts", &payload)).await;
    assert_eq!(status, StatusCode::CREATED);

    let created: AmazonAccount = serde_json::from_slice(&body).expect("parse");
    assert_eq!(created.label, "My Amazon DE");

    let (status, body) = send(app, get("/api/amazon/accounts")).await;
    assert_eq!(status, StatusCode::OK);

    let accounts: Vec<AmazonAccount> = serde_json::from_slice(&body).expect("parse");
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].label, "My Amazon DE");
}

#[sqlx::test]
async fn amazon_accounts_delete(pool: PgPool) {
    let (app, db) = setup(pool).await;

    let account = AmazonAccount {
        id: AmazonAccountId::new(),
        label: "To Delete".to_owned(),
    };
    db.insert_amazon_account(&account).await.expect("insert");

    let (status, _body) = send(
        app.clone(),
        delete(&format!("/api/amazon/accounts/{}", account.id)),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify it's gone
    let (status, body) = send(app, get("/api/amazon/accounts")).await;
    assert_eq!(status, StatusCode::OK);
    let accounts: Vec<AmazonAccount> = serde_json::from_slice(&body).expect("parse");
    assert!(accounts.is_empty());
}

#[sqlx::test]
async fn amazon_accounts_delete_nonexistent_returns_404(pool: PgPool) {
    let (app, _db) = setup(pool).await;
    let fake_id = uuid::Uuid::new_v4();

    let (status, _body) = send(app, delete(&format!("/api/amazon/accounts/{fake_id}"))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn amazon_accounts_update_label(pool: PgPool) {
    let (app, db) = setup(pool).await;

    let account = AmazonAccount {
        id: AmazonAccountId::new(),
        label: "Old Label".to_owned(),
    };
    db.insert_amazon_account(&account).await.expect("insert");

    let payload = serde_json::json!({ "label": "New Label" });
    let (status, body) = send(
        app,
        patch_json(&format!("/api/amazon/accounts/{}", account.id), &payload),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let updated: AmazonAccount = serde_json::from_slice(&body).expect("parse");
    assert_eq!(updated.label, "New Label");
    assert_eq!(updated.id, account.id);
}

#[sqlx::test]
async fn amazon_accounts_update_nonexistent_returns_404(pool: PgPool) {
    let (app, _db) = setup(pool).await;
    let fake_id = uuid::Uuid::new_v4();

    let payload = serde_json::json!({ "label": "Nope" });
    let (status, _body) = send(
        app,
        patch_json(&format!("/api/amazon/accounts/{fake_id}"), &payload),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ===========================================================================
// Amazon: cookies
// ===========================================================================

#[sqlx::test]
async fn amazon_cookies_nonexistent_account_returns_404(pool: PgPool) {
    let (app, _db) = setup(pool).await;
    let fake_id = uuid::Uuid::new_v4();

    let payload = serde_json::json!({ "cookies": [] });
    let (status, _body) = send(
        app,
        post_json(&format!("/api/amazon/accounts/{fake_id}/cookies"), &payload),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ===========================================================================
// Amazon: account status
// ===========================================================================

#[sqlx::test]
async fn amazon_account_status(pool: PgPool) {
    let (app, db) = setup(pool).await;

    let account = AmazonAccount {
        id: AmazonAccountId::new(),
        label: "Status Test".to_owned(),
    };
    db.insert_amazon_account(&account).await.expect("insert");

    let (status, body) = send(
        app,
        get(&format!("/api/amazon/accounts/{}/status", account.id)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let resp: serde_json::Value = serde_json::from_slice(&body).expect("parse");
    assert_eq!(resp["account"]["label"], "Status Test");
    // No cookies file exists, so cookies_valid should be null
    assert!(resp["cookies_valid"].is_null());
}

#[sqlx::test]
async fn amazon_account_status_nonexistent_returns_404(pool: PgPool) {
    let (app, _db) = setup(pool).await;
    let fake_id = uuid::Uuid::new_v4();

    let (status, _body) = send(app, get(&format!("/api/amazon/accounts/{fake_id}/status"))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ===========================================================================
// Amazon: sync
// ===========================================================================

#[sqlx::test]
async fn amazon_trigger_sync(pool: PgPool) {
    let (app, db) = setup(pool).await;

    let account = AmazonAccount {
        id: AmazonAccountId::new(),
        label: "Sync Test".to_owned(),
    };
    db.insert_amazon_account(&account).await.expect("insert");

    let (status, _body) = send(
        app,
        post_empty(&format!("/api/amazon/accounts/{}/sync", account.id)),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
}

#[sqlx::test]
async fn amazon_trigger_sync_nonexistent_returns_404(pool: PgPool) {
    let (app, _db) = setup(pool).await;
    let fake_id = uuid::Uuid::new_v4();

    let (status, _body) = send(
        app,
        post_empty(&format!("/api/amazon/accounts/{fake_id}/sync")),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ===========================================================================
// Amazon: enrichment
// ===========================================================================

#[sqlx::test]
async fn amazon_enrichment_nonexistent_returns_404(pool: PgPool) {
    let (app, _db) = setup(pool).await;
    let fake_id = uuid::Uuid::new_v4();

    let (status, _body) = send(app, get(&format!("/api/amazon/enrichment/{fake_id}"))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ===========================================================================
// Amazon: matches
// ===========================================================================

#[sqlx::test]
async fn amazon_matches_empty(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    let (status, body) = send(app, get("/api/amazon/matches")).await;
    assert_eq!(status, StatusCode::OK);

    let matches: Vec<serde_json::Value> = serde_json::from_slice(&body).expect("parse");
    assert!(matches.is_empty());
}

// ===========================================================================
// Budgets: burndown
// ===========================================================================

#[sqlx::test]
async fn budgets_burndown_missing_category_returns_404(pool: PgPool) {
    let (app, _db) = setup(pool).await;
    let fake_id = uuid::Uuid::new_v4();

    let (status, _body) = send(
        app,
        get(&format!("/api/budgets/burndown?category_id={fake_id}")),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn budgets_burndown_non_monthly_returns_400(pool: PgPool) {
    let (app, db) = setup(pool).await;

    // Create a category with annual budget (not monthly)
    let category = Category {
        id: CategoryId::new(),
        name: CategoryName::new("Annual Cat").expect("valid"),
        parent_id: None,
        budget: BudgetConfig::Annual {
            amount: rust_decimal::Decimal::new(120_000, 2),
            budget_type: BudgetType::Variable,
        },
    };
    db.insert_category(&category).await.expect("category");

    let (status, _body) = send(
        app,
        get(&format!(
            "/api/budgets/burndown?category_id={}",
            category.id.as_uuid()
        )),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[sqlx::test]
async fn budgets_burndown_fixed_category_returns_400(pool: PgPool) {
    let (app, db) = setup(pool).await;

    // Monthly fixed category — burndown only supports variable
    let category = Category {
        id: CategoryId::new(),
        name: CategoryName::new("Fixed Cat").expect("valid"),
        parent_id: None,
        budget: BudgetConfig::Monthly {
            amount: rust_decimal::Decimal::new(5000, 2),
            budget_type: BudgetType::Fixed,
        },
    };
    db.insert_category(&category).await.expect("category");

    let (status, _body) = send(
        app,
        get(&format!(
            "/api/budgets/burndown?category_id={}",
            category.id.as_uuid()
        )),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[sqlx::test]
async fn budgets_burndown_with_data(pool: PgPool) {
    let (app, db) = setup(pool).await;

    let account = make_account("Burndown Test");
    db.upsert_account(&account).await.expect("account");

    // Salary category to create budget months
    let salary_cat = Category {
        budget: BudgetConfig::Salary,
        ..make_category("Salary")
    };
    db.insert_category(&salary_cat).await.expect("salary cat");

    // Monthly variable category for burndown
    let category = Category {
        id: CategoryId::new(),
        name: CategoryName::new("Groceries").expect("valid"),
        parent_id: None,
        budget: BudgetConfig::Monthly {
            amount: rust_decimal::Decimal::new(50000, 2),
            budget_type: BudgetType::Variable,
        },
    };
    db.insert_category(&category).await.expect("category");

    // Insert a salary transaction to create budget months
    let today = chrono::Utc::now().date_naive();
    let mut salary_txn = make_txn(account.id, "EMPLOYER", 1);
    salary_txn.categorization = Categorization::Manual(salary_cat.id);
    salary_txn.amount = rust_decimal::Decimal::new(500_000, 2);
    salary_txn.posted_date = today - chrono::Duration::days(5);
    db.upsert_transaction(&salary_txn, Some("salary-bd"))
        .await
        .expect("salary txn");

    // Insert a spending transaction
    let mut spend_txn = make_txn(account.id, "SUPERMARKET", 1);
    spend_txn.categorization = Categorization::Manual(category.id);
    spend_txn.amount = rust_decimal::Decimal::new(-5000, 2);
    spend_txn.posted_date = today - chrono::Duration::days(3);
    db.upsert_transaction(&spend_txn, Some("spend-bd"))
        .await
        .expect("spend txn");
    db.update_transaction_category(
        spend_txn.id,
        category.id,
        budget_core::models::CategoryMethod::Manual,
        None,
    )
    .await
    .expect("categorize");

    let (status, body) = send(
        app,
        get(&format!(
            "/api/budgets/burndown?category_id={}",
            category.id.as_uuid()
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let resp: serde_json::Value = serde_json::from_slice(&body).expect("parse");
    assert_eq!(resp["category_name"], "Groceries");
    assert_eq!(resp["budget_amount"], "500.00");
    assert!(resp["current"]["points"].is_array());
    assert!(resp["current"]["total_days"].as_u64().expect("u64") > 0);
}

// ===========================================================================
// Categories: suggestions
// ===========================================================================

#[sqlx::test]
async fn categories_suggestions_empty(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    let (status, body) = send(app, get("/api/categories/suggestions")).await;
    assert_eq!(status, StatusCode::OK);

    let suggestions: Vec<serde_json::Value> = serde_json::from_slice(&body).expect("parse");
    assert!(suggestions.is_empty());
}

#[sqlx::test]
async fn categories_suggestions_with_data(pool: PgPool) {
    let (app, db) = setup(pool).await;

    let account = make_account("Suggestions Test");
    db.upsert_account(&account).await.expect("account");

    // Insert uncategorized transactions with suggested_category set
    let mut txn1 = make_txn(account.id, "STORE A", 1);
    txn1.suggested_category = Some("Groceries".to_owned());
    db.upsert_transaction(&txn1, Some("sug-1"))
        .await
        .expect("insert");
    db.update_transaction_suggested_category(txn1.id, "Groceries", None)
        .await
        .expect("update suggestion");

    let mut txn2 = make_txn(account.id, "STORE B", 2);
    txn2.suggested_category = Some("Groceries".to_owned());
    db.upsert_transaction(&txn2, Some("sug-2"))
        .await
        .expect("insert");
    db.update_transaction_suggested_category(txn2.id, "Groceries", None)
        .await
        .expect("update suggestion");

    let mut txn3 = make_txn(account.id, "GAS STATION", 3);
    txn3.suggested_category = Some("Transport".to_owned());
    db.upsert_transaction(&txn3, Some("sug-3"))
        .await
        .expect("insert");
    db.update_transaction_suggested_category(txn3.id, "Transport", None)
        .await
        .expect("update suggestion");

    let (status, body) = send(app, get("/api/categories/suggestions")).await;
    assert_eq!(status, StatusCode::OK);

    let suggestions: Vec<serde_json::Value> = serde_json::from_slice(&body).expect("parse");
    assert_eq!(suggestions.len(), 2);
    // Sorted by count descending
    assert_eq!(suggestions[0]["category_name"], "Groceries");
    assert_eq!(suggestions[0]["count"], 2);
    assert_eq!(suggestions[1]["category_name"], "Transport");
    assert_eq!(suggestions[1]["count"], 1);
}

// ===========================================================================
// Connections: authorize
// ===========================================================================

#[sqlx::test]
async fn connections_authorize_returns_501_when_not_configured(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    let payload = serde_json::json!({
        "aspsp_name": "Test Bank",
        "aspsp_country": "FI"
    });

    let (status, _body) = send(app, post_json("/api/connections/authorize", &payload)).await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
}

// ===========================================================================
// Connections: DELETE /{id}
// ===========================================================================

#[sqlx::test]
async fn connections_revoke(pool: PgPool) {
    let (app, db) = setup(pool).await;

    // Seed a connection directly
    let connection = Connection {
        id: ConnectionId::new(),
        provider: "enable_banking".to_owned(),
        provider_session_id: "test-session".to_owned(),
        institution_name: "Test Bank".to_owned(),
        valid_until: chrono::Utc::now() + chrono::Duration::days(90),
        status: ConnectionStatus::Active,
    };
    db.insert_connection(&connection).await.expect("insert");

    let (status, _body) = send(
        app.clone(),
        delete(&format!("/api/connections/{}", connection.id)),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify it's revoked (still exists but status changed)
    let fetched = db.get_connection(connection.id).await.expect("query");
    assert!(fetched.is_some());
    assert_eq!(fetched.unwrap().status, ConnectionStatus::Revoked);
}

#[sqlx::test]
async fn connections_revoke_nonexistent_returns_404(pool: PgPool) {
    let (app, _db) = setup(pool).await;
    let fake_id = uuid::Uuid::new_v4();

    let (status, _body) = send(app, delete(&format!("/api/connections/{fake_id}"))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ===========================================================================
// Import: POST /accounts/{id}/import
// ===========================================================================

#[sqlx::test]
async fn import_csv_nonexistent_account_returns_404(pool: PgPool) {
    let (app, _db) = setup(pool).await;
    let fake_id = uuid::Uuid::new_v4();

    let csv = "Date,Description,Amount\n01/01/2025,Test,10.00";
    let (status, _body) = send(
        app,
        post_text(&format!("/api/accounts/{fake_id}/import"), csv),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn import_csv_invalid_csv_returns_400(pool: PgPool) {
    let (app, db) = setup(pool).await;

    let account = make_account("Import Test");
    db.upsert_account(&account).await.expect("account");

    let bad_csv = "this is not valid amex csv data at all";
    let (status, _body) = send(
        app,
        post_text(&format!("/api/accounts/{}/import", account.id), bad_csv),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ===========================================================================
// Jobs: counts
// ===========================================================================

#[sqlx::test]
async fn jobs_counts_returns_200(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    let (status, body) = send(app, get("/api/jobs/counts")).await;
    assert_eq!(status, StatusCode::OK);

    let counts: Vec<serde_json::Value> = serde_json::from_slice(&body).expect("parse");
    // May be empty if no jobs have ever been enqueued, or may have entries
    for count in &counts {
        assert!(count["job_type"].is_string());
        assert!(count["active"].is_number());
        assert!(count["waiting"].is_number());
    }
}

#[sqlx::test]
async fn jobs_counts_after_enqueue(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    // Enqueue a categorize job
    let (status, _body) = send(app.clone(), post_empty("/api/jobs/categorize")).await;
    assert_eq!(status, StatusCode::ACCEPTED);

    let (status, body) = send(app, get("/api/jobs/counts")).await;
    assert_eq!(status, StatusCode::OK);

    let counts: Vec<serde_json::Value> = serde_json::from_slice(&body).expect("parse");
    let categorize_count = counts.iter().find(|c| {
        c["job_type"]
            .as_str()
            .is_some_and(|s| s.contains("Categorize"))
    });
    assert!(
        categorize_count.is_some(),
        "expected a categorize queue entry, got: {counts:?}"
    );
}

// ===========================================================================
// Jobs: schedule
// ===========================================================================

#[sqlx::test]
async fn jobs_schedule_empty(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    let (status, body) = send(app, get("/api/jobs/schedule")).await;
    assert_eq!(status, StatusCode::OK);

    let schedule: Vec<serde_json::Value> = serde_json::from_slice(&body).expect("parse");
    assert!(schedule.is_empty());
}

// ===========================================================================
// Rules: preview
// ===========================================================================

#[sqlx::test]
async fn rules_preview_no_matches(pool: PgPool) {
    let (app, db) = setup(pool).await;

    let category = make_category("Transport");
    db.insert_category(&category).await.expect("category");

    let payload = serde_json::json!({
        "rule_type": "categorization",
        "conditions": [{"field": "merchant", "pattern": "NONEXISTENT_MERCHANT"}],
        "target_category_id": category.id.to_string(),
        "priority": 10
    });

    let (status, body) = send(app, post_json("/api/rules/preview", &payload)).await;
    assert_eq!(status, StatusCode::OK);

    let resp: serde_json::Value = serde_json::from_slice(&body).expect("parse");
    assert_eq!(resp["match_count"], 0);
    assert!(resp["sample"].as_array().expect("array").is_empty());
}

#[sqlx::test]
async fn rules_preview_with_matches(pool: PgPool) {
    let (app, db) = setup(pool).await;

    let account = make_account("Preview Test");
    db.upsert_account(&account).await.expect("account");

    let category = make_category("Groceries");
    db.insert_category(&category).await.expect("category");

    // Uncategorized transactions that should match
    let txn1 = make_uncategorized_txn(account.id, "LIDL STORE 123", 1);
    let txn2 = make_uncategorized_txn(account.id, "LIDL MARKET 456", 2);
    let txn3 = make_uncategorized_txn(account.id, "ALDI NORD", 3);
    db.upsert_transaction(&txn1, None).await.expect("insert");
    db.upsert_transaction(&txn2, None).await.expect("insert");
    db.upsert_transaction(&txn3, None).await.expect("insert");

    let payload = serde_json::json!({
        "rule_type": "categorization",
        "conditions": [{"field": "merchant", "pattern": "LIDL"}],
        "target_category_id": category.id.to_string(),
        "priority": 10
    });

    let (status, body) = send(app, post_json("/api/rules/preview", &payload)).await;
    assert_eq!(status, StatusCode::OK);

    let resp: serde_json::Value = serde_json::from_slice(&body).expect("parse");
    assert_eq!(resp["match_count"], 2);
    let sample = resp["sample"].as_array().expect("array");
    assert_eq!(sample.len(), 2);
    assert!(
        sample[0]["merchant_name"]
            .as_str()
            .expect("str")
            .contains("LIDL")
    );
}

#[sqlx::test]
async fn rules_preview_invalid_pattern_returns_400(pool: PgPool) {
    let (app, db) = setup(pool).await;

    let category = make_category("Test");
    db.insert_category(&category).await.expect("category");

    let payload = serde_json::json!({
        "rule_type": "categorization",
        "conditions": [{"field": "merchant", "pattern": "[invalid regex"}],
        "target_category_id": category.id.to_string(),
        "priority": 10
    });

    let (status, _body) = send(app, post_json("/api/rules/preview", &payload)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ===========================================================================
// Transactions: GET /{id}
// ===========================================================================

#[sqlx::test]
async fn transactions_get_one(pool: PgPool) {
    let (app, db) = setup(pool).await;

    let account = make_account("Get One Test");
    db.upsert_account(&account).await.expect("account");

    let txn = make_txn(account.id, "TEST MERCHANT", 15);
    db.upsert_transaction(&txn, Some("get-one"))
        .await
        .expect("insert");

    let (status, body) = send(app, get(&format!("/api/transactions/{}", txn.id))).await;
    assert_eq!(status, StatusCode::OK);

    let fetched: Transaction = serde_json::from_slice(&body).expect("parse");
    assert_eq!(fetched.id, txn.id);
    assert_eq!(fetched.merchant_name, "TEST MERCHANT");
    assert_eq!(fetched.account_id, account.id);
}

#[sqlx::test]
async fn transactions_get_one_nonexistent_returns_404(pool: PgPool) {
    let (app, _db) = setup(pool).await;
    let fake_id = uuid::Uuid::new_v4();

    let (status, _body) = send(app, get(&format!("/api/transactions/{fake_id}"))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ===========================================================================
// Transactions: DELETE /{id}/categorize (uncategorize)
// ===========================================================================

#[sqlx::test]
async fn transactions_uncategorize(pool: PgPool) {
    let (app, db) = setup(pool).await;

    let account = make_account("Uncat Test");
    db.upsert_account(&account).await.expect("account");

    let category = make_category("Coffee");
    db.insert_category(&category).await.expect("category");

    // Categorize a transaction
    let mut txn = make_txn(account.id, "STARBUCKS", 5);
    txn.categorization = Categorization::Manual(category.id);
    db.upsert_transaction(&txn, Some("uncat-test"))
        .await
        .expect("insert");
    db.update_transaction_category(
        txn.id,
        category.id,
        budget_core::models::CategoryMethod::Manual,
        None,
    )
    .await
    .expect("categorize");

    // Verify it's categorized
    let before = db
        .get_transaction_by_id(txn.id)
        .await
        .expect("query")
        .expect("found");
    assert_eq!(before.categorization.category_id(), Some(category.id));

    // Uncategorize it
    let (status, _body) = send(
        app,
        delete(&format!("/api/transactions/{}/categorize", txn.id)),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify it's uncategorized
    let after = db
        .get_transaction_by_id(txn.id)
        .await
        .expect("query")
        .expect("found");
    assert_eq!(after.categorization, Categorization::Uncategorized);
}

// ===========================================================================
// Transactions: POST /{id}/skip-correlation
// ===========================================================================

#[sqlx::test]
async fn transactions_skip_correlation(pool: PgPool) {
    let (app, db) = setup(pool).await;

    let account = make_account("Skip Corr Test");
    db.upsert_account(&account).await.expect("account");

    let txn = make_txn(account.id, "TRANSFER", 10);
    db.upsert_transaction(&txn, Some("skip-corr"))
        .await
        .expect("insert");

    // Set skip_correlation = true
    let payload = serde_json::json!({ "skip": true });
    let (status, _body) = send(
        app.clone(),
        post_json(
            &format!("/api/transactions/{}/skip-correlation", txn.id),
            &payload,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify via DB
    let updated = db
        .get_transaction_by_id(txn.id)
        .await
        .expect("query")
        .expect("found");
    assert!(updated.skip_correlation);

    // Unset skip_correlation
    let payload = serde_json::json!({ "skip": false });
    let (status, _body) = send(
        app,
        post_json(
            &format!("/api/transactions/{}/skip-correlation", txn.id),
            &payload,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let restored = db
        .get_transaction_by_id(txn.id)
        .await
        .expect("query")
        .expect("found");
    assert!(!restored.skip_correlation);
}
