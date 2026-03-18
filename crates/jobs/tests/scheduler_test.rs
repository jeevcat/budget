//! Integration tests for the scheduler module.
//!
//! Each test gets its own `PostgreSQL` database (via `sqlx::test`) with all
//! migrations applied. The `scheduler_tick` function accepts an injectable
//! `now` for deterministic time control.

use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;

use budget_core::models::{
    Account, AccountId, AccountOrigin, AccountType, Connection, ConnectionId, ConnectionStatus,
    CurrencyCode,
};
use budget_db::Db;
use budget_jobs::schedule_queries::{
    self, AccountType as ScheduleAccountType, RunStatus, ScheduleRun, TriggerReason,
};
use budget_jobs::scheduler::scheduler_tick;
use budget_jobs::{AmazonSyncJob, ApalisPool, JobStorage, PayPalSyncJob, PipelineStorage};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn setup_db(pool: PgPool) -> (Db, ApalisPool) {
    let db = Db::from_pool(pool.clone());
    apalis_postgres::PostgresStorage::setup(&pool)
        .await
        .expect("apalis setup");
    db.run_migrations().await.expect("domain migrations");
    (db, pool)
}

async fn seed_connection(db: &Db) -> ConnectionId {
    let id = ConnectionId::new();
    let connection = Connection {
        id,
        provider: "enable_banking".to_owned(),
        provider_session_id: format!("mock-session-{id}"),
        institution_name: "Mock Bank".to_owned(),
        valid_until: Utc::now() + Duration::days(90),
        status: ConnectionStatus::Active,
    };
    db.insert_connection(&connection)
        .await
        .expect("seed connection");
    id
}

async fn seed_account(db: &Db, name: &str, connection_id: ConnectionId) -> Account {
    let account = Account {
        id: AccountId::new(),
        provider_account_id: format!("mock-{}", name.to_lowercase().replace(' ', "-")),
        name: name.to_owned(),
        nickname: None,
        institution: "Mock Bank".to_owned(),
        account_type: AccountType::Checking,
        currency: CurrencyCode::new("USD").unwrap(),
        origin: AccountOrigin::Connected(connection_id),
    };
    db.upsert_account(&account).await.expect("seed account");
    account
}

fn make_run(
    account_id: uuid::Uuid,
    status: RunStatus,
    reason: TriggerReason,
    attempt: i32,
    started_at: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>,
    error_message: Option<&str>,
) -> ScheduleRun {
    ScheduleRun {
        id: uuid::Uuid::new_v4(),
        account_id,
        account_type: ScheduleAccountType::Bank,
        status,
        trigger_reason: reason,
        attempt,
        started_at: Some(started_at),
        finished_at,
        next_run_at: None,
        error_message: error_message.map(str::to_owned),
        created_at: started_at,
    }
}

// ===========================================================================
// Tests
// ===========================================================================

/// New account with no prior runs gets a pipeline enqueued immediately.
#[sqlx::test]
async fn scheduler_enqueues_pipeline_for_new_account(pool: PgPool) {
    let (db, apalis_pool) = setup_db(pool).await;
    let conn_id = seed_connection(&db).await;
    let account = seed_account(&db, "Primary Checking", conn_id).await;
    let pipeline_storage = PipelineStorage::new(&apalis_pool);
    let amazon_storage = JobStorage::<AmazonSyncJob>::new(&apalis_pool);
    let paypal_storage = JobStorage::<PayPalSyncJob>::new(&apalis_pool);
    let now = Utc::now();

    scheduler_tick(
        &db,
        &apalis_pool,
        &pipeline_storage,
        &amazon_storage,
        &paypal_storage,
        now,
    )
    .await
    .expect("tick should succeed");

    let account_uuid: uuid::Uuid = account.id.into();
    let run = schedule_queries::get_latest_run_for_account(
        &apalis_pool,
        account_uuid,
        ScheduleAccountType::Bank,
    )
    .await
    .expect("query should succeed")
    .expect("should have a run");

    assert_eq!(run.status, RunStatus::Running);
    assert_eq!(run.trigger_reason, TriggerReason::Scheduled);
    assert_eq!(run.attempt, 1);
    assert_eq!(run.account_id, account_uuid);
}

