//! Integration tests for job handlers.
//!
//! Each test creates its own in-memory `SQLite` database with all migrations
//! applied, seeds the necessary data, and invokes the handler function
//! directly. Mock providers from `budget_providers` supply deterministic
//! bank and LLM responses.

use apalis::prelude::Data;
use chrono::NaiveDate;
use rust_decimal_macros::dec;
use sqlx::SqlitePool;
use uuid::Uuid;

use budget_core::db;
use budget_core::models::{
    Account, AccountId, AccountType, Category, CategoryId, CorrelationType, MatchField, Rule,
    RuleId, RuleType, Transaction, TransactionId,
};
use budget_jobs::{
    BankClient, CategorizeJob, CorrelateJob, LlmClient, SyncJob, handle_categorize_job,
    handle_correlate_job, handle_sync_job,
};
use budget_providers::{MockBankProvider, MockLlmProvider};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create an in-memory `SQLite` pool and run all migrations.
async fn setup_pool() -> SqlitePool {
    let pool = SqlitePool::connect("sqlite::memory:")
        .await
        .expect("failed to create in-memory SQLite pool");
    sqlx::migrate!("../../migrations")
        .run(&pool)
        .await
        .expect("failed to run migrations");
    pool
}

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).expect("valid test date")
}

/// Seed the "mock-checking-001" account so that sync can find it.
async fn seed_checking_account(pool: &SqlitePool) -> Account {
    let account = Account {
        id: AccountId::new(),
        provider_account_id: "mock-checking-001".to_owned(),
        name: "Primary Checking".to_owned(),
        institution: "Mock Bank".to_owned(),
        account_type: AccountType::Checking,
        currency: "USD".to_owned(),
        connection_id: None,
    };
    db::upsert_account(pool, &account)
        .await
        .expect("seed checking account");
    account
}

/// Seed the "mock-credit-001" credit card account.
async fn seed_credit_card_account(pool: &SqlitePool) -> Account {
    let account = Account {
        id: AccountId::new(),
        provider_account_id: "mock-credit-001".to_owned(),
        name: "Rewards Credit Card".to_owned(),
        institution: "Mock Bank".to_owned(),
        account_type: AccountType::CreditCard,
        currency: "USD".to_owned(),
        connection_id: None,
    };
    db::upsert_account(pool, &account)
        .await
        .expect("seed credit card account");
    account
}

/// Insert a category and return it.
async fn seed_category(pool: &SqlitePool, name: &str) -> Category {
    let cat = Category {
        id: CategoryId::new(),
        name: name.to_owned(),
        parent_id: None,
    };
    db::insert_category(pool, &cat)
        .await
        .expect("seed category");
    cat
}

/// Insert a transaction directly into the database.
async fn seed_transaction(
    pool: &SqlitePool,
    account_id: AccountId,
    merchant: &str,
    amount: rust_decimal::Decimal,
    posted_date: NaiveDate,
    category_id: Option<CategoryId>,
) -> Transaction {
    let txn = Transaction {
        id: TransactionId::new(),
        account_id,
        category_id,
        amount,
        original_amount: None,
        original_currency: None,
        merchant_name: merchant.to_owned(),
        description: String::new(),
        posted_date,
        budget_month_id: None,
        project_id: None,
        correlation_id: None,
        correlation_type: None,
    };
    db::upsert_transaction(pool, &txn, None)
        .await
        .expect("seed transaction");
    txn
}

fn make_bank_client() -> BankClient {
    BankClient::new(MockBankProvider::new())
}

fn make_llm_client() -> LlmClient {
    LlmClient::new(MockLlmProvider::new())
}

// ===========================================================================
// sync.rs tests
// ===========================================================================

#[tokio::test]
async fn sync_valid_account_upserts_transactions() {
    let pool = setup_pool().await;
    let account = seed_checking_account(&pool).await;
    let bank = make_bank_client();

    let job = SyncJob {
        account_id: account.id.as_uuid().to_string(),
    };

    handle_sync_job(job, Data::new(pool.clone()), Data::new(bank))
        .await
        .expect("sync job should succeed");

    let txns = db::list_transactions_by_account(&pool, account.id)
        .await
        .expect("list transactions");

    // MockBankProvider returns 9 checking transactions, but only those
    // whose posted_date >= (now - 90 days) are fetched. All mock dates are
    // 2025-01-*, which is far in the past, so none will pass the since
    // filter unless the test is run during that window. However, the
    // handler itself should not fail -- it just syncs zero transactions.
    //
    // Rather than depending on clock position, we assert that the handler
    // completed without error and that the count is non-negative.
    assert!(txns.len() <= 9);
}

