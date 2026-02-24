use axum::Router;
use axum::body::Body;
use http::Request;
use http::StatusCode;
use sqlx::SqlitePool;
use tower::ServiceExt;

use api::routes;
use api::state::{AppState, JobStorage};

use budget_core::db;
use budget_core::models::{
    Account, AccountId, AccountType, BudgetMonth, BudgetMonthId, BudgetPeriod, Category,
    CategoryId, Project, Rule, Transaction, TransactionId,
};

// ---------------------------------------------------------------------------
// Shared test setup
// ---------------------------------------------------------------------------

/// Create an in-memory `SQLite` database with all migrations applied,
/// build the full application `Router`, and return both.
async fn setup() -> (Router, SqlitePool) {
    let pool = SqlitePool::connect("sqlite::memory:")
        .await
        .expect("in-memory pool");

    // Apalis tables must exist before the domain migrations
    apalis_sqlite::SqliteStorage::setup(&pool)
        .await
        .expect("apalis setup");

    let mut migrator = sqlx::migrate!("../../migrations");
    migrator.set_ignore_missing(true);
    migrator.run(&pool).await.expect("domain migrations");

    let state = AppState {
        pool: pool.clone(),
        sync_storage: JobStorage::new(&pool),
        categorize_storage: JobStorage::new(&pool),
        correlate_storage: JobStorage::new(&pool),
        recompute_storage: JobStorage::new(&pool),
    };

    let app = Router::new()
        .nest("/api/accounts", routes::accounts::router())
        .nest("/api/transactions", routes::transactions::router())
        .nest("/api/categories", routes::categories::router())
        .nest("/api/rules", routes::rules::router())
        .nest("/api/budgets", routes::budgets::router())
        .nest("/api/projects", routes::projects::router())
        .nest("/api/jobs", routes::jobs::router())
        .with_state(state);

    (app, pool)
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

/// Helper: build a JSON POST request.
fn post_json(uri: &str, json: &serde_json::Value) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(json).expect("serialize")))
        .expect("build request")
}

/// Helper: build a JSON PUT request.
fn put_json(uri: &str, json: &serde_json::Value) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .method("PUT")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(json).expect("serialize")))
        .expect("build request")
}

/// Helper: build a GET request.
fn get(uri: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .method("GET")
        .body(Body::empty())
        .expect("build request")
}

/// Helper: build a DELETE request.
fn delete(uri: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .method("DELETE")
        .body(Body::empty())
        .expect("build request")
}

/// Helper: build a POST request with empty body.
fn post_empty(uri: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .method("POST")
        .body(Body::empty())
        .expect("build request")
}

// ===========================================================================
// Accounts
// ===========================================================================

#[tokio::test]
async fn accounts_list_empty() {
    let (app, _pool) = setup().await;
    let (status, body) = send(app, get("/api/accounts")).await;

    assert_eq!(status, StatusCode::OK);
    let accounts: Vec<Account> = serde_json::from_slice(&body).expect("parse");
    assert!(accounts.is_empty());
}