/// Account with a running pipeline is skipped — no duplicate enqueue.
#[sqlx::test]
async fn scheduler_skips_account_with_running_pipeline(pool: PgPool) {
    let (db, apalis_pool) = setup_db(pool).await;
    let conn_id = seed_connection(&db).await;
    let account = seed_account(&db, "Primary Checking", conn_id).await;
    let account_uuid: uuid::Uuid = account.id.into();
    let now = Utc::now();

    let existing = make_run(
        account_uuid,
        RunStatus::Running,
        TriggerReason::Scheduled,
        1,
        now - Duration::minutes(2),
        None,
        None,
    );
    schedule_queries::insert_schedule_run(&apalis_pool, &existing)
        .await
        .expect("insert run");

    let pipeline_storage = PipelineStorage::new(&apalis_pool);
    let amazon_storage = JobStorage::<AmazonSyncJob>::new(&apalis_pool);
    let paypal_storage = JobStorage::<PayPalSyncJob>::new(&apalis_pool);
    scheduler_tick(
        &db,
        &apalis_pool,
        &pipeline_storage,
        &amazon_storage,
        &paypal_storage,
        now,
    )
    .await
    .expect("tick should succeed");

    // Count all runs — should still be just the one
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM schedule_runs WHERE account_id = $1")
        .bind(account.id)
        .fetch_one(&apalis_pool)
        .await
        .expect("count query");
    assert_eq!(count.0, 1, "no new run should be created");
}

/// Failed run with transient error and elapsed backoff triggers a retry.
#[sqlx::test]
async fn scheduler_retries_failed_transient_error(pool: PgPool) {
    let (db, apalis_pool) = setup_db(pool).await;
    let conn_id = seed_connection(&db).await;
    let account = seed_account(&db, "Primary Checking", conn_id).await;
    let account_uuid: uuid::Uuid = account.id.into();
    let now = Utc::now();

    // Failed 2 minutes ago with transient error, attempt 1
    // Backoff for attempt 1 = 60s, so 2 min > 60s → should retry
    let failed = make_run(
        account_uuid,
        RunStatus::Failed,
        TriggerReason::Scheduled,
        1,
        now - Duration::minutes(3),
        Some(now - Duration::minutes(2)),
        Some("connection failed: timeout"),
    );
    schedule_queries::insert_schedule_run(&apalis_pool, &failed)
        .await
        .expect("insert run");

    let pipeline_storage = PipelineStorage::new(&apalis_pool);
    let amazon_storage = JobStorage::<AmazonSyncJob>::new(&apalis_pool);
    let paypal_storage = JobStorage::<PayPalSyncJob>::new(&apalis_pool);
    scheduler_tick(
        &db,
        &apalis_pool,
        &pipeline_storage,
        &amazon_storage,
        &paypal_storage,
        now,
    )
    .await
    .expect("tick should succeed");

    // Should have 2 runs now — original failed + new retry
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM schedule_runs WHERE account_id = $1")
        .bind(account.id)
        .fetch_one(&apalis_pool)
        .await
        .expect("count query");
    assert_eq!(count.0, 2);

    // Latest run should be a retry at attempt 2
    let latest = schedule_queries::get_latest_run_for_account(
        &apalis_pool,
        account_uuid,
        ScheduleAccountType::Bank,
    )
    .await
    .expect("query")
    .expect("should have latest run");
    assert_eq!(latest.status, RunStatus::Running);
    assert_eq!(latest.trigger_reason, TriggerReason::Retry);
    assert_eq!(latest.attempt, 2);
}

/// Failed run within backoff window is not retried.
#[sqlx::test]
async fn scheduler_respects_backoff_delay(pool: PgPool) {
    let (db, apalis_pool) = setup_db(pool).await;
    let conn_id = seed_connection(&db).await;
    let account = seed_account(&db, "Primary Checking", conn_id).await;
    let account_uuid: uuid::Uuid = account.id.into();
    let now = Utc::now();

    // Failed 30s ago with transient error, attempt 2
    // Backoff for attempt 2 = 120s, so 30s < 120s → should not retry
    let failed = make_run(
        account_uuid,
        RunStatus::Failed,
        TriggerReason::Retry,
        2,
        now - Duration::minutes(1),
        Some(now - Duration::seconds(30)),
        Some("connection failed"),
    );
    schedule_queries::insert_schedule_run(&apalis_pool, &failed)
        .await
        .expect("insert run");

    let pipeline_storage = PipelineStorage::new(&apalis_pool);
    let amazon_storage = JobStorage::<AmazonSyncJob>::new(&apalis_pool);
    let paypal_storage = JobStorage::<PayPalSyncJob>::new(&apalis_pool);
    scheduler_tick(
        &db,
        &apalis_pool,
        &pipeline_storage,
        &amazon_storage,
        &paypal_storage,
        now,
    )
    .await
    .expect("tick should succeed");

    // Should still be just 1 run
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM schedule_runs WHERE account_id = $1")
        .bind(account.id)
        .fetch_one(&apalis_pool)
        .await
        .expect("count query");
    assert_eq!(count.0, 1, "should not retry yet — backoff not elapsed");
}