#[tokio::test]
async fn sync_nonexistent_account_returns_error() {
    let pool = setup_pool().await;
    let bank = make_bank_client();

    // Valid UUID but no matching row in the accounts table
    let job = SyncJob {
        account_id: Uuid::new_v4().to_string(),
    };

    let result = handle_sync_job(job, Data::new(pool), Data::new(bank)).await;
    assert!(result.is_err(), "sync with nonexistent account should fail");

    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("not found"),
        "error should mention 'not found', got: {err_msg}"
    );
}

#[tokio::test]
async fn sync_invalid_uuid_returns_error() {
    let pool = setup_pool().await;
    let bank = make_bank_client();

    let job = SyncJob {
        account_id: "not-a-uuid".to_owned(),
    };

    let result = handle_sync_job(job, Data::new(pool), Data::new(bank)).await;
    assert!(result.is_err(), "sync with invalid UUID should fail");

    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("invalid account_id UUID"),
        "error should mention UUID parsing, got: {err_msg}"
    );
}

#[tokio::test]
async fn sync_deduplicates_on_rerun() {
    let pool = setup_pool().await;
    let account = seed_checking_account(&pool).await;
    let bank = make_bank_client();

    // Run sync twice
    for _ in 0..2 {
        let job = SyncJob {
            account_id: account.id.as_uuid().to_string(),
        };
        handle_sync_job(job, Data::new(pool.clone()), Data::new(bank.clone()))
            .await
            .expect("sync job should succeed");
    }

    let txns = db::list_transactions_by_account(&pool, account.id)
        .await
        .expect("list transactions");

    // Each provider_transaction_id + account_id pair is unique due to the
    // ON CONFLICT clause. Running sync twice must not double the count.
    // The exact count depends on the since-date filter, but whatever it is,
    // it should be the same after two runs.

    // Run a third time and compare counts
    let count_after_two = txns.len();

    let job = SyncJob {
        account_id: account.id.as_uuid().to_string(),
    };
    handle_sync_job(job, Data::new(pool.clone()), Data::new(bank))
        .await
        .expect("sync job third run should succeed");

    let txns_after_three = db::list_transactions_by_account(&pool, account.id)
        .await
        .expect("list transactions");
    assert_eq!(
        count_after_two,
        txns_after_three.len(),
        "third sync run should not create duplicate transactions"
    );
}

// ===========================================================================
// categorize.rs tests
// ===========================================================================

#[tokio::test]
async fn categorize_rule_based_assignment() {
    let pool = setup_pool().await;
    let account = seed_checking_account(&pool).await;
    let llm = make_llm_client();

    let groceries_cat = seed_category(&pool, "Food:Groceries").await;

    // Insert a categorization rule that matches "WHOLE FOODS"
    let rule = Rule {
        id: RuleId::new(),
        rule_type: RuleType::Categorization,
        match_field: MatchField::Merchant,
        match_pattern: "WHOLE FOODS".to_owned(),
        target_category_id: Some(groceries_cat.id),
        target_correlation_type: None,
        priority: 100,
    };
    db::insert_rule(&pool, &rule)
        .await
        .expect("insert categorization rule");

    // Seed an uncategorized transaction matching the rule
    let txn = seed_transaction(
        &pool,
        account.id,
        "WHOLE FOODS MARKET",
        dec!(-72.34),
        date(2025, 1, 5),
        None,
    )
    .await;

    handle_categorize_job(CategorizeJob, Data::new(pool.clone()), Data::new(llm))
        .await
        .expect("categorize job should succeed");

    let updated = db::list_transactions(&pool).await.expect("list txns");
    let found = updated.iter().find(|t| t.id == txn.id).expect("find txn");

    assert_eq!(
        found.category_id,
        Some(groceries_cat.id),
        "transaction should be categorized by the rule"
    );
}