#[tokio::test]
async fn accounts_create_and_list() {
    let (app, _pool) = setup().await;

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

#[tokio::test]
async fn accounts_get_nonexistent_returns_404() {
    let (app, _pool) = setup().await;
    let fake_id = uuid::Uuid::new_v4();

    let (status, _body) = send(app, get(&format!("/api/accounts/{fake_id}"))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn accounts_get_after_creation() {
    let (app, _pool) = setup().await;

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

#[tokio::test]
async fn accounts_create_invalid_account_type_returns_400() {
    let (app, _pool) = setup().await;

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

#[tokio::test]
async fn transactions_list_empty() {
    let (app, _pool) = setup().await;

    let (status, body) = send(app, get("/api/transactions")).await;
    assert_eq!(status, StatusCode::OK);

    let txns: Vec<Transaction> = serde_json::from_slice(&body).expect("parse");
    assert!(txns.is_empty());
}

#[tokio::test]
async fn transactions_uncategorized_returns_only_uncategorized() {
    let (app, pool) = setup().await;

    // Insert an account directly so we can reference it
    let account = Account {
        id: AccountId::new(),
        provider_account_id: "prov-tx-1".to_owned(),
        name: "Tx Test Account".to_owned(),
        institution: "Bank".to_owned(),
        account_type: AccountType::Checking,
        currency: "USD".to_owned(),
    };
    db::upsert_account(&pool, &account).await.expect("account");

    let category = Category {
        id: CategoryId::new(),
        name: "Groceries".to_owned(),
        parent_id: None,
    };
    db::insert_category(&pool, &category)
        .await
        .expect("category");

    // Insert one categorized and one uncategorized transaction
    let txn_categorized = Transaction {
        id: TransactionId::new(),
        account_id: account.id,
        category_id: Some(category.id),
        amount: rust_decimal::Decimal::new(2500, 2),
        original_amount: None,
        original_currency: None,
        merchant_name: "Grocery Store".to_owned(),
        description: "Weekly groceries".to_owned(),
        posted_date: chrono::NaiveDate::from_ymd_opt(2025, 1, 15).expect("date"),
        budget_month_id: None,
        project_id: None,
        correlation_id: None,
        correlation_type: None,
    };
    db::upsert_transaction(&pool, &txn_categorized, Some("txn-cat-1"))
        .await
        .expect("insert categorized");

    let txn_uncategorized = Transaction {
        id: TransactionId::new(),
        account_id: account.id,
        category_id: None,
        amount: rust_decimal::Decimal::new(1000, 2),
        original_amount: None,
        original_currency: None,
        merchant_name: "Coffee Shop".to_owned(),
        description: "Morning coffee".to_owned(),
        posted_date: chrono::NaiveDate::from_ymd_opt(2025, 1, 16).expect("date"),
        budget_month_id: None,
        project_id: None,
        correlation_id: None,
        correlation_type: None,
    };
    db::upsert_transaction(&pool, &txn_uncategorized, Some("txn-uncat-1"))
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

#[tokio::test]
async fn transactions_categorize_success() {
    let (app, pool) = setup().await;

    let account = Account {
        id: AccountId::new(),
        provider_account_id: "prov-cat-test".to_owned(),
        name: "Cat Test".to_owned(),
        institution: "Bank".to_owned(),
        account_type: AccountType::Checking,
        currency: "USD".to_owned(),
    };
    db::upsert_account(&pool, &account).await.expect("account");

    let category = Category {
        id: CategoryId::new(),
        name: "Dining".to_owned(),
        parent_id: None,
    };
    db::insert_category(&pool, &category)
        .await
        .expect("category");

    let txn = Transaction {
        id: TransactionId::new(),
        account_id: account.id,
        category_id: None,
        amount: rust_decimal::Decimal::new(1500, 2),
        original_amount: None,
        original_currency: None,
        merchant_name: "Restaurant".to_owned(),
        description: "Dinner".to_owned(),
        posted_date: chrono::NaiveDate::from_ymd_opt(2025, 2, 1).expect("date"),
        budget_month_id: None,
        project_id: None,
        correlation_id: None,
        correlation_type: None,
    };
    db::upsert_transaction(&pool, &txn, Some("txn-to-categorize"))
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
    assert_eq!(status, StatusCode::OK);

    // Verify via DB that the category was set
    let updated_txns = db::list_transactions(&pool).await.expect("list");
    let updated = updated_txns
        .iter()
        .find(|t| t.id == txn.id)
        .expect("find txn");
    assert_eq!(updated.category_id, Some(category.id));
}

#[tokio::test]
async fn transactions_categorize_invalid_uuid_returns_400() {
    let (app, _pool) = setup().await;

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

#[tokio::test]
async fn categories_create_and_list() {
    let (app, _pool) = setup().await;

    let payload = serde_json::json!({
        "name": "Entertainment"
    });

    let (status, body) = send(app.clone(), post_json("/api/categories", &payload)).await;
    assert_eq!(status, StatusCode::CREATED);

    let created: Category = serde_json::from_slice(&body).expect("parse");
    assert_eq!(created.name, "Entertainment");
    assert!(created.parent_id.is_none());

    // List should contain the created category
    let (status, body) = send(app, get("/api/categories")).await;
    assert_eq!(status, StatusCode::OK);

    let cats: Vec<Category> = serde_json::from_slice(&body).expect("parse");
    assert_eq!(cats.len(), 1);
    assert_eq!(cats[0].name, "Entertainment");
}

#[tokio::test]
async fn categories_delete_returns_204() {
    let (app, _pool) = setup().await;

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

#[tokio::test]
async fn categories_create_with_parent() {
    let (app, _pool) = setup().await;

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

// ===========================================================================
// Rules
// ===========================================================================

#[tokio::test]
async fn rules_create_and_list() {
    let (app, pool) = setup().await;

    // Rules with target_category_id need a valid category for context,
    // but the DB schema only has a FK constraint. Insert a category first.
    let category = Category {
        id: CategoryId::new(),
        name: "Groceries".to_owned(),
        parent_id: None,
    };
    db::insert_category(&pool, &category)
        .await
        .expect("category");

    let payload = serde_json::json!({
        "rule_type": "categorization",
        "match_field": "merchant",
        "match_pattern": "GROCERY.*",
        "target_category_id": category.id.to_string(),
        "priority": 10
    });

    let (status, body) = send(app.clone(), post_json("/api/rules", &payload)).await;
    assert_eq!(status, StatusCode::CREATED);

    let created: Rule = serde_json::from_slice(&body).expect("parse");
    assert_eq!(created.match_pattern, "GROCERY.*");
    assert_eq!(created.priority, 10);

    // List should contain the created rule
    let (status, body) = send(app, get("/api/rules")).await;
    assert_eq!(status, StatusCode::OK);

    let rules: Vec<Rule> = serde_json::from_slice(&body).expect("parse");
    assert_eq!(rules.len(), 1);
}

#[tokio::test]
async fn rules_update() {
    let (app, pool) = setup().await;

    let category = Category {
        id: CategoryId::new(),
        name: "Transport".to_owned(),
        parent_id: None,
    };
    db::insert_category(&pool, &category)
        .await
        .expect("category");

    let payload = serde_json::json!({
        "rule_type": "categorization",
        "match_field": "merchant",
        "match_pattern": "UBER.*",
        "target_category_id": category.id.to_string(),
        "priority": 5
    });

    let (_, body) = send(app.clone(), post_json("/api/rules", &payload)).await;
    let created: Rule = serde_json::from_slice(&body).expect("parse");

    // Update the rule's priority
    let update_payload = serde_json::json!({
        "rule_type": "categorization",
        "match_field": "merchant",
        "match_pattern": "UBER.*|LYFT.*",
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
    assert_eq!(updated.match_pattern, "UBER.*|LYFT.*");
    assert_eq!(updated.id, created.id);
}

#[tokio::test]
async fn rules_delete() {
    let (app, _pool) = setup().await;

    let payload = serde_json::json!({
        "rule_type": "correlation",
        "match_field": "description",
        "match_pattern": "TRANSFER.*",
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

#[tokio::test]
async fn rules_create_invalid_rule_type_returns_400() {
    let (app, _pool) = setup().await;

    let payload = serde_json::json!({
        "rule_type": "invalid_type",
        "match_field": "merchant",
        "match_pattern": "TEST",
        "priority": 1
    });

    let (status, _body) = send(app, post_json("/api/rules", &payload)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ===========================================================================
// Budgets
// ===========================================================================

#[tokio::test]
async fn budgets_status_returns_404_when_no_months() {
    let (app, _pool) = setup().await;

    let (status, _body) = send(app, get("/api/budgets/status")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn budgets_create_period() {
    let (app, pool) = setup().await;

    let category = Category {
        id: CategoryId::new(),
        name: "Rent".to_owned(),
        parent_id: None,
    };
    db::insert_category(&pool, &category)
        .await
        .expect("category");

    let payload = serde_json::json!({
        "category_id": category.id.to_string(),
        "period_type": "monthly",
        "amount": "1500.00"
    });

    let (status, body) = send(app, post_json("/api/budgets/periods", &payload)).await;
    assert_eq!(status, StatusCode::CREATED);

    let created: BudgetPeriod = serde_json::from_slice(&body).expect("parse");
    assert_eq!(created.category_id, category.id);
    assert_eq!(created.amount, rust_decimal::Decimal::new(150_000, 2));
}

#[tokio::test]
async fn budgets_months_empty() {
    let (app, _pool) = setup().await;

    let (status, body) = send(app, get("/api/budgets/months")).await;
    assert_eq!(status, StatusCode::OK);

    let months: Vec<BudgetMonth> = serde_json::from_slice(&body).expect("parse");
    assert!(months.is_empty());
}

#[tokio::test]
async fn budgets_delete_period() {
    let (app, pool) = setup().await;

    let category = Category {
        id: CategoryId::new(),
        name: "Utils".to_owned(),
        parent_id: None,
    };
    db::insert_category(&pool, &category)
        .await
        .expect("category");

    let payload = serde_json::json!({
        "category_id": category.id.to_string(),
        "period_type": "annual",
        "amount": "600.00"
    });

    let (status, body) = send(app.clone(), post_json("/api/budgets/periods", &payload)).await;
    assert_eq!(status, StatusCode::CREATED);
    let created: BudgetPeriod = serde_json::from_slice(&body).expect("parse");

    let (status, _body) = send(
        app.clone(),
        delete(&format!("/api/budgets/periods/{}", created.id)),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify the period is gone via the DB
    let periods = db::list_budget_periods(&pool).await.expect("list");
    assert!(periods.is_empty());
}

#[tokio::test]
async fn budgets_status_with_current_month() {
    let (app, pool) = setup().await;

    let category = Category {
        id: CategoryId::new(),
        name: "Food".to_owned(),
        parent_id: None,
    };
    db::insert_category(&pool, &category)
        .await
        .expect("category");

    // Create a budget period
    let bp_payload = serde_json::json!({
        "category_id": category.id.to_string(),
        "period_type": "monthly",
        "amount": "500.00"
    });
    let (status, _) = send(app.clone(), post_json("/api/budgets/periods", &bp_payload)).await;
    assert_eq!(status, StatusCode::CREATED);

    // Insert a budget month with no end_date (current month)
    let month = BudgetMonth {
        id: BudgetMonthId::new(),
        start_date: chrono::NaiveDate::from_ymd_opt(2025, 1, 1).expect("date"),
        end_date: None,
        salary_transactions_detected: 1,
    };
    db::replace_budget_months(&pool, &[month])
        .await
        .expect("months");

    let (status, body) = send(app, get("/api/budgets/status")).await;
    assert_eq!(status, StatusCode::OK);

    let statuses: Vec<budget_core::models::BudgetStatus> =
        serde_json::from_slice(&body).expect("parse");
    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].category_id, category.id);
}

// ===========================================================================
// Projects
// ===========================================================================

#[tokio::test]
async fn projects_create_and_list() {
    let (app, pool) = setup().await;

    let category = Category {
        id: CategoryId::new(),
        name: "Home".to_owned(),
        parent_id: None,
    };
    db::insert_category(&pool, &category)
        .await
        .expect("category");

    let payload = serde_json::json!({
        "name": "Kitchen Renovation",
        "category_id": category.id.to_string(),
        "start_date": "2025-03-01",
        "end_date": "2025-06-30",
        "budget_amount": "10000.00"
    });

    let (status, body) = send(app.clone(), post_json("/api/projects", &payload)).await;
    assert_eq!(status, StatusCode::CREATED);

    let created: Project = serde_json::from_slice(&body).expect("parse");
    assert_eq!(created.name, "Kitchen Renovation");
    assert_eq!(created.category_id, category.id);
    assert_eq!(
        created.start_date,
        chrono::NaiveDate::from_ymd_opt(2025, 3, 1).expect("date")
    );
    assert_eq!(
        created.end_date,
        Some(chrono::NaiveDate::from_ymd_opt(2025, 6, 30).expect("date"))
    );
    assert_eq!(
        created.budget_amount,
        Some(rust_decimal::Decimal::new(1_000_000, 2))
    );

    // List should contain the project
    let (status, body) = send(app, get("/api/projects")).await;
    assert_eq!(status, StatusCode::OK);

    let projects: Vec<Project> = serde_json::from_slice(&body).expect("parse");
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].name, "Kitchen Renovation");
}

#[tokio::test]
async fn projects_update() {
    let (app, pool) = setup().await;

    let category = Category {
        id: CategoryId::new(),
        name: "Travel".to_owned(),
        parent_id: None,
    };
    db::insert_category(&pool, &category)
        .await
        .expect("category");

    let payload = serde_json::json!({
        "name": "Trip to Paris",
        "category_id": category.id.to_string(),
        "start_date": "2025-07-01"
    });

    let (_, body) = send(app.clone(), post_json("/api/projects", &payload)).await;
    let created: Project = serde_json::from_slice(&body).expect("parse");

    let update_payload = serde_json::json!({
        "name": "Trip to Tokyo",
        "category_id": category.id.to_string(),
        "start_date": "2025-08-01",
        "end_date": "2025-08-15",
        "budget_amount": "5000.00"
    });

    let (status, body) = send(
        app,
        put_json(&format!("/api/projects/{}", created.id), &update_payload),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let updated: Project = serde_json::from_slice(&body).expect("parse");
    assert_eq!(updated.name, "Trip to Tokyo");
    assert_eq!(updated.id, created.id);
    assert_eq!(
        updated.start_date,
        chrono::NaiveDate::from_ymd_opt(2025, 8, 1).expect("date")
    );
}

#[tokio::test]
async fn projects_delete() {
    let (app, pool) = setup().await;

    let category = Category {
        id: CategoryId::new(),
        name: "Misc".to_owned(),
        parent_id: None,
    };
    db::insert_category(&pool, &category)
        .await
        .expect("category");

    let payload = serde_json::json!({
        "name": "Temp Project",
        "category_id": category.id.to_string(),
        "start_date": "2025-01-01"
    });

    let (status, body) = send(app.clone(), post_json("/api/projects", &payload)).await;
    assert_eq!(status, StatusCode::CREATED);
    let created: Project = serde_json::from_slice(&body).expect("parse");

    let (status, _body) = send(
        app.clone(),
        delete(&format!("/api/projects/{}", created.id)),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify it's gone
    let (status, body) = send(app, get("/api/projects")).await;
    assert_eq!(status, StatusCode::OK);
    let projects: Vec<Project> = serde_json::from_slice(&body).expect("parse");
    assert!(projects.is_empty());
}

#[tokio::test]
async fn projects_create_invalid_date_returns_400() {
    let (app, pool) = setup().await;

    let category = Category {
        id: CategoryId::new(),
        name: "Bad Date Category".to_owned(),
        parent_id: None,
    };
    db::insert_category(&pool, &category)
        .await
        .expect("category");

    let payload = serde_json::json!({
        "name": "Bad Date Project",
        "category_id": category.id.to_string(),
        "start_date": "not-a-date"
    });

    let (status, _body) = send(app, post_json("/api/projects", &payload)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ===========================================================================
// Jobs
// ===========================================================================

#[tokio::test]
async fn jobs_sync_returns_202() {
    let (app, _pool) = setup().await;
    let account_id = uuid::Uuid::new_v4();

    let (status, _body) = send(app, post_empty(&format!("/api/jobs/sync/{account_id}"))).await;
    assert_eq!(status, StatusCode::ACCEPTED);
}

#[tokio::test]
async fn jobs_categorize_returns_202() {
    let (app, _pool) = setup().await;

    let (status, _body) = send(app, post_empty("/api/jobs/categorize")).await;
    assert_eq!(status, StatusCode::ACCEPTED);
}

#[tokio::test]
async fn jobs_correlate_returns_202() {
    let (app, _pool) = setup().await;

    let (status, _body) = send(app, post_empty("/api/jobs/correlate")).await;
    assert_eq!(status, StatusCode::ACCEPTED);
}

#[tokio::test]
async fn jobs_recompute_returns_202() {
    let (app, _pool) = setup().await;

    let (status, _body) = send(app, post_empty("/api/jobs/recompute")).await;
    assert_eq!(status, StatusCode::ACCEPTED);
}

// ===========================================================================
// Additional edge case tests
// ===========================================================================

#[tokio::test]
async fn accounts_get_invalid_uuid_returns_400() {
    let (app, _pool) = setup().await;

    let (status, _body) = send(app, get("/api/accounts/not-a-uuid")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn categories_delete_nonexistent_returns_204() {
    // DELETE on a nonexistent row is not an error -- the SQL succeeds with 0 rows affected
    let (app, _pool) = setup().await;
    let fake_id = uuid::Uuid::new_v4();

    let (status, _body) = send(app, delete(&format!("/api/categories/{fake_id}"))).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn rules_create_correlation_rule() {
    let (app, _pool) = setup().await;

    let payload = serde_json::json!({
        "rule_type": "correlation",
        "match_field": "amount_range",
        "match_pattern": "100-500",
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
    assert_eq!(
        created.match_field,
        budget_core::models::MatchField::AmountRange
    );
    assert_eq!(
        created.target_correlation_type,
        Some(budget_core::models::CorrelationType::Reimbursement)
    );
}

#[tokio::test]
async fn budgets_update_period() {
    let (app, pool) = setup().await;

    let category = Category {
        id: CategoryId::new(),
        name: "Insurance".to_owned(),
        parent_id: None,
    };
    db::insert_category(&pool, &category)
        .await
        .expect("category");

    let payload = serde_json::json!({
        "category_id": category.id.to_string(),
        "period_type": "monthly",
        "amount": "200.00"
    });

    let (status, body) = send(app.clone(), post_json("/api/budgets/periods", &payload)).await;
    assert_eq!(status, StatusCode::CREATED);
    let created: BudgetPeriod = serde_json::from_slice(&body).expect("parse");

    let update_payload = serde_json::json!({
        "category_id": category.id.to_string(),
        "period_type": "annual",
        "amount": "2400.00"
    });

    let (status, body) = send(
        app,
        put_json(
            &format!("/api/budgets/periods/{}", created.id),
            &update_payload,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let updated: BudgetPeriod = serde_json::from_slice(&body).expect("parse");
    assert_eq!(updated.id, created.id);
    assert_eq!(updated.period_type, budget_core::models::PeriodType::Annual);
    assert_eq!(updated.amount, rust_decimal::Decimal::new(240_000, 2));
}

#[tokio::test]
async fn projects_create_without_optional_fields() {
    let (app, pool) = setup().await;

    let category = Category {
        id: CategoryId::new(),
        name: "Minimal".to_owned(),
        parent_id: None,
    };
    db::insert_category(&pool, &category)
        .await
        .expect("category");

    let payload = serde_json::json!({
        "name": "Minimal Project",
        "category_id": category.id.to_string(),
        "start_date": "2025-05-01"
    });

    let (status, body) = send(app, post_json("/api/projects", &payload)).await;
    assert_eq!(status, StatusCode::CREATED);

    let created: Project = serde_json::from_slice(&body).expect("parse");
    assert_eq!(created.name, "Minimal Project");
    assert!(created.end_date.is_none());
    assert!(created.budget_amount.is_none());
}

#[tokio::test]
async fn transactions_categorize_with_invalid_txn_id_returns_400() {
    let (app, _pool) = setup().await;

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

#[tokio::test]
async fn rules_create_invalid_match_field_returns_400() {
    let (app, _pool) = setup().await;

    let payload = serde_json::json!({
        "rule_type": "categorization",
        "match_field": "bogus_field",
        "match_pattern": "TEST",
        "priority": 1
    });

    let (status, _body) = send(app, post_json("/api/rules", &payload)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn budgets_create_period_invalid_amount_returns_400() {
    let (app, pool) = setup().await;

    let category = Category {
        id: CategoryId::new(),
        name: "BadAmount".to_owned(),
        parent_id: None,
    };
    db::insert_category(&pool, &category)
        .await
        .expect("category");

    let payload = serde_json::json!({
        "category_id": category.id.to_string(),
        "period_type": "monthly",
        "amount": "not-a-number"
    });

    let (status, _body) = send(app, post_json("/api/budgets/periods", &payload)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn accounts_create_all_types() {
    let (app, _pool) = setup().await;

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

#[tokio::test]
async fn budgets_create_period_invalid_period_type_returns_400() {
    let (app, pool) = setup().await;

    let category = Category {
        id: CategoryId::new(),
        name: "BadPeriod".to_owned(),
        parent_id: None,
    };
    db::insert_category(&pool, &category)
        .await
        .expect("category");

    let payload = serde_json::json!({
        "category_id": category.id.to_string(),
        "period_type": "weekly",
        "amount": "100.00"
    });

    let (status, _body) = send(app, post_json("/api/budgets/periods", &payload)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn projects_create_invalid_category_uuid_returns_400() {
    let (app, _pool) = setup().await;

    let payload = serde_json::json!({
        "name": "Bad UUID Project",
        "category_id": "not-a-uuid",
        "start_date": "2025-01-01"
    });

    let (status, _body) = send(app, post_json("/api/projects", &payload)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
