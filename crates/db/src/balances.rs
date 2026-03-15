use budget_core::models::{AccountId, BalanceSnapshot};

use crate::{Db, DbError, row_to_balance_snapshot};

impl Db {
    /// Insert a new balance snapshot.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn insert_balance_snapshot(&self, snapshot: &BalanceSnapshot) -> Result<(), DbError> {
        let pool = &self.0;
        sqlx::query(
            "INSERT INTO balance_snapshots (id, account_id, current_balance, available_balance, currency, snapshot_at)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(snapshot.id)
        .bind(snapshot.account_id)
        .bind(snapshot.current)
        .bind(snapshot.available)
        .bind(&snapshot.currency)
        .bind(snapshot.snapshot_at)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// List balance snapshots for an account, newest first.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn list_balance_snapshots(
        &self,
        account_id: AccountId,
        limit: Option<i64>,
    ) -> Result<Vec<BalanceSnapshot>, DbError> {
        let pool = &self.0;
        let rows = sqlx::query(
            "SELECT id, account_id, current_balance, available_balance, currency, snapshot_at
             FROM balance_snapshots
             WHERE account_id = $1
             ORDER BY snapshot_at DESC
             LIMIT $2",
        )
        .bind(account_id)
        .bind(limit.unwrap_or(1000))
        .fetch_all(pool)
        .await?;
        rows.iter().map(row_to_balance_snapshot).collect()
    }

    /// List all balance snapshots across all accounts, oldest first.
    ///
    /// Used for time series construction (net worth projection).
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn list_all_balance_snapshots(&self) -> Result<Vec<BalanceSnapshot>, DbError> {
        let pool = &self.0;
        let rows = sqlx::query(
            "SELECT id, account_id, current_balance, available_balance, currency, snapshot_at
             FROM balance_snapshots
             ORDER BY snapshot_at ASC",
        )
        .fetch_all(pool)
        .await?;
        rows.iter().map(row_to_balance_snapshot).collect()
    }

    /// Get the most recent balance snapshot per account.
    ///
    /// Uses `DISTINCT ON` to return one row per account, ordered by
    /// `snapshot_at DESC`. Useful for net worth aggregation.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn get_latest_balance_per_account(&self) -> Result<Vec<BalanceSnapshot>, DbError> {
        let pool = &self.0;
        let rows = sqlx::query(
            "SELECT DISTINCT ON (account_id)
                    id, account_id, current_balance, available_balance, currency, snapshot_at
             FROM balance_snapshots
             ORDER BY account_id, snapshot_at DESC",
        )
        .fetch_all(pool)
        .await?;
        rows.iter().map(row_to_balance_snapshot).collect()
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use rust_decimal_macros::dec;
    use sqlx::PgPool;

    use budget_core::models::{
        Account, AccountId, AccountOrigin, AccountType, BalanceSnapshot, BalanceSnapshotId,
        CurrencyCode,
    };

    use crate::Db;

    async fn setup_db(pool: PgPool) -> Db {
        let db = Db::from_pool(pool);
        db.run_migrations().await.expect("migrations");
        db
    }

    fn seed_account_model() -> Account {
        Account {
            id: AccountId::new(),
            provider_account_id: "test-001".to_owned(),
            name: "Test Checking".to_owned(),
            nickname: None,
            institution: "Test Bank".to_owned(),
            account_type: AccountType::Checking,
            currency: "EUR".parse::<CurrencyCode>().expect("valid"),
            origin: AccountOrigin::Manual,
        }
    }

    #[sqlx::test]
    async fn insert_and_list_snapshots(pool: PgPool) {
        let db = setup_db(pool).await;
        let account = seed_account_model();
        db.upsert_account(&account).await.expect("seed account");

        let s1 = BalanceSnapshot {
            id: BalanceSnapshotId::new(),
            account_id: account.id,
            current: dec!(1000.00),
            available: Some(dec!(950.00)),
            currency: "EUR".parse().expect("valid"),
            snapshot_at: Utc::now() - chrono::Duration::hours(2),
        };
        let s2 = BalanceSnapshot {
            id: BalanceSnapshotId::new(),
            account_id: account.id,
            current: dec!(1050.00),
            available: None,
            currency: "EUR".parse().expect("valid"),
            snapshot_at: Utc::now(),
        };

        db.insert_balance_snapshot(&s1).await.expect("insert s1");
        db.insert_balance_snapshot(&s2).await.expect("insert s2");

        let snapshots = db
            .list_balance_snapshots(account.id, None)
            .await
            .expect("list");
        assert_eq!(snapshots.len(), 2);
        // Newest first
        assert_eq!(snapshots[0].current, dec!(1050.00));
        assert_eq!(snapshots[1].current, dec!(1000.00));
    }