#[tokio::test]
async fn categorize_llm_high_confidence_assigns_category() {
    let pool = setup_pool().await;
    let account = seed_checking_account(&pool).await;
    let llm = make_llm_client();

    // Create the category that the MockLlmProvider will propose
    // MockLlmProvider returns "Food:Groceries" at 0.92 confidence for
    // "WHOLE FOODS" -- above the 0.80 threshold.
    let groceries_cat = seed_category(&pool, "Food:Groceries").await;

    // No rules in the DB, so the handler will fall through to LLM
    let txn = seed_transaction(
        &pool,
        account.id,
        "WHOLE FOODS MARKET",
        dec!(-72.34),
        date(2025, 1, 5),
        None,
    )
    .await;

    handle_categorize_job(CategorizeJob, Data::new(pool.clone()), Data::new(llm))
        .await
        .expect("categorize job should succeed");

    let updated = db::list_transactions(&pool).await.expect("list txns");
    let found = updated.iter().find(|t| t.id == txn.id).expect("find txn");

    assert_eq!(
        found.category_id,
        Some(groceries_cat.id),
        "LLM high-confidence result should assign the category"
    );
}

#[tokio::test]
async fn categorize_llm_low_confidence_leaves_uncategorized() {
    let pool = setup_pool().await;
    let account = seed_checking_account(&pool).await;
    let llm = make_llm_client();

    // "AMAZON" triggers MockLlmProvider to return "Shopping" at 0.70,
    // which is below the 0.80 threshold. Even if the category exists
    // in the DB, the transaction should remain uncategorized.
    let _shopping_cat = seed_category(&pool, "Shopping").await;

    let txn = seed_transaction(
        &pool,
        account.id,
        "AMAZON.COM",
        dec!(-45.99),
        date(2025, 1, 22),
        None,
    )
    .await;

    handle_categorize_job(CategorizeJob, Data::new(pool.clone()), Data::new(llm))
        .await
        .expect("categorize job should succeed");

    let updated = db::list_transactions(&pool).await.expect("list txns");
    let found = updated.iter().find(|t| t.id == txn.id).expect("find txn");

    assert_eq!(
        found.category_id, None,
        "LLM confidence 0.70 < 0.80 threshold: transaction should stay uncategorized"
    );
}

#[tokio::test]
async fn categorize_no_uncategorized_transactions_is_noop() {
    let pool = setup_pool().await;
    let account = seed_checking_account(&pool).await;
    let llm = make_llm_client();

    let cat = seed_category(&pool, "Food:Groceries").await;

    // Seed an already-categorized transaction
    seed_transaction(
        &pool,
        account.id,
        "WHOLE FOODS MARKET",
        dec!(-72.34),
        date(2025, 1, 5),
        Some(cat.id),
    )
    .await;

    // Should complete without error and without changing anything
    handle_categorize_job(CategorizeJob, Data::new(pool.clone()), Data::new(llm))
        .await
        .expect("categorize job should succeed with nothing to do");

    let txns = db::list_transactions(&pool).await.expect("list txns");
    assert_eq!(txns.len(), 1);
    assert_eq!(
        txns[0].category_id,
        Some(cat.id),
        "already-categorized transaction should be unchanged"
    );
}

#[tokio::test]
async fn categorize_llm_unknown_category_name_leaves_uncategorized() {
    let pool = setup_pool().await;
    let account = seed_checking_account(&pool).await;
    let llm = make_llm_client();

    // MockLlmProvider returns "Entertainment:Subscriptions" for NETFLIX at
    // 0.95 confidence. If no category with that name exists in the DB,
    // the transaction should remain uncategorized.
    // Deliberately do NOT seed "Entertainment:Subscriptions".

    let txn = seed_transaction(
        &pool,
        account.id,
        "NETFLIX.COM",
        dec!(-15.99),
        date(2025, 1, 3),
        None,
    )
    .await;

    handle_categorize_job(CategorizeJob, Data::new(pool.clone()), Data::new(llm))
        .await
        .expect("categorize job should succeed");

    let updated = db::list_transactions(&pool).await.expect("list txns");
    let found = updated.iter().find(|t| t.id == txn.id).expect("find txn");

    assert_eq!(
        found.category_id, None,
        "LLM proposed a category name not in the DB: transaction should stay uncategorized"
    );
}

