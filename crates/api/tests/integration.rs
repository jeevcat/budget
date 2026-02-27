use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::middleware;
use sqlx::PgPool;
use tower::ServiceExt;

use api::auth;
use api::routes;
use api::state::AppState;
use budget_jobs::{JobStorage, PipelineStorage};

use budget_core::db::Db;
use budget_core::models::{
    Account, AccountId, AccountType, Category, CategoryId, Rule, RuleCondition, Transaction,
    TransactionId,
};
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
        secret_key: TEST_SECRET.to_owned(),
        sync_storage: JobStorage::new(&pool),
        categorize_storage: JobStorage::new(&pool),
        correlate_storage: JobStorage::new(&pool),
        pipeline_storage: PipelineStorage::new(&pool),
        apalis_pool: pool,
        enable_banking_auth: None,
        llm: LlmClient::new(MockLlmProvider::new()),
        expected_salary_count: 1,
        host: "http://localhost:3000".to_owned(),
    };

    let api_routes = Router::new()
        .nest("/accounts", routes::accounts::router())
        .nest("/transactions", routes::transactions::router())
        .nest("/categories", routes::categories::router())
        .nest("/rules", routes::rules::router())
        .nest("/budgets", routes::budgets::router())
        .nest("/jobs", routes::jobs::router())
        .nest("/connections", routes::connections::router())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_bearer_token,
        ));

    let app = Router::new()
        .route("/health", axum::routing::get(health))
        .merge(routes::connections::callback_router())
        .nest("/api", api_routes)
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
        id: TransactionId::new(),
        account_id,
        category_id: None,
        amount: rust_decimal::Decimal::new(1000, 2),
        original_amount: None,
        original_currency: None,
        merchant_name: merchant.to_owned(),
        description: String::new(),
        posted_date: chrono::NaiveDate::from_ymd_opt(2025, 1, day).expect("date"),
        correlation_id: None,
        correlation_type: None,
        category_method: None,
        suggested_category: None,
        counterparty_name: None,
        counterparty_iban: None,
        counterparty_bic: None,
        bank_transaction_code: None,
        skip_correlation: false,
    }
}

/// Helper: build a category with no budget config.
fn make_category(name: &str) -> Category {
    Category {
        id: CategoryId::new(),
        name: name.to_owned(),
        parent_id: None,
        budget_mode: None,
        budget_amount: None,
        project_start_date: None,
        project_end_date: None,
    }
}