    #[sqlx::test]
    async fn list_snapshots_respects_limit(pool: PgPool) {
        let db = setup_db(pool).await;
        let account = seed_account_model();
        db.upsert_account(&account).await.expect("seed account");

        for i in 0..5 {
            let s = BalanceSnapshot {
                id: BalanceSnapshotId::new(),
                account_id: account.id,
                current: rust_decimal::Decimal::from(i * 100),
                available: None,
                currency: "EUR".parse().expect("valid"),
                snapshot_at: Utc::now() + chrono::Duration::seconds(i),
            };
            db.insert_balance_snapshot(&s).await.expect("insert");
        }

        let snapshots = db
            .list_balance_snapshots(account.id, Some(2))
            .await
            .expect("list");
        assert_eq!(snapshots.len(), 2);
    }

    #[sqlx::test]
    async fn list_all_snapshots_ordered_by_time(pool: PgPool) {
        let db = setup_db(pool).await;

        let a1 = seed_account_model();
        db.upsert_account(&a1).await.expect("seed a1");

        let mut a2 = seed_account_model();
        a2.provider_account_id = "test-002".to_owned();
        a2.name = "Savings".to_owned();
        db.upsert_account(&a2).await.expect("seed a2");

        let now = Utc::now();
        for (aid, amount, offset_secs) in [
            (a1.id, dec!(100.00), 0),
            (a2.id, dec!(500.00), 30),
            (a1.id, dec!(200.00), 60),
        ] {
            let s = BalanceSnapshot {
                id: BalanceSnapshotId::new(),
                account_id: aid,
                current: amount,
                available: None,
                currency: "EUR".parse().expect("valid"),
                snapshot_at: now + chrono::Duration::seconds(offset_secs),
            };
            db.insert_balance_snapshot(&s).await.expect("insert");
        }

        let all = db.list_all_balance_snapshots().await.expect("list all");
        assert_eq!(all.len(), 3);
        // Oldest first
        assert_eq!(all[0].current, dec!(100.00));
        assert_eq!(all[1].current, dec!(500.00));
        assert_eq!(all[2].current, dec!(200.00));
        // Multiple accounts present
        assert_ne!(all[0].account_id, all[1].account_id);
    }

    #[sqlx::test]
    async fn latest_balance_per_account(pool: PgPool) {
        let db = setup_db(pool).await;

        let a1 = seed_account_model();
        db.upsert_account(&a1).await.expect("seed a1");

        let mut a2 = seed_account_model();
        a2.provider_account_id = "test-002".to_owned();
        a2.name = "Savings".to_owned();
        db.upsert_account(&a2).await.expect("seed a2");

        // Two snapshots for a1, one for a2
        for (aid, amount, offset_secs) in [
            (a1.id, dec!(100.00), 0),
            (a1.id, dec!(200.00), 60),
            (a2.id, dec!(500.00), 0),
        ] {
            let s = BalanceSnapshot {
                id: BalanceSnapshotId::new(),
                account_id: aid,
                current: amount,
                available: None,
                currency: "EUR".parse().expect("valid"),
                snapshot_at: Utc::now() + chrono::Duration::seconds(offset_secs),
            };
            db.insert_balance_snapshot(&s).await.expect("insert");
        }

        let latest = db.get_latest_balance_per_account().await.expect("latest");
        assert_eq!(latest.len(), 2);

        let a1_latest = latest.iter().find(|s| s.account_id == a1.id).expect("a1");
        assert_eq!(a1_latest.current, dec!(200.00));

        let a2_latest = latest.iter().find(|s| s.account_id == a2.id).expect("a2");
        assert_eq!(a2_latest.current, dec!(500.00));
    }
}