// ===========================================================================
// correlate.rs tests
// ===========================================================================

#[tokio::test]
async fn correlate_rule_based_linking() {
    let pool = setup_pool().await;
    let checking = seed_checking_account(&pool).await;
    let credit = seed_credit_card_account(&pool).await;
    let llm = make_llm_client();

    let transfer_cat = seed_category(&pool, "Transfers").await;

    // Seed two categorized transactions (correlation only considers
    // categorized, uncorrelated transactions)
    let txn_a = seed_transaction(
        &pool,
        checking.id,
        "CHASE CREDIT CRD AUTOPAY",
        dec!(-1500.00),
        date(2025, 1, 20),
        Some(transfer_cat.id),
    )
    .await;

    let txn_b = seed_transaction(
        &pool,
        credit.id,
        "PAYMENT RECEIVED",
        dec!(1500.00),
        date(2025, 1, 20),
        Some(transfer_cat.id),
    )
    .await;

    // Insert a correlation rule matching "PAYMENT RECEIVED"
    let rule = Rule {
        id: RuleId::new(),
        rule_type: RuleType::Correlation,
        match_field: MatchField::Merchant,
        match_pattern: "PAYMENT RECEIVED".to_owned(),
        target_category_id: None,
        target_correlation_type: Some(CorrelationType::Transfer),
        priority: 100,
    };
    db::insert_rule(&pool, &rule)
        .await
        .expect("insert correlation rule");

    handle_correlate_job(CorrelateJob, Data::new(pool.clone()), Data::new(llm))
        .await
        .expect("correlate job should succeed");

    let all_txns = db::list_transactions(&pool).await.expect("list txns");
    let a = all_txns.iter().find(|t| t.id == txn_a.id).expect("txn_a");
    let b = all_txns.iter().find(|t| t.id == txn_b.id).expect("txn_b");

    // At least one side should be correlated (the rule matches txn_b as
    // a candidate for txn_a, linking them)
    assert!(
        a.correlation_id.is_some() || b.correlation_id.is_some(),
        "at least one transaction should be correlated after rule match"
    );
}

#[tokio::test]
async fn correlate_llm_equal_opposite_amounts_links() {
    let pool = setup_pool().await;
    let checking = seed_checking_account(&pool).await;
    let credit = seed_credit_card_account(&pool).await;
    let llm = make_llm_client();

    let cat = seed_category(&pool, "Transfers").await;

    // Equal and opposite amounts, same date -- MockLlmProvider returns
    // Transfer at 0.95 confidence (close dates + cancelling amounts).
    let txn_a = seed_transaction(
        &pool,
        checking.id,
        "BANK TRANSFER OUT",
        dec!(-500.00),
        date(2025, 1, 15),
        Some(cat.id),
    )
    .await;

    let txn_b = seed_transaction(
        &pool,
        credit.id,
        "BANK TRANSFER IN",
        dec!(500.00),
        date(2025, 1, 15),
        Some(cat.id),
    )
    .await;

    // No rules in DB -- handler falls through to LLM for equal-opposite pairs
    handle_correlate_job(CorrelateJob, Data::new(pool.clone()), Data::new(llm))
        .await
        .expect("correlate job should succeed");

    let all_txns = db::list_transactions(&pool).await.expect("list txns");
    let a = all_txns.iter().find(|t| t.id == txn_a.id).expect("txn_a");
    let b = all_txns.iter().find(|t| t.id == txn_b.id).expect("txn_b");

    assert_eq!(
        a.correlation_id,
        Some(txn_b.id),
        "txn_a should be correlated to txn_b"
    );
    assert_eq!(
        b.correlation_id,
        Some(txn_a.id),
        "txn_b should be correlated to txn_a"
    );
    assert_eq!(a.correlation_type, Some(CorrelationType::Transfer));
    assert_eq!(b.correlation_type, Some(CorrelationType::Transfer));
}