/// After max retries (5), no more retries — waits for next hourly window.
#[sqlx::test]
async fn scheduler_stops_retrying_after_max_attempts(pool: PgPool) {
    let (db, apalis_pool) = setup_db(pool).await;
    let conn_id = seed_connection(&db).await;
    let account = seed_account(&db, "Primary Checking", conn_id).await;
    let account_uuid: uuid::Uuid = account.id.into();
    let now = Utc::now();

    // Failed 20 minutes ago at attempt 5
    let failed = make_run(
        account_uuid,
        RunStatus::Failed,
        TriggerReason::Retry,
        5,
        now - Duration::minutes(25),
        Some(now - Duration::minutes(20)),
        Some("connection failed"),
    );
    schedule_queries::insert_schedule_run(&apalis_pool, &failed)
        .await
        .expect("insert run");

    let pipeline_storage = PipelineStorage::new(&apalis_pool);
    let amazon_storage = JobStorage::<AmazonSyncJob>::new(&apalis_pool);
    let paypal_storage = JobStorage::<PayPalSyncJob>::new(&apalis_pool);
    scheduler_tick(
        &db,
        &apalis_pool,
        &pipeline_storage,
        &amazon_storage,
        &paypal_storage,
        now,
    )
    .await
    .expect("tick should succeed");

    // Should still be just 1 run — no retry
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM schedule_runs WHERE account_id = $1")
        .bind(account.id)
        .fetch_one(&apalis_pool)
        .await
        .expect("count query");
    assert_eq!(count.0, 1, "should not retry after max attempts");

    // But next_run_at should be set on the existing run
    let latest = schedule_queries::get_latest_run_for_account(
        &apalis_pool,
        account_uuid,
        ScheduleAccountType::Bank,
    )
    .await
    .expect("query")
    .expect("should have latest run");
    assert!(
        latest.next_run_at.is_some(),
        "next_run_at should be set for UI"
    );
}

/// Permanent (non-transient) error skips retry, waits for next hourly window.
#[sqlx::test]
async fn scheduler_does_not_retry_permanent_error(pool: PgPool) {
    let (db, apalis_pool) = setup_db(pool).await;
    let conn_id = seed_connection(&db).await;
    let account = seed_account(&db, "Primary Checking", conn_id).await;
    let account_uuid: uuid::Uuid = account.id.into();
    let now = Utc::now();

    // Failed 2 minutes ago with permanent error
    let failed = make_run(
        account_uuid,
        RunStatus::Failed,
        TriggerReason::Scheduled,
        1,
        now - Duration::minutes(3),
        Some(now - Duration::minutes(2)),
        Some("account not found"),
    );
    schedule_queries::insert_schedule_run(&apalis_pool, &failed)
        .await
        .expect("insert run");

    let pipeline_storage = PipelineStorage::new(&apalis_pool);
    let amazon_storage = JobStorage::<AmazonSyncJob>::new(&apalis_pool);
    let paypal_storage = JobStorage::<PayPalSyncJob>::new(&apalis_pool);
    scheduler_tick(
        &db,
        &apalis_pool,
        &pipeline_storage,
        &amazon_storage,
        &paypal_storage,
        now,
    )
    .await
    .expect("tick should succeed");

    // Should still be just 1 run — no retry for permanent errors
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM schedule_runs WHERE account_id = $1")
        .bind(account.id)
        .fetch_one(&apalis_pool)
        .await
        .expect("count query");
    assert_eq!(count.0, 1, "should not retry permanent error");
}

/// CSV-only accounts (no connection_id) are not scheduled.
#[sqlx::test]
async fn scheduler_skips_csv_only_accounts(pool: PgPool) {
    let (db, apalis_pool) = setup_db(pool).await;
    let now = Utc::now();

    // Create an account with no connection (CSV-only, like Amex)
    let csv_account = Account {
        id: AccountId::new(),
        provider_account_id: "csv-amex".to_owned(),
        name: "Amex CSV".to_owned(),
        nickname: None,
        institution: "American Express".to_owned(),
        account_type: AccountType::Checking,
        currency: CurrencyCode::new("EUR").unwrap(),
        origin: AccountOrigin::Manual,
    };
    db.upsert_account(&csv_account)
        .await
        .expect("seed csv account");

    let pipeline_storage = PipelineStorage::new(&apalis_pool);
    let amazon_storage = JobStorage::<AmazonSyncJob>::new(&apalis_pool);
    let paypal_storage = JobStorage::<PayPalSyncJob>::new(&apalis_pool);
    scheduler_tick(
        &db,
        &apalis_pool,
        &pipeline_storage,
        &amazon_storage,
        &paypal_storage,
        now,
    )
    .await
    .expect("tick should succeed");

    // No schedule runs should exist for the CSV-only account
    let csv_uuid: uuid::Uuid = csv_account.id.into();
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM schedule_runs WHERE account_id = $1")
        .bind(csv_uuid)
        .fetch_one(&apalis_pool)
        .await
        .expect("count query");
    assert_eq!(count.0, 0, "CSV-only account should not be scheduled");
}