/// Helper: build a basic transaction.
fn make_txn(account_id: AccountId, merchant: &str, day: u32) -> Transaction {
    Transaction {
        id: TransactionId::new(),
        account_id,
        category_id: None,
        amount: rust_decimal::Decimal::new(1000, 2),
        original_amount: None,
        original_currency: None,
        merchant_name: merchant.to_owned(),
        description: String::new(),
        posted_date: chrono::NaiveDate::from_ymd_opt(2025, 1, day).expect("date"),
        correlation_id: None,
        correlation_type: None,
        category_method: None,
        suggested_category: None,
        counterparty_name: None,
        counterparty_iban: None,
        counterparty_bic: None,
        bank_transaction_code: None,
        skip_correlation: false,
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
async fn accounts_create_invalid_account_type_returns_400(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    let payload = serde_json::json!({
        "provider_account_id": "prov-789",
        "name": "Bad Account",
        "institution": "Test Bank",
        "account_type": "nonexistent_type",
        "currency": "USD"
    });

    let (status, _body) = send(app, post_json("/api/accounts", &payload)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ===========================================================================
// Transactions
// ===========================================================================

#[sqlx::test]
async fn transactions_list_empty(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    let (status, body) = send(app, get("/api/transactions")).await;
    assert_eq!(status, StatusCode::OK);

    let txns: Vec<Transaction> = serde_json::from_slice(&body).expect("parse");
    assert!(txns.is_empty());
}

#[sqlx::test]
async fn transactions_uncategorized_returns_only_uncategorized(pool: PgPool) {
    let (app, db) = setup(pool).await;

    // Insert an account directly so we can reference it
    let account = Account {
        id: AccountId::new(),
        provider_account_id: "prov-tx-1".to_owned(),
        name: "Tx Test Account".to_owned(),
        nickname: None,
        institution: "Bank".to_owned(),
        account_type: AccountType::Checking,
        currency: "USD".to_owned(),
        connection_id: None,
    };
    db.upsert_account(&account).await.expect("account");

    let category = make_category("Groceries");
    db.insert_category(&category).await.expect("category");

    // Insert one categorized and one uncategorized transaction
    let mut txn_categorized = make_txn(account.id, "Grocery Store", 15);
    txn_categorized.category_id = Some(category.id);
    txn_categorized.amount = rust_decimal::Decimal::new(2500, 2);
    txn_categorized.description = "Weekly groceries".to_owned();
    db.upsert_transaction(&txn_categorized, Some("txn-cat-1"))
        .await
        .expect("insert categorized");

    let mut txn_uncategorized = make_txn(account.id, "Coffee Shop", 16);
    txn_uncategorized.description = "Morning coffee".to_owned();
    db.upsert_transaction(&txn_uncategorized, Some("txn-uncat-1"))
        .await
        .expect("insert uncategorized");

    // GET /uncategorized should return only the uncategorized one
    let (status, body) = send(app.clone(), get("/api/transactions/uncategorized")).await;
    assert_eq!(status, StatusCode::OK);

    let txns: Vec<Transaction> = serde_json::from_slice(&body).expect("parse");
    assert_eq!(txns.len(), 1);
    assert_eq!(txns[0].merchant_name, "Coffee Shop");
    assert!(txns[0].category_id.is_none());

    // GET / should return both
    let (status, body) = send(app, get("/api/transactions")).await;
    assert_eq!(status, StatusCode::OK);

    let all_txns: Vec<Transaction> = serde_json::from_slice(&body).expect("parse");
    assert_eq!(all_txns.len(), 2);
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
        currency: "USD".to_owned(),
        connection_id: None,
    };
    db.upsert_account(&account).await.expect("account");

    let category = make_category("Dining");
    db.insert_category(&category).await.expect("category");

    let mut txn = make_txn(account.id, "Restaurant", 1);
    txn.amount = rust_decimal::Decimal::new(1500, 2);
    txn.description = "Dinner".to_owned();
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
    assert_eq!(updated.category_id, Some(category.id));
}

#[sqlx::test]
async fn transactions_categorize_invalid_uuid_returns_400(pool: PgPool) {
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
    assert_eq!(status, StatusCode::BAD_REQUEST);
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
    assert!(created.budget_mode.is_none());

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
        created.budget_mode,
        Some(budget_core::models::BudgetMode::Monthly)
    );
    assert_eq!(
        created.budget_amount,
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
        created.budget_mode,
        Some(budget_core::models::BudgetMode::Project)
    );
    assert_eq!(
        created.project_start_date,
        Some(chrono::NaiveDate::from_ymd_opt(2025, 3, 1).expect("date"))
    );
    assert_eq!(
        created.project_end_date,
        Some(chrono::NaiveDate::from_ymd_opt(2025, 6, 30).expect("date"))
    );
}

#[sqlx::test]
async fn categories_update_budget_fields(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    // Create with no budget
    let payload = serde_json::json!({ "name": "Transport" });
    let (status, body) = send(app.clone(), post_json("/api/categories", &payload)).await;
    assert_eq!(status, StatusCode::CREATED);
    let created: Category = serde_json::from_slice(&body).expect("parse");
    assert!(created.budget_mode.is_none());

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
        updated.budget_mode,
        Some(budget_core::models::BudgetMode::Monthly)
    );
    assert_eq!(
        updated.budget_amount,
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
async fn categories_create_invalid_budget_mode_returns_400(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    let payload = serde_json::json!({
        "name": "Bad",
        "budget_mode": "weekly"
    });

    let (status, _body) = send(app, post_json("/api/categories", &payload)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
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
    assert_eq!(created.priority, 10);

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
    assert_eq!(updated.priority, 20);
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
async fn rules_create_invalid_rule_type_returns_400(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    let payload = serde_json::json!({
        "rule_type": "invalid_type",
        "conditions": [{"field": "merchant", "pattern": "TEST"}],
        "priority": 1
    });

    let (status, _body) = send(app, post_json("/api/rules", &payload)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
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
        currency: "USD".to_owned(),
        connection_id: None,
    };
    db.upsert_account(&account).await.expect("account");

    let category = make_category("Groceries");
    db.insert_category(&category).await.expect("category");

    // Manually categorized transaction
    let mut txn = make_txn(account.id, "WHOLE FOODS MARKET", 15);
    txn.category_id = Some(category.id);
    txn.amount = rust_decimal::Decimal::new(7234, 2);
    txn.description = "Weekly groceries".to_owned();
    db.upsert_transaction(&txn, None).await.expect("insert txn");
    db.update_transaction_category(
        txn.id,
        category.id,
        budget_core::models::CategoryMethod::Manual,
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
        currency: "USD".to_owned(),
        connection_id: None,
    };
    db.upsert_account(&account).await.expect("account");

    let category = make_category("Coffee");
    db.insert_category(&category).await.expect("category");

    let mut txn = make_txn(account.id, "STARBUCKS", 10);
    txn.category_id = Some(category.id);
    db.upsert_transaction(&txn, None).await.expect("insert txn");
    db.update_transaction_category(
        txn.id,
        category.id,
        budget_core::models::CategoryMethod::Rule,
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
        currency: "USD".to_owned(),
        connection_id: None,
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
        currency: "USD".to_owned(),
        connection_id: None,
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
        currency: "USD".to_owned(),
        connection_id: None,
    };
    db.upsert_account(&account).await.expect("account");

    let category = make_category("Groceries");
    db.insert_category(&category).await.expect("category");

    // Create a rule matching "LIDL"
    let rule = Rule {
        id: budget_core::models::RuleId::new(),
        rule_type: budget_core::models::RuleType::Categorization,
        conditions: vec![RuleCondition {
            field: budget_core::models::MatchField::Merchant,
            pattern: "LIDL".to_owned(),
        }],
        target_category_id: Some(category.id),
        target_correlation_type: None,
        priority: 10,
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
    assert_eq!(updated_1.category_id, Some(category.id));

    let updated_2 = db
        .get_transaction_by_id(txn_match_2.id)
        .await
        .expect("query")
        .expect("found");
    assert_eq!(updated_2.category_id, Some(category.id));

    let unchanged = db
        .get_transaction_by_id(txn_nomatch.id)
        .await
        .expect("query")
        .expect("found");
    assert!(unchanged.category_id.is_none());
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
        currency: "USD".to_owned(),
        connection_id: None,
    };
    db.upsert_account(&account).await.expect("account");

    let category_a = make_category("Coffee");
    let category_b = make_category("Tea");
    db.insert_category(&category_a).await.expect("category");
    db.insert_category(&category_b).await.expect("category");

    // Rule matches "STARBUCKS"
    let rule = Rule {
        id: budget_core::models::RuleId::new(),
        rule_type: budget_core::models::RuleType::Categorization,
        conditions: vec![RuleCondition {
            field: budget_core::models::MatchField::Merchant,
            pattern: "STARBUCKS".to_owned(),
        }],
        target_category_id: Some(category_a.id),
        target_correlation_type: None,
        priority: 10,
    };
    db.insert_rule(&rule).await.expect("rule");

    // Already-categorized transaction — should not be re-categorized
    let mut txn = make_txn(account.id, "STARBUCKS RESERVE", 1);
    txn.category_id = Some(category_b.id);
    txn.amount = rust_decimal::Decimal::new(550, 2);
    txn.description = "Coffee".to_owned();
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
    assert_eq!(fetched.category_id, Some(category_b.id));
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
        currency: "USD".to_owned(),
        connection_id: None,
    };
    db.upsert_account(&account).await.expect("account");

    // Create a Salary category so months can be derived
    let salary_cat = make_category("Salary");
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
    salary_txn.category_id = Some(salary_cat.id);
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
        created.rule_type,
        budget_core::models::RuleType::Correlation
    );
    assert_eq!(created.conditions.len(), 1);
    assert_eq!(
        created.conditions[0].field,
        budget_core::models::MatchField::AmountRange
    );
    assert_eq!(
        created.target_correlation_type,
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
async fn rules_create_invalid_match_field_returns_400(pool: PgPool) {
    let (app, _db) = setup(pool).await;

    let payload = serde_json::json!({
        "rule_type": "categorization",
        "conditions": [{"field": "bogus_field", "pattern": "TEST"}],
        "priority": 1
    });

    let (status, _body) = send(app, post_json("/api/rules", &payload)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
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