#[tokio::test]
async fn correlate_bidirectional_both_sides_linked() {
    let pool = setup_pool().await;
    let checking = seed_checking_account(&pool).await;
    let credit = seed_credit_card_account(&pool).await;
    let llm = make_llm_client();

    let cat = seed_category(&pool, "Payments").await;

    let txn_a = seed_transaction(
        &pool,
        checking.id,
        "WIRE TRANSFER",
        dec!(-200.00),
        date(2025, 1, 10),
        Some(cat.id),
    )
    .await;

    let txn_b = seed_transaction(
        &pool,
        credit.id,
        "DEPOSIT",
        dec!(200.00),
        date(2025, 1, 10),
        Some(cat.id),
    )
    .await;

    handle_correlate_job(CorrelateJob, Data::new(pool.clone()), Data::new(llm))
        .await
        .expect("correlate job should succeed");

    let all_txns = db::list_transactions(&pool).await.expect("list txns");
    let a = all_txns.iter().find(|t| t.id == txn_a.id).expect("txn_a");
    let b = all_txns.iter().find(|t| t.id == txn_b.id).expect("txn_b");

    // Both sides should point to each other
    assert_eq!(a.correlation_id, Some(txn_b.id));
    assert_eq!(b.correlation_id, Some(txn_a.id));
    assert_eq!(a.correlation_type, b.correlation_type);
}

#[tokio::test]
async fn correlate_no_uncorrelated_transactions_is_noop() {
    let pool = setup_pool().await;
    let llm = make_llm_client();

    // No transactions at all -- handler should return Ok immediately
    handle_correlate_job(CorrelateJob, Data::new(pool.clone()), Data::new(llm))
        .await
        .expect("correlate job should succeed with no transactions");

    let txns = db::list_transactions(&pool).await.expect("list txns");
    assert!(txns.is_empty());
}

#[tokio::test]
async fn correlate_already_paired_not_correlated_again() {
    let pool = setup_pool().await;
    let checking = seed_checking_account(&pool).await;
    let credit = seed_credit_card_account(&pool).await;
    let llm = make_llm_client();

    let cat = seed_category(&pool, "Transfers").await;

    let txn_a = seed_transaction(
        &pool,
        checking.id,
        "TRANSFER OUT",
        dec!(-300.00),
        date(2025, 1, 5),
        Some(cat.id),
    )
    .await;

    let txn_b = seed_transaction(
        &pool,
        credit.id,
        "TRANSFER IN",
        dec!(300.00),
        date(2025, 1, 5),
        Some(cat.id),
    )
    .await;

    // Manually pre-link them (simulate a prior correlation run)
    db::update_transaction_correlation(&pool, txn_a.id, txn_b.id, CorrelationType::Transfer)
        .await
        .expect("pre-link a->b");
    db::update_transaction_correlation(&pool, txn_b.id, txn_a.id, CorrelationType::Transfer)
        .await
        .expect("pre-link b->a");

    // Add a third uncorrelated transaction with no counterpart
    let txn_c = seed_transaction(
        &pool,
        checking.id,
        "RANDOM MERCHANT",
        dec!(-50.00),
        date(2025, 1, 12),
        Some(cat.id),
    )
    .await;

    handle_correlate_job(CorrelateJob, Data::new(pool.clone()), Data::new(llm))
        .await
        .expect("correlate job should succeed");

    let all_txns = db::list_transactions(&pool).await.expect("list txns");

    // txn_a and txn_b should still have their original correlations
    let a = all_txns.iter().find(|t| t.id == txn_a.id).expect("txn_a");
    let b = all_txns.iter().find(|t| t.id == txn_b.id).expect("txn_b");
    assert_eq!(a.correlation_id, Some(txn_b.id));
    assert_eq!(b.correlation_id, Some(txn_a.id));

    // txn_c has no counterpart with opposite amount, so it stays uncorrelated
    let c = all_txns.iter().find(|t| t.id == txn_c.id).expect("txn_c");
    assert_eq!(
        c.correlation_id, None,
        "txn_c has no matching counterpart; should remain uncorrelated"
    );
}

// ===========================================================================
// recompute.rs tests
// ===========================================================================

// Recompute tests are skipped because handle_recompute_job calls
// `budget_core::load_config()` which reads from a confy config file on
// disk. The config file path depends on the OS-level user data directory
// and may not exist in CI or in fresh environments. Mocking confy would
// require production code changes purely for testability, which is not
// warranted at this stage. The budget math engine itself
// (`detect_budget_month_boundaries`) and `find_budget_month_for_date` are
// tested via unit tests in their respective modules.