/// `complete_schedule_run` correctly marks a run as succeeded.
#[sqlx::test]
async fn pipeline_completion_updates_schedule_run(pool: PgPool) {
    let (db, apalis_pool) = setup_db(pool).await;
    let conn_id = seed_connection(&db).await;
    let account = seed_account(&db, "Primary Checking", conn_id).await;
    let account_uuid: uuid::Uuid = account.id.into();
    let now = Utc::now();

    let running = make_run(
        account_uuid,
        RunStatus::Running,
        TriggerReason::Scheduled,
        1,
        now,
        None,
        None,
    );
    let run_id = running.id;
    schedule_queries::insert_schedule_run(&apalis_pool, &running)
        .await
        .expect("insert run");

    // Simulate pipeline completion
    schedule_queries::complete_schedule_run(&apalis_pool, run_id, RunStatus::Succeeded, None)
        .await
        .expect("complete should succeed");

    let updated = schedule_queries::get_latest_run_for_account(
        &apalis_pool,
        account_uuid,
        ScheduleAccountType::Bank,
    )
    .await
    .expect("query")
    .expect("should have run");

    assert_eq!(updated.status, RunStatus::Succeeded);
    assert!(updated.finished_at.is_some());
    assert!(updated.error_message.is_none());
}

/// `get_all_schedule_status` returns per-account summaries.
#[sqlx::test]
async fn schedule_status_query_returns_summary(pool: PgPool) {
    let (db, apalis_pool) = setup_db(pool).await;
    let conn_id = seed_connection(&db).await;
    let acct1 = seed_account(&db, "Checking", conn_id).await;
    let acct2 = seed_account(&db, "Savings", conn_id).await;
    let now = Utc::now();

    // Checking: succeeded run
    let run1 = make_run(
        acct1.id.into(),
        RunStatus::Succeeded,
        TriggerReason::Scheduled,
        1,
        now - Duration::hours(1),
        Some(now - Duration::minutes(55)),
        None,
    );
    schedule_queries::insert_schedule_run(&apalis_pool, &run1)
        .await
        .expect("insert");

    // Savings: failed run
    let run2 = make_run(
        acct2.id.into(),
        RunStatus::Failed,
        TriggerReason::Scheduled,
        1,
        now - Duration::minutes(10),
        Some(now - Duration::minutes(5)),
        Some("connection failed"),
    );
    schedule_queries::insert_schedule_run(&apalis_pool, &run2)
        .await
        .expect("insert");

    // Also create a CSV-only account (no connection)
    let csv_account = Account {
        id: AccountId::new(),
        provider_account_id: "csv-only".to_owned(),
        name: "CSV Import".to_owned(),
        nickname: None,
        institution: "Manual".to_owned(),
        account_type: AccountType::Checking,
        currency: CurrencyCode::new("EUR").unwrap(),
        origin: AccountOrigin::Manual,
    };
    db.upsert_account(&csv_account)
        .await
        .expect("seed csv account");

    let statuses = schedule_queries::get_all_schedule_status(&apalis_pool)
        .await
        .expect("query");

    // CSV-only account is excluded — only connected accounts appear
    assert_eq!(statuses.len(), 2);
    assert!(statuses.iter().all(|s| s.syncable));
    assert!(!statuses.iter().any(|s| s.account_name == "CSV Import"));

    let checking = statuses
        .iter()
        .find(|s| s.account_name == "Checking")
        .expect("checking");
    assert_eq!(checking.last_run_status, Some(RunStatus::Succeeded));
    assert!(checking.last_error.is_none());

    let savings = statuses
        .iter()
        .find(|s| s.account_name == "Savings")
        .expect("savings");
    assert_eq!(savings.last_run_status, Some(RunStatus::Failed));
    assert_eq!(savings.last_error.as_deref(), Some("connection failed"));
}
