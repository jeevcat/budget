use chrono::NaiveDate;
use rust_decimal::Decimal;
use sqlx::any::AnyRow;
use sqlx::{AnyPool, Row};
use uuid::Uuid;

use crate::models::{
    Account, AccountId, AccountType, BudgetMonth, BudgetMonthId, BudgetPeriod, BudgetPeriodId,
    Category, CategoryId, Connection, ConnectionId, ConnectionStatus, CorrelationType, MatchField,
    PeriodType, Project, ProjectId, Rule, RuleId, RuleType, Transaction, TransactionId,
};

// ---------------------------------------------------------------------------
// Private parse helpers
// ---------------------------------------------------------------------------

fn parse_uuid(row: &AnyRow, col: &str) -> Result<Uuid, sqlx::Error> {
    let s: String = row.try_get(col)?;
    s.parse::<Uuid>().map_err(|e| sqlx::Error::ColumnDecode {
        index: col.to_owned(),
        source: Box::new(e),
    })
}

fn parse_uuid_opt(row: &AnyRow, col: &str) -> Result<Option<Uuid>, sqlx::Error> {
    let s: Option<String> = row.try_get(col)?;
    s.map(|v| {
        v.parse::<Uuid>().map_err(|e| sqlx::Error::ColumnDecode {
            index: col.to_owned(),
            source: Box::new(e),
        })
    })
    .transpose()
}

fn parse_decimal(row: &AnyRow, col: &str) -> Result<Decimal, sqlx::Error> {
    let s: String = row.try_get(col)?;
    s.parse::<Decimal>().map_err(|e| sqlx::Error::ColumnDecode {
        index: col.to_owned(),
        source: Box::new(e),
    })
}

fn parse_decimal_opt(row: &AnyRow, col: &str) -> Result<Option<Decimal>, sqlx::Error> {
    let s: Option<String> = row.try_get(col)?;
    s.map(|v| {
        v.parse::<Decimal>().map_err(|e| sqlx::Error::ColumnDecode {
            index: col.to_owned(),
            source: Box::new(e),
        })
    })
    .transpose()
}

fn parse_date(row: &AnyRow, col: &str) -> Result<NaiveDate, sqlx::Error> {
    let s: String = row.try_get(col)?;
    NaiveDate::parse_from_str(&s, "%Y-%m-%d").map_err(|e| sqlx::Error::ColumnDecode {
        index: col.to_owned(),
        source: Box::new(e),
    })
}

fn parse_date_opt(row: &AnyRow, col: &str) -> Result<Option<NaiveDate>, sqlx::Error> {
    let s: Option<String> = row.try_get(col)?;
    s.map(|v| {
        NaiveDate::parse_from_str(&v, "%Y-%m-%d").map_err(|e| sqlx::Error::ColumnDecode {
            index: col.to_owned(),
            source: Box::new(e),
        })
    })
    .transpose()
}

fn parse_enum<T: std::str::FromStr>(row: &AnyRow, col: &str) -> Result<T, sqlx::Error>
where
    T::Err: std::error::Error + Send + Sync + 'static,
{
    let s: String = row.try_get(col)?;
    s.parse::<T>().map_err(|e| sqlx::Error::ColumnDecode {
        index: col.to_owned(),
        source: Box::new(e),
    })
}

fn parse_enum_opt<T: std::str::FromStr>(row: &AnyRow, col: &str) -> Result<Option<T>, sqlx::Error>
where
    T::Err: std::error::Error + Send + Sync + 'static,
{
    let s: Option<String> = row.try_get(col)?;
    s.map(|v| {
        v.parse::<T>().map_err(|e| sqlx::Error::ColumnDecode {
            index: col.to_owned(),
            source: Box::new(e),
        })
    })
    .transpose()
}

// ---------------------------------------------------------------------------
// Row-to-domain mappers
// ---------------------------------------------------------------------------

fn row_to_account(row: &AnyRow) -> Result<Account, sqlx::Error> {
    Ok(Account {
        id: AccountId::from_uuid(parse_uuid(row, "id")?),
        provider_account_id: row.try_get("provider_account_id")?,
        name: row.try_get("name")?,
        institution: row.try_get("institution")?,
        account_type: parse_enum::<AccountType>(row, "account_type")?,
        currency: row.try_get("currency")?,
        connection_id: parse_uuid_opt(row, "connection_id")?.map(ConnectionId::from_uuid),
    })
}

fn row_to_connection(row: &AnyRow) -> Result<Connection, sqlx::Error> {
    Ok(Connection {
        id: ConnectionId::from_uuid(parse_uuid(row, "id")?),
        provider: row.try_get("provider")?,
        provider_session_id: row.try_get("provider_session_id")?,
        institution_name: row.try_get("institution_name")?,
        valid_until: row.try_get("valid_until")?,
        status: parse_enum::<ConnectionStatus>(row, "status")?,
    })
}

fn row_to_category(row: &AnyRow) -> Result<Category, sqlx::Error> {
    Ok(Category {
        id: CategoryId::from_uuid(parse_uuid(row, "id")?),
        name: row.try_get("name")?,
        parent_id: parse_uuid_opt(row, "parent_id")?.map(CategoryId::from_uuid),
    })
}

fn row_to_transaction(row: &AnyRow) -> Result<Transaction, sqlx::Error> {
    Ok(Transaction {
        id: TransactionId::from_uuid(parse_uuid(row, "id")?),
        account_id: AccountId::from_uuid(parse_uuid(row, "account_id")?),
        category_id: parse_uuid_opt(row, "category_id")?.map(CategoryId::from_uuid),
        amount: parse_decimal(row, "amount")?,
        original_amount: parse_decimal_opt(row, "original_amount")?,
        original_currency: row.try_get("original_currency")?,
        merchant_name: row.try_get("merchant_name")?,
        description: row.try_get("description")?,
        posted_date: parse_date(row, "posted_date")?,
        budget_month_id: parse_uuid_opt(row, "budget_month_id")?.map(BudgetMonthId::from_uuid),
        project_id: parse_uuid_opt(row, "project_id")?.map(ProjectId::from_uuid),
        correlation_id: parse_uuid_opt(row, "correlation_id")?.map(TransactionId::from_uuid),
        correlation_type: parse_enum_opt::<CorrelationType>(row, "correlation_type")?,
        suggested_category: row.try_get("suggested_category")?,
    })
}

fn row_to_rule(row: &AnyRow) -> Result<Rule, sqlx::Error> {
    Ok(Rule {
        id: RuleId::from_uuid(parse_uuid(row, "id")?),
        rule_type: parse_enum::<RuleType>(row, "rule_type")?,
        match_field: parse_enum::<MatchField>(row, "match_field")?,
        match_pattern: row.try_get("match_pattern")?,
        target_category_id: parse_uuid_opt(row, "target_category_id")?.map(CategoryId::from_uuid),
        target_correlation_type: parse_enum_opt::<CorrelationType>(row, "target_correlation_type")?,
        priority: row.try_get("priority")?,
    })
}

fn row_to_budget_period(row: &AnyRow) -> Result<BudgetPeriod, sqlx::Error> {
    Ok(BudgetPeriod {
        id: BudgetPeriodId::from_uuid(parse_uuid(row, "id")?),
        category_id: CategoryId::from_uuid(parse_uuid(row, "category_id")?),
        period_type: parse_enum::<PeriodType>(row, "period_type")?,
        amount: parse_decimal(row, "amount")?,
    })
}

fn row_to_budget_month(row: &AnyRow) -> Result<BudgetMonth, sqlx::Error> {
    Ok(BudgetMonth {
        id: BudgetMonthId::from_uuid(parse_uuid(row, "id")?),
        start_date: parse_date(row, "start_date")?,
        end_date: parse_date_opt(row, "end_date")?,
        salary_transactions_detected: row.try_get("salary_transactions_detected")?,
    })
}

fn row_to_project(row: &AnyRow) -> Result<Project, sqlx::Error> {
    Ok(Project {
        id: ProjectId::from_uuid(parse_uuid(row, "id")?),
        name: row.try_get("name")?,
        category_id: CategoryId::from_uuid(parse_uuid(row, "category_id")?),
        start_date: parse_date(row, "start_date")?,
        end_date: parse_date_opt(row, "end_date")?,
        budget_amount: parse_decimal_opt(row, "budget_amount")?,
    })
}

// ---------------------------------------------------------------------------
// Db wrapper
// ---------------------------------------------------------------------------

/// Database handle wrapping the connection pool.
///
/// All query functions are methods on this struct so that the pool type is
/// private. Callers never depend on `AnyPool` directly.
#[derive(Clone)]
pub struct Db(AnyPool);

impl Db {
    /// Open a connection pool to the database at `url`.
    ///
    /// Installs the default sqlx `Any` drivers (`SQLite`, Postgres) before
    /// connecting. The URL scheme determines which backend is used.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the connection fails.
    pub async fn connect(url: &str) -> Result<Self, sqlx::Error> {
        sqlx::any::install_default_drivers();
        Ok(Self(AnyPool::connect(url).await?))
    }

    /// Expose the inner pool for callers that need direct pool access
    /// (e.g. running raw queries against the apalis `Jobs` table).
    #[must_use]
    pub fn pool(&self) -> &AnyPool {
        &self.0
    }

    /// Run PRAGMAs (`SQLite` only) and domain migrations.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if any PRAGMA or migration fails.
    pub async fn run_migrations(&self, url: &str) -> Result<(), sqlx::Error> {
        if url.starts_with("sqlite:") {
            sqlx::query("PRAGMA journal_mode = 'WAL'")
                .execute(&self.0)
                .await?;
            sqlx::query("PRAGMA synchronous = NORMAL")
                .execute(&self.0)
                .await?;
            sqlx::query("PRAGMA cache_size = 64000")
                .execute(&self.0)
                .await?;
        }

        let mut migrator = sqlx::migrate!("../../migrations");
        migrator.set_ignore_missing(true);
        migrator.run(&self.0).await?;

        Ok(())
    }

    // ---------------------------------------------------------------------------
    // Accounts
    // ---------------------------------------------------------------------------

    /// Insert or update an account by primary key.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn upsert_account(&self, account: &Account) -> Result<(), sqlx::Error> {
        let pool = &self.0;
        sqlx::query(
            "INSERT INTO accounts (id, provider_account_id, name, institution, account_type, currency, connection_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT(id) DO UPDATE SET
                 provider_account_id = excluded.provider_account_id,
                 name = excluded.name,
                 institution = excluded.institution,
                 account_type = excluded.account_type,
                 currency = excluded.currency,
                 connection_id = excluded.connection_id",
        )
        .bind(account.id.to_string())
        .bind(&account.provider_account_id)
        .bind(&account.name)
        .bind(&account.institution)
        .bind(account.account_type.to_string())
        .bind(&account.currency)
        .bind(account.connection_id.map(|id| id.to_string()))
        .execute(pool)
        .await?;
        Ok(())
    }

    /// List all accounts.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn list_accounts(&self) -> Result<Vec<Account>, sqlx::Error> {
        let pool = &self.0;
        let rows = sqlx::query(
            "SELECT id, provider_account_id, name, institution, account_type, currency, connection_id FROM accounts",
        )
        .fetch_all(pool)
        .await?;
        rows.iter().map(row_to_account).collect()
    }

    /// Get a single account by its ID.
    ///
    /// Returns `None` if no account with the given ID exists.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn get_account(&self, id: AccountId) -> Result<Option<Account>, sqlx::Error> {
        let pool = &self.0;
        let row = sqlx::query(
            "SELECT id, provider_account_id, name, institution, account_type, currency, connection_id FROM accounts WHERE id = $1",
        )
        .bind(id.to_string())
        .fetch_optional(pool)
        .await?;
        row.as_ref().map(row_to_account).transpose()
    }

    /// Find an account by its provider account ID.
    ///
    /// Returns `None` if no account with the given provider ID exists.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn get_account_by_provider_id(
        &self,
        provider_account_id: &str,
    ) -> Result<Option<Account>, sqlx::Error> {
        let pool = &self.0;
        let row = sqlx::query(
            "SELECT id, provider_account_id, name, institution, account_type, currency, connection_id
             FROM accounts WHERE provider_account_id = $1",
        )
        .bind(provider_account_id)
        .fetch_optional(pool)
        .await?;
        row.as_ref().map(row_to_account).transpose()
    }

    // ---------------------------------------------------------------------------
    // Transactions
    // ---------------------------------------------------------------------------

    /// Insert or update a transaction using provider-level deduplication.
    ///
    /// When `provider_transaction_id` is `Some`, uses `ON CONFLICT(account_id,
    /// provider_transaction_id)` to update provider-sourced fields while
    /// preserving locally-enriched fields (category, budget month, project,
    /// correlation).
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn upsert_transaction(
        &self,
        txn: &Transaction,
        provider_transaction_id: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        let pool = &self.0;
        sqlx::query(
            "INSERT INTO transactions
                 (id, account_id, category_id, amount, original_amount, original_currency,
                  merchant_name, description, posted_date, budget_month_id, project_id,
                  correlation_id, correlation_type, provider_transaction_id, suggested_category)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
             ON CONFLICT(account_id, provider_transaction_id) DO UPDATE SET
                 amount = excluded.amount,
                 original_amount = excluded.original_amount,
                 original_currency = excluded.original_currency,
                 merchant_name = excluded.merchant_name,
                 description = excluded.description,
                 posted_date = excluded.posted_date",
        )
        .bind(txn.id.to_string())
        .bind(txn.account_id.to_string())
        .bind(txn.category_id.map(|id| id.to_string()))
        .bind(txn.amount.to_string())
        .bind(txn.original_amount.map(|d| d.to_string()))
        .bind(txn.original_currency.as_deref())
        .bind(&txn.merchant_name)
        .bind(&txn.description)
        .bind(txn.posted_date.to_string())
        .bind(txn.budget_month_id.map(|id| id.to_string()))
        .bind(txn.project_id.map(|id| id.to_string()))
        .bind(txn.correlation_id.map(|id| id.to_string()))
        .bind(txn.correlation_type.map(|ct| ct.to_string()))
        .bind(provider_transaction_id)
        .bind(txn.suggested_category.as_deref())
        .execute(pool)
        .await?;
        Ok(())
    }

    /// List all transactions.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn list_transactions(&self) -> Result<Vec<Transaction>, sqlx::Error> {
        let pool = &self.0;
        let rows = sqlx::query(
            "SELECT id, account_id, category_id, amount, original_amount, original_currency,
                    merchant_name, description, posted_date, budget_month_id, project_id,
                    correlation_id, correlation_type, suggested_category
             FROM transactions
             ORDER BY posted_date DESC, merchant_name ASC",
        )
        .fetch_all(pool)
        .await?;
        rows.iter().map(row_to_transaction).collect()
    }

    /// Get a single transaction by its ID.
    ///
    /// Returns `None` if no transaction exists with the given ID.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn get_transaction_by_id(
        &self,
        id: TransactionId,
    ) -> Result<Option<Transaction>, sqlx::Error> {
        let pool = &self.0;
        let row = sqlx::query(
            "SELECT id, account_id, category_id, amount, original_amount, original_currency,
                    merchant_name, description, posted_date, budget_month_id, project_id,
                    correlation_id, correlation_type, suggested_category
             FROM transactions
             WHERE id = $1",
        )
        .bind(id.to_string())
        .fetch_optional(pool)
        .await?;
        row.as_ref().map(row_to_transaction).transpose()
    }

    /// Get transactions that have not yet been categorized.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn get_uncategorized_transactions(&self) -> Result<Vec<Transaction>, sqlx::Error> {
        let pool = &self.0;
        let rows = sqlx::query(
            "SELECT id, account_id, category_id, amount, original_amount, original_currency,
                    merchant_name, description, posted_date, budget_month_id, project_id,
                    correlation_id, correlation_type, suggested_category
             FROM transactions
             WHERE category_id IS NULL",
        )
        .fetch_all(pool)
        .await?;
        rows.iter().map(row_to_transaction).collect()
    }

    /// Get categorized transactions that have no correlation assigned.
    ///
    /// Useful for the correlation engine to find candidate transfers and
    /// reimbursements.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn get_uncorrelated_transactions(&self) -> Result<Vec<Transaction>, sqlx::Error> {
        let pool = &self.0;
        let rows = sqlx::query(
            "SELECT id, account_id, category_id, amount, original_amount, original_currency,
                    merchant_name, description, posted_date, budget_month_id, project_id,
                    correlation_id, correlation_type, suggested_category
             FROM transactions
             WHERE correlation_id IS NULL AND correlation_type IS NULL AND category_id IS NOT NULL",
        )
        .fetch_all(pool)
        .await?;
        rows.iter().map(row_to_transaction).collect()
    }

    /// Get uncorrelated transactions with the exact opposite amount of the given value.
    ///
    /// Used by the per-transaction correlation handler to find candidate partners.
    /// Excludes the transaction identified by `exclude_id`.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn get_correlation_candidates(
        &self,
        opposite_amount: Decimal,
        exclude_id: TransactionId,
    ) -> Result<Vec<Transaction>, sqlx::Error> {
        let pool = &self.0;
        let rows = sqlx::query(
            "SELECT id, account_id, category_id, amount, original_amount, original_currency,
                    merchant_name, description, posted_date, budget_month_id, project_id,
                    correlation_id, correlation_type, suggested_category
             FROM transactions
             WHERE correlation_id IS NULL AND correlation_type IS NULL
               AND category_id IS NOT NULL
               AND amount = $1
               AND id != $2",
        )
        .bind(opposite_amount.to_string())
        .bind(exclude_id.to_string())
        .fetch_all(pool)
        .await?;
        rows.iter().map(row_to_transaction).collect()
    }

    /// Set the category of a transaction.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn update_transaction_category(
        &self,
        id: TransactionId,
        category_id: CategoryId,
    ) -> Result<(), sqlx::Error> {
        let pool = &self.0;
        sqlx::query("UPDATE transactions SET category_id = $1 WHERE id = $2")
            .bind(category_id.to_string())
            .bind(id.to_string())
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Set the correlation of a transaction (transfer or reimbursement link).
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn update_transaction_correlation(
        &self,
        id: TransactionId,
        correlation_id: TransactionId,
        correlation_type: CorrelationType,
    ) -> Result<(), sqlx::Error> {
        let pool = &self.0;
        sqlx::query(
            "UPDATE transactions SET correlation_id = $1, correlation_type = $2 WHERE id = $3",
        )
        .bind(correlation_id.to_string())
        .bind(correlation_type.to_string())
        .bind(id.to_string())
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Set the LLM-suggested category name on a transaction.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn update_transaction_suggested_category(
        &self,
        id: TransactionId,
        suggested_category: &str,
    ) -> Result<(), sqlx::Error> {
        let pool = &self.0;
        sqlx::query("UPDATE transactions SET suggested_category = $1 WHERE id = $2")
            .bind(suggested_category)
            .bind(id.to_string())
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Count uncategorized transactions grouped by their LLM-suggested category.
    ///
    /// Only includes transactions where `category_id IS NULL` and
    /// `suggested_category IS NOT NULL`. Results are ordered by count descending.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn get_suggestion_histogram(&self) -> Result<Vec<(String, i64)>, sqlx::Error> {
        let pool = &self.0;
        let rows = sqlx::query(
            "SELECT suggested_category, COUNT(*) as cnt
             FROM transactions
             WHERE category_id IS NULL AND suggested_category IS NOT NULL
             GROUP BY suggested_category
             ORDER BY cnt DESC",
        )
        .fetch_all(pool)
        .await?;
        rows.iter()
            .map(|row| {
                let name: String = row.try_get("suggested_category")?;
                let count: i64 = row.try_get("cnt")?;
                Ok((name, count))
            })
            .collect()
    }

    /// List all distinct category names currently in the categories table.
    ///
    /// Used to pass existing categories to the LLM so it maps to known names.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn list_category_names(&self) -> Result<Vec<String>, sqlx::Error> {
        let pool = &self.0;
        let rows = sqlx::query("SELECT name FROM categories ORDER BY name")
            .fetch_all(pool)
            .await?;
        rows.iter().map(|row| row.try_get("name")).collect()
    }

    /// Assign a transaction to a budget month.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn update_transaction_budget_month(
        &self,
        id: TransactionId,
        budget_month_id: BudgetMonthId,
    ) -> Result<(), sqlx::Error> {
        let pool = &self.0;
        sqlx::query("UPDATE transactions SET budget_month_id = $1 WHERE id = $2")
            .bind(budget_month_id.to_string())
            .bind(id.to_string())
            .execute(pool)
            .await?;
        Ok(())
    }

    /// List all transactions belonging to a specific account.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn list_transactions_by_account(
        &self,
        account_id: AccountId,
    ) -> Result<Vec<Transaction>, sqlx::Error> {
        let pool = &self.0;
        let rows = sqlx::query(
            "SELECT id, account_id, category_id, amount, original_amount, original_currency,
                    merchant_name, description, posted_date, budget_month_id, project_id,
                    correlation_id, correlation_type, suggested_category
             FROM transactions
             WHERE account_id = $1",
        )
        .bind(account_id.to_string())
        .fetch_all(pool)
        .await?;
        rows.iter().map(row_to_transaction).collect()
    }

    /// Get the most recent `posted_date` for a given account, or `None` if the
    /// account has no transactions yet.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn get_latest_transaction_date(
        &self,
        account_id: AccountId,
    ) -> Result<Option<NaiveDate>, sqlx::Error> {
        let pool = &self.0;
        let row = sqlx::query(
            "SELECT MAX(posted_date) as max_date FROM transactions WHERE account_id = $1",
        )
        .bind(account_id.to_string())
        .fetch_optional(pool)
        .await?;
        match row {
            Some(r) => {
                let s: Option<String> = r.try_get("max_date")?;
                Ok(s.and_then(|v| NaiveDate::parse_from_str(&v, "%Y-%m-%d").ok()))
            }
            None => Ok(None),
        }
    }

    // ---------------------------------------------------------------------------
    // Categories
    // ---------------------------------------------------------------------------

    /// Insert a new category.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails (e.g. duplicate primary key).
    pub async fn insert_category(&self, category: &Category) -> Result<(), sqlx::Error> {
        let pool = &self.0;
        sqlx::query("INSERT INTO categories (id, name, parent_id) VALUES ($1, $2, $3)")
            .bind(category.id.to_string())
            .bind(&category.name)
            .bind(category.parent_id.map(|id| id.to_string()))
            .execute(pool)
            .await?;
        Ok(())
    }

    /// List all categories.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn list_categories(&self) -> Result<Vec<Category>, sqlx::Error> {
        let pool = &self.0;
        let rows = sqlx::query("SELECT id, name, parent_id FROM categories")
            .fetch_all(pool)
            .await?;
        rows.iter().map(row_to_category).collect()
    }

    /// Get a single category by its ID.
    ///
    /// Returns `None` if no category with the given ID exists.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn get_category(&self, id: CategoryId) -> Result<Option<Category>, sqlx::Error> {
        let pool = &self.0;
        let row = sqlx::query("SELECT id, name, parent_id FROM categories WHERE id = $1")
            .bind(id.to_string())
            .fetch_optional(pool)
            .await?;
        row.as_ref().map(row_to_category).transpose()
    }

    /// Find a category by its exact name.
    ///
    /// Returns `None` if no category with the given name exists.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn get_category_by_name(&self, name: &str) -> Result<Option<Category>, sqlx::Error> {
        let pool = &self.0;
        let row = sqlx::query("SELECT id, name, parent_id FROM categories WHERE name = $1")
            .bind(name)
            .fetch_optional(pool)
            .await?;
        row.as_ref().map(row_to_category).transpose()
    }

    /// Delete a category by its ID.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails (e.g. foreign key violation).
    pub async fn delete_category(&self, id: CategoryId) -> Result<(), sqlx::Error> {
        let pool = &self.0;
        sqlx::query("DELETE FROM categories WHERE id = $1")
            .bind(id.to_string())
            .execute(pool)
            .await?;
        Ok(())
    }

    // ---------------------------------------------------------------------------
    // Rules
    // ---------------------------------------------------------------------------

    /// Insert a new rule.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn insert_rule(&self, rule: &Rule) -> Result<(), sqlx::Error> {
        let pool = &self.0;
        sqlx::query(
            "INSERT INTO rules (id, rule_type, match_field, match_pattern, target_category_id, target_correlation_type, priority)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(rule.id.to_string())
        .bind(rule.rule_type.to_string())
        .bind(rule.match_field.to_string())
        .bind(&rule.match_pattern)
        .bind(rule.target_category_id.map(|id| id.to_string()))
        .bind(rule.target_correlation_type.map(|ct| ct.to_string()))
        .bind(rule.priority)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// List all rules.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn list_rules(&self) -> Result<Vec<Rule>, sqlx::Error> {
        let pool = &self.0;
        let rows = sqlx::query(
            "SELECT id, rule_type, match_field, match_pattern, target_category_id, target_correlation_type, priority
             FROM rules",
        )
        .fetch_all(pool)
        .await?;
        rows.iter().map(row_to_rule).collect()
    }

    /// List rules filtered by type, ordered by priority descending.
    ///
    /// Higher-priority rules are returned first.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn list_rules_by_type(&self, rule_type: RuleType) -> Result<Vec<Rule>, sqlx::Error> {
        let pool = &self.0;
        let rows = sqlx::query(
            "SELECT id, rule_type, match_field, match_pattern, target_category_id, target_correlation_type, priority
             FROM rules
             WHERE rule_type = $1
             ORDER BY priority DESC",
        )
        .bind(rule_type.to_string())
        .fetch_all(pool)
        .await?;
        rows.iter().map(row_to_rule).collect()
    }

    /// Get a single rule by its ID.
    ///
    /// Returns `None` if no rule with the given ID exists.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn get_rule(&self, id: RuleId) -> Result<Option<Rule>, sqlx::Error> {
        let pool = &self.0;
        let row = sqlx::query(
            "SELECT id, rule_type, match_field, match_pattern, target_category_id, target_correlation_type, priority
             FROM rules WHERE id = $1",
        )
        .bind(id.to_string())
        .fetch_optional(pool)
        .await?;
        row.as_ref().map(row_to_rule).transpose()
    }

    /// Update all mutable fields of an existing rule.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn update_rule(&self, rule: &Rule) -> Result<(), sqlx::Error> {
        let pool = &self.0;
        sqlx::query(
            "UPDATE rules SET rule_type = $1, match_field = $2, match_pattern = $3,
                    target_category_id = $4, target_correlation_type = $5, priority = $6
             WHERE id = $7",
        )
        .bind(rule.rule_type.to_string())
        .bind(rule.match_field.to_string())
        .bind(&rule.match_pattern)
        .bind(rule.target_category_id.map(|id| id.to_string()))
        .bind(rule.target_correlation_type.map(|ct| ct.to_string()))
        .bind(rule.priority)
        .bind(rule.id.to_string())
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Delete a rule by its ID.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn delete_rule(&self, id: RuleId) -> Result<(), sqlx::Error> {
        let pool = &self.0;
        sqlx::query("DELETE FROM rules WHERE id = $1")
            .bind(id.to_string())
            .execute(pool)
            .await?;
        Ok(())
    }

    // ---------------------------------------------------------------------------
    // Budget Periods
    // ---------------------------------------------------------------------------

    /// Insert a new budget period.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn insert_budget_period(&self, bp: &BudgetPeriod) -> Result<(), sqlx::Error> {
        let pool = &self.0;
        sqlx::query(
            "INSERT INTO budget_periods (id, category_id, period_type, amount)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(bp.id.to_string())
        .bind(bp.category_id.to_string())
        .bind(bp.period_type.to_string())
        .bind(bp.amount.to_string())
        .execute(pool)
        .await?;
        Ok(())
    }

    /// List all budget periods.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn list_budget_periods(&self) -> Result<Vec<BudgetPeriod>, sqlx::Error> {
        let pool = &self.0;
        let rows = sqlx::query("SELECT id, category_id, period_type, amount FROM budget_periods")
            .fetch_all(pool)
            .await?;
        rows.iter().map(row_to_budget_period).collect()
    }

    /// Get a single budget period by its ID.
    ///
    /// Returns `None` if no budget period with the given ID exists.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn get_budget_period(
        &self,
        id: BudgetPeriodId,
    ) -> Result<Option<BudgetPeriod>, sqlx::Error> {
        let pool = &self.0;
        let row = sqlx::query(
            "SELECT id, category_id, period_type, amount FROM budget_periods WHERE id = $1",
        )
        .bind(id.to_string())
        .fetch_optional(pool)
        .await?;
        row.as_ref().map(row_to_budget_period).transpose()
    }

    /// Update all mutable fields of an existing budget period.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn update_budget_period(&self, bp: &BudgetPeriod) -> Result<(), sqlx::Error> {
        let pool = &self.0;
        sqlx::query(
            "UPDATE budget_periods SET category_id = $1, period_type = $2, amount = $3 WHERE id = $4",
        )
        .bind(bp.category_id.to_string())
        .bind(bp.period_type.to_string())
        .bind(bp.amount.to_string())
        .bind(bp.id.to_string())
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Delete a budget period by its ID.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn delete_budget_period(&self, id: BudgetPeriodId) -> Result<(), sqlx::Error> {
        let pool = &self.0;
        sqlx::query("DELETE FROM budget_periods WHERE id = $1")
            .bind(id.to_string())
            .execute(pool)
            .await?;
        Ok(())
    }

    // ---------------------------------------------------------------------------
    // Budget Months
    // ---------------------------------------------------------------------------

    /// Replace all budget months atomically.
    ///
    /// Deletes every existing budget month and inserts the provided set within
    /// a single transaction.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if any query within the transaction fails.
    pub async fn replace_budget_months(&self, months: &[BudgetMonth]) -> Result<(), sqlx::Error> {
        let mut tx = self.0.begin().await?;

        sqlx::query("DELETE FROM budget_months")
            .execute(&mut *tx)
            .await?;

        for month in months {
            sqlx::query(
                "INSERT INTO budget_months (id, start_date, end_date, salary_transactions_detected)
                 VALUES ($1, $2, $3, $4)",
            )
            .bind(month.id.to_string())
            .bind(month.start_date.to_string())
            .bind(month.end_date.map(|d| d.to_string()))
            .bind(month.salary_transactions_detected)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    /// List all budget months.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn list_budget_months(&self) -> Result<Vec<BudgetMonth>, sqlx::Error> {
        let pool = &self.0;
        let rows = sqlx::query(
            "SELECT id, start_date, end_date, salary_transactions_detected FROM budget_months",
        )
        .fetch_all(pool)
        .await?;
        rows.iter().map(row_to_budget_month).collect()
    }

    // ---------------------------------------------------------------------------
    // Projects
    // ---------------------------------------------------------------------------

    /// Insert a new project.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn insert_project(&self, project: &Project) -> Result<(), sqlx::Error> {
        let pool = &self.0;
        sqlx::query(
            "INSERT INTO projects (id, name, category_id, start_date, end_date, budget_amount)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(project.id.to_string())
        .bind(&project.name)
        .bind(project.category_id.to_string())
        .bind(project.start_date.to_string())
        .bind(project.end_date.map(|d| d.to_string()))
        .bind(project.budget_amount.map(|d| d.to_string()))
        .execute(pool)
        .await?;
        Ok(())
    }

    /// List all projects.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn list_projects(&self) -> Result<Vec<Project>, sqlx::Error> {
        let pool = &self.0;
        let rows = sqlx::query(
            "SELECT id, name, category_id, start_date, end_date, budget_amount FROM projects",
        )
        .fetch_all(pool)
        .await?;
        rows.iter().map(row_to_project).collect()
    }

    /// Get a single project by its ID.
    ///
    /// Returns `None` if no project with the given ID exists.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn get_project(&self, id: ProjectId) -> Result<Option<Project>, sqlx::Error> {
        let pool = &self.0;
        let row = sqlx::query(
            "SELECT id, name, category_id, start_date, end_date, budget_amount FROM projects WHERE id = $1",
        )
        .bind(id.to_string())
        .fetch_optional(pool)
        .await?;
        row.as_ref().map(row_to_project).transpose()
    }

    /// Update all mutable fields of an existing project.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn update_project(&self, project: &Project) -> Result<(), sqlx::Error> {
        let pool = &self.0;
        sqlx::query(
            "UPDATE projects SET name = $1, category_id = $2, start_date = $3, end_date = $4, budget_amount = $5
             WHERE id = $6",
        )
        .bind(&project.name)
        .bind(project.category_id.to_string())
        .bind(project.start_date.to_string())
        .bind(project.end_date.map(|d| d.to_string()))
        .bind(project.budget_amount.map(|d| d.to_string()))
        .bind(project.id.to_string())
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Delete a project by its ID.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn delete_project(&self, id: ProjectId) -> Result<(), sqlx::Error> {
        let pool = &self.0;
        sqlx::query("DELETE FROM projects WHERE id = $1")
            .bind(id.to_string())
            .execute(pool)
            .await?;
        Ok(())
    }

    // ---------------------------------------------------------------------------
    // Connections
    // ---------------------------------------------------------------------------

    /// Insert a new connection.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn insert_connection(&self, connection: &Connection) -> Result<(), sqlx::Error> {
        let pool = &self.0;
        sqlx::query(
            "INSERT INTO connections (id, provider, provider_session_id, institution_name, valid_until, status)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(connection.id.to_string())
        .bind(&connection.provider)
        .bind(&connection.provider_session_id)
        .bind(&connection.institution_name)
        .bind(&connection.valid_until)
        .bind(connection.status.to_string())
        .execute(pool)
        .await?;
        Ok(())
    }

    /// List all connections.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn list_connections(&self) -> Result<Vec<Connection>, sqlx::Error> {
        let pool = &self.0;
        let rows = sqlx::query(
            "SELECT id, provider, provider_session_id, institution_name, valid_until, status
             FROM connections ORDER BY created_at DESC",
        )
        .fetch_all(pool)
        .await?;
        rows.iter().map(row_to_connection).collect()
    }

    /// Get a single connection by its ID.
    ///
    /// Returns `None` if no connection with the given ID exists.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn get_connection(
        &self,
        id: ConnectionId,
    ) -> Result<Option<Connection>, sqlx::Error> {
        let pool = &self.0;
        let row = sqlx::query(
            "SELECT id, provider, provider_session_id, institution_name, valid_until, status
             FROM connections WHERE id = $1",
        )
        .bind(id.to_string())
        .fetch_optional(pool)
        .await?;
        row.as_ref().map(row_to_connection).transpose()
    }

    /// Update the status of an existing connection.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn update_connection_status(
        &self,
        id: ConnectionId,
        status: ConnectionStatus,
    ) -> Result<(), sqlx::Error> {
        let pool = &self.0;
        sqlx::query(
            "UPDATE connections SET status = $1, updated_at = CURRENT_TIMESTAMP WHERE id = $2",
        )
        .bind(status.to_string())
        .bind(id.to_string())
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Delete a connection by its ID.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn delete_connection(&self, id: ConnectionId) -> Result<(), sqlx::Error> {
        let pool = &self.0;
        sqlx::query("DELETE FROM connections WHERE id = $1")
            .bind(id.to_string())
            .execute(pool)
            .await?;
        Ok(())
    }

    // ---------------------------------------------------------------------------
    // State Tokens
    // ---------------------------------------------------------------------------

    /// Insert a new state token for the OAuth callback flow.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn insert_state_token(
        &self,
        token: &str,
        user_data: &str,
        expires_at: &str,
    ) -> Result<(), sqlx::Error> {
        let pool = &self.0;
        sqlx::query("INSERT INTO state_tokens (token, user_data, expires_at) VALUES ($1, $2, $3)")
            .bind(token)
            .bind(user_data)
            .bind(expires_at)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Atomically consume a state token, returning its user data if valid.
    ///
    /// Uses `UPDATE ... RETURNING` to mark the token as used in a single
    /// statement, preventing replay attacks. Returns `None` if the token
    /// does not exist, has already been used, or has expired.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn consume_state_token(&self, token: &str) -> Result<Option<String>, sqlx::Error> {
        let pool = &self.0;
        let row = sqlx::query(
            "UPDATE state_tokens SET used = 1
             WHERE token = $1 AND used = 0 AND expires_at > CURRENT_TIMESTAMP
             RETURNING user_data",
        )
        .bind(token)
        .fetch_optional(pool)
        .await?;
        match row {
            Some(r) => Ok(Some(r.try_get("user_data")?)),
            None => Ok(None),
        }
    }

    /// Delete expired state tokens.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn prune_expired_state_tokens(&self) -> Result<u64, sqlx::Error> {
        let pool = &self.0;
        let result = sqlx::query("DELETE FROM state_tokens WHERE expires_at <= CURRENT_TIMESTAMP")
            .execute(pool)
            .await?;
        Ok(result.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use chrono::NaiveDate;
    use rust_decimal_macros::dec;

    use crate::models::{
        Account, AccountId, AccountType, BudgetMonth, BudgetMonthId, BudgetPeriod, BudgetPeriodId,
        Category, CategoryId, CorrelationType, MatchField, PeriodType, Project, ProjectId, Rule,
        RuleId, RuleType, Transaction, TransactionId,
    };

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    async fn setup_db() -> Db {
        let url = format!(
            "sqlite:file:coretest_{}?mode=memory&cache=shared",
            uuid::Uuid::new_v4().simple()
        );
        let db = Db::connect(&url).await.unwrap();
        db.run_migrations(&url).await.unwrap();
        db
    }

    fn make_account() -> Account {
        Account {
            id: AccountId::new(),
            provider_account_id: "prov-acct-001".into(),
            name: "My Checking".into(),
            institution: "Test Bank".into(),
            account_type: AccountType::Checking,
            currency: "EUR".into(),
            connection_id: None,
        }
    }

    fn make_category(name: &str) -> Category {
        Category {
            id: CategoryId::new(),
            name: name.into(),
            parent_id: None,
        }
    }

    fn make_transaction(account_id: AccountId) -> Transaction {
        Transaction {
            id: TransactionId::new(),
            account_id,
            category_id: None,
            amount: dec!(-42.50),
            original_amount: None,
            original_currency: None,
            merchant_name: "Coffee Shop".into(),
            description: "Morning coffee".into(),
            posted_date: NaiveDate::from_ymd_opt(2025, 3, 15).unwrap(),
            budget_month_id: None,
            project_id: None,
            correlation_id: None,
            correlation_type: None,
            suggested_category: None,
        }
    }

    fn make_rule(rule_type: RuleType, priority: i32) -> Rule {
        Rule {
            id: RuleId::new(),
            rule_type,
            match_field: MatchField::Merchant,
            match_pattern: "Coffee.*".into(),
            target_category_id: None,
            target_correlation_type: None,
            priority,
        }
    }

    fn make_budget_period(category_id: CategoryId) -> BudgetPeriod {
        BudgetPeriod {
            id: BudgetPeriodId::new(),
            category_id,
            period_type: PeriodType::Monthly,
            amount: dec!(500.00),
        }
    }

    fn make_budget_month(start: NaiveDate, end: Option<NaiveDate>) -> BudgetMonth {
        BudgetMonth {
            id: BudgetMonthId::new(),
            start_date: start,
            end_date: end,
            salary_transactions_detected: 0,
        }
    }

    fn make_project(category_id: CategoryId) -> Project {
        Project {
            id: ProjectId::new(),
            name: "Kitchen Renovation".into(),
            category_id,
            start_date: NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
            end_date: None,
            budget_amount: Some(dec!(10000.00)),
        }
    }

    // -----------------------------------------------------------------------
    // Account tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_upsert_account_roundtrip() {
        let db = setup_db().await;
        let acct = make_account();

        db.upsert_account(&acct).await.unwrap();
        let fetched = db.get_account(acct.id).await.unwrap().unwrap();

        assert_eq!(fetched.id, acct.id);
        assert_eq!(fetched.provider_account_id, acct.provider_account_id);
        assert_eq!(fetched.name, acct.name);
        assert_eq!(fetched.institution, acct.institution);
        assert_eq!(fetched.account_type, acct.account_type);
        assert_eq!(fetched.currency, acct.currency);
    }

    #[tokio::test]
    async fn test_upsert_account_replaces_existing() {
        let db = setup_db().await;
        let mut acct = make_account();

        db.upsert_account(&acct).await.unwrap();

        acct.name = "Updated Checking".into();
        acct.institution = "New Bank".into();
        acct.account_type = AccountType::Savings;

        db.upsert_account(&acct).await.unwrap();

        let all = db.list_accounts().await.unwrap();
        assert_eq!(all.len(), 1);

        let fetched = &all[0];
        assert_eq!(fetched.name, "Updated Checking");
        assert_eq!(fetched.institution, "New Bank");
        assert_eq!(fetched.account_type, AccountType::Savings);
    }

    #[tokio::test]
    async fn test_list_accounts_returns_all() {
        let db = setup_db().await;

        let acct1 = make_account();
        let mut acct2 = make_account();
        acct2.id = AccountId::new();
        acct2.name = "Savings Account".into();
        acct2.account_type = AccountType::Savings;

        db.upsert_account(&acct1).await.unwrap();
        db.upsert_account(&acct2).await.unwrap();

        let all = db.list_accounts().await.unwrap();
        assert_eq!(all.len(), 2);

        let ids: Vec<_> = all.iter().map(|a| a.id).collect();
        assert!(ids.contains(&acct1.id));
        assert!(ids.contains(&acct2.id));
    }

    #[tokio::test]
    async fn test_get_account_returns_none_for_nonexistent() {
        let db = setup_db().await;
        let result = db.get_account(AccountId::new()).await.unwrap();
        assert!(result.is_none());
    }

    // -----------------------------------------------------------------------
    // Transaction tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_upsert_transaction_roundtrip() {
        let db = setup_db().await;
        let acct = make_account();
        db.upsert_account(&acct).await.unwrap();

        let txn = make_transaction(acct.id);
        db.upsert_transaction(&txn, None).await.unwrap();

        let all = db.list_transactions().await.unwrap();
        assert_eq!(all.len(), 1);

        let fetched = &all[0];
        assert_eq!(fetched.id, txn.id);
        assert_eq!(fetched.account_id, txn.account_id);
        assert_eq!(fetched.amount, dec!(-42.50));
        assert_eq!(fetched.merchant_name, "Coffee Shop");
        assert_eq!(fetched.description, "Morning coffee");
        assert_eq!(
            fetched.posted_date,
            NaiveDate::from_ymd_opt(2025, 3, 15).unwrap()
        );
        assert!(fetched.category_id.is_none());
        assert!(fetched.budget_month_id.is_none());
        assert!(fetched.project_id.is_none());
        assert!(fetched.correlation_id.is_none());
        assert!(fetched.correlation_type.is_none());
    }

    #[tokio::test]
    async fn test_upsert_transaction_dedup_by_provider_id() {
        let db = setup_db().await;
        let acct = make_account();
        db.upsert_account(&acct).await.unwrap();

        let txn1 = make_transaction(acct.id);
        db.upsert_transaction(&txn1, Some("PROV-TXN-001"))
            .await
            .unwrap();

        // Second insert with same provider_transaction_id but different
        // domain ID should trigger ON CONFLICT update, not create a duplicate.
        let mut txn2 = make_transaction(acct.id);
        txn2.merchant_name = "Updated Coffee Shop".into();
        txn2.amount = dec!(-45.00);
        db.upsert_transaction(&txn2, Some("PROV-TXN-001"))
            .await
            .unwrap();

        let all = db.list_transactions().await.unwrap();
        assert_eq!(all.len(), 1, "dedup should prevent duplicate rows");

        // The provider-sourced fields should reflect the second insert.
        let fetched = &all[0];
        assert_eq!(fetched.merchant_name, "Updated Coffee Shop");
        assert_eq!(fetched.amount, dec!(-45.00));
    }

    #[tokio::test]
    async fn test_upsert_transaction_dedup_preserves_local_fields() {
        let db = setup_db().await;
        let acct = make_account();
        db.upsert_account(&acct).await.unwrap();

        let cat = make_category("Food & Drink");
        db.insert_category(&cat).await.unwrap();

        let bm = make_budget_month(
            NaiveDate::from_ymd_opt(2025, 3, 1).unwrap(),
            Some(NaiveDate::from_ymd_opt(2025, 3, 31).unwrap()),
        );
        db.replace_budget_months(std::slice::from_ref(&bm))
            .await
            .unwrap();

        // Insert original transaction with a provider ID.
        let txn = make_transaction(acct.id);
        db.upsert_transaction(&txn, Some("PROV-TXN-002"))
            .await
            .unwrap();

        // Locally enrich the transaction with category and budget month.
        db.update_transaction_category(txn.id, cat.id)
            .await
            .unwrap();
        db.update_transaction_budget_month(txn.id, bm.id)
            .await
            .unwrap();

        // Now simulate a provider re-sync: upsert again with the same
        // provider_transaction_id but updated provider fields.
        let mut txn_updated = make_transaction(acct.id);
        txn_updated.merchant_name = "Coffee Shop (corrected)".into();
        txn_updated.amount = dec!(-43.00);
        db.upsert_transaction(&txn_updated, Some("PROV-TXN-002"))
            .await
            .unwrap();

        let all = db.list_transactions().await.unwrap();
        assert_eq!(all.len(), 1);

        let fetched = &all[0];
        // Provider-sourced fields should be updated.
        assert_eq!(fetched.merchant_name, "Coffee Shop (corrected)");
        assert_eq!(fetched.amount, dec!(-43.00));

        // Locally-enriched fields should be preserved.
        assert_eq!(fetched.category_id, Some(cat.id));
        assert_eq!(fetched.budget_month_id, Some(bm.id));
    }

    #[tokio::test]
    async fn test_get_uncategorized_transactions() {
        let db = setup_db().await;
        let acct = make_account();
        db.upsert_account(&acct).await.unwrap();

        let cat = make_category("Groceries");
        db.insert_category(&cat).await.unwrap();

        // Insert two transactions: one uncategorized, one categorized.
        let txn_uncat = make_transaction(acct.id);
        db.upsert_transaction(&txn_uncat, None).await.unwrap();

        let mut txn_cat = make_transaction(acct.id);
        txn_cat.category_id = Some(cat.id);
        db.upsert_transaction(&txn_cat, None).await.unwrap();

        let uncat = db.get_uncategorized_transactions().await.unwrap();
        assert_eq!(uncat.len(), 1);
        assert_eq!(uncat[0].id, txn_uncat.id);
    }

    #[tokio::test]
    async fn test_get_uncorrelated_transactions() {
        let db = setup_db().await;
        let acct = make_account();
        db.upsert_account(&acct).await.unwrap();

        let cat = make_category("Transfers");
        db.insert_category(&cat).await.unwrap();

        // Uncategorized transaction: should NOT appear (not categorized).
        let txn_uncat = make_transaction(acct.id);
        db.upsert_transaction(&txn_uncat, None).await.unwrap();

        // Categorized but uncorrelated: SHOULD appear.
        let mut txn_uncorr = make_transaction(acct.id);
        txn_uncorr.category_id = Some(cat.id);
        db.upsert_transaction(&txn_uncorr, None).await.unwrap();

        // Categorized AND correlated: should NOT appear.
        let mut txn_corr = make_transaction(acct.id);
        txn_corr.category_id = Some(cat.id);
        txn_corr.correlation_id = Some(txn_uncorr.id);
        txn_corr.correlation_type = Some(CorrelationType::Transfer);
        db.upsert_transaction(&txn_corr, None).await.unwrap();

        let uncorr = db.get_uncorrelated_transactions().await.unwrap();
        assert_eq!(uncorr.len(), 1);
        assert_eq!(uncorr[0].id, txn_uncorr.id);
    }

    #[tokio::test]
    async fn test_update_transaction_category() {
        let db = setup_db().await;
        let acct = make_account();
        db.upsert_account(&acct).await.unwrap();

        let cat = make_category("Dining");
        db.insert_category(&cat).await.unwrap();

        let txn = make_transaction(acct.id);
        db.upsert_transaction(&txn, None).await.unwrap();

        db.update_transaction_category(txn.id, cat.id)
            .await
            .unwrap();

        let all = db.list_transactions().await.unwrap();
        assert_eq!(all[0].category_id, Some(cat.id));
    }

    #[tokio::test]
    async fn test_update_transaction_correlation() {
        let db = setup_db().await;
        let acct = make_account();
        db.upsert_account(&acct).await.unwrap();

        let txn_a = make_transaction(acct.id);
        let txn_b = make_transaction(acct.id);
        db.upsert_transaction(&txn_a, None).await.unwrap();
        db.upsert_transaction(&txn_b, None).await.unwrap();

        db.update_transaction_correlation(txn_a.id, txn_b.id, CorrelationType::Transfer)
            .await
            .unwrap();

        let fetched = db.list_transactions().await.unwrap();
        let a = fetched.iter().find(|t| t.id == txn_a.id).unwrap();
        assert_eq!(a.correlation_id, Some(txn_b.id));
        assert_eq!(a.correlation_type, Some(CorrelationType::Transfer));
    }

    #[tokio::test]
    async fn test_update_transaction_budget_month() {
        let db = setup_db().await;
        let acct = make_account();
        db.upsert_account(&acct).await.unwrap();

        let bm = make_budget_month(
            NaiveDate::from_ymd_opt(2025, 3, 1).unwrap(),
            Some(NaiveDate::from_ymd_opt(2025, 3, 31).unwrap()),
        );
        db.replace_budget_months(std::slice::from_ref(&bm))
            .await
            .unwrap();

        let txn = make_transaction(acct.id);
        db.upsert_transaction(&txn, None).await.unwrap();

        db.update_transaction_budget_month(txn.id, bm.id)
            .await
            .unwrap();

        let all = db.list_transactions().await.unwrap();
        assert_eq!(all[0].budget_month_id, Some(bm.id));
    }

    #[tokio::test]
    async fn test_list_transactions_by_account() {
        let db = setup_db().await;

        let acct1 = make_account();
        let mut acct2 = make_account();
        acct2.id = AccountId::new();
        acct2.provider_account_id = "prov-acct-002".into();
        db.upsert_account(&acct1).await.unwrap();
        db.upsert_account(&acct2).await.unwrap();

        let txn1 = make_transaction(acct1.id);
        let txn2 = make_transaction(acct1.id);
        let txn3 = make_transaction(acct2.id);
        db.upsert_transaction(&txn1, None).await.unwrap();
        db.upsert_transaction(&txn2, None).await.unwrap();
        db.upsert_transaction(&txn3, None).await.unwrap();

        let acct1_txns = db.list_transactions_by_account(acct1.id).await.unwrap();
        assert_eq!(acct1_txns.len(), 2);
        assert!(acct1_txns.iter().all(|t| t.account_id == acct1.id));

        let acct2_txns = db.list_transactions_by_account(acct2.id).await.unwrap();
        assert_eq!(acct2_txns.len(), 1);
        assert_eq!(acct2_txns[0].id, txn3.id);
    }

    // -----------------------------------------------------------------------
    // Category tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_insert_category_and_list_roundtrip() {
        let db = setup_db().await;

        let cat1 = make_category("Groceries");
        let cat2 = make_category("Transport");

        db.insert_category(&cat1).await.unwrap();
        db.insert_category(&cat2).await.unwrap();

        let all = db.list_categories().await.unwrap();
        assert_eq!(all.len(), 2);

        let names: Vec<_> = all.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"Groceries"));
        assert!(names.contains(&"Transport"));
    }

    #[tokio::test]
    async fn test_get_category_by_id() {
        let db = setup_db().await;
        let cat = make_category("Entertainment");
        db.insert_category(&cat).await.unwrap();

        let fetched = db.get_category(cat.id).await.unwrap().unwrap();
        assert_eq!(fetched.id, cat.id);
        assert_eq!(fetched.name, "Entertainment");
        assert!(fetched.parent_id.is_none());
    }

    #[tokio::test]
    async fn test_get_category_by_name() {
        let db = setup_db().await;
        let cat = make_category("Subscriptions");
        db.insert_category(&cat).await.unwrap();

        let fetched = db
            .get_category_by_name("Subscriptions")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.id, cat.id);
        assert_eq!(fetched.name, "Subscriptions");
    }

    #[tokio::test]
    async fn test_get_category_by_name_returns_none_for_nonexistent() {
        let db = setup_db().await;
        let result = db.get_category_by_name("Nonexistent").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_delete_category() {
        let db = setup_db().await;
        let cat = make_category("ToDelete");
        db.insert_category(&cat).await.unwrap();

        db.delete_category(cat.id).await.unwrap();

        let result = db.get_category(cat.id).await.unwrap();
        assert!(result.is_none());
    }

    // -----------------------------------------------------------------------
    // Rule tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_insert_rule_and_list_roundtrip() {
        let db = setup_db().await;

        let cat = make_category("Food");
        db.insert_category(&cat).await.unwrap();

        let mut rule = make_rule(RuleType::Categorization, 10);
        rule.target_category_id = Some(cat.id);

        db.insert_rule(&rule).await.unwrap();

        let all = db.list_rules().await.unwrap();
        assert_eq!(all.len(), 1);

        let fetched = &all[0];
        assert_eq!(fetched.id, rule.id);
        assert_eq!(fetched.rule_type, RuleType::Categorization);
        assert_eq!(fetched.match_field, MatchField::Merchant);
        assert_eq!(fetched.match_pattern, "Coffee.*");
        assert_eq!(fetched.target_category_id, Some(cat.id));
        assert!(fetched.target_correlation_type.is_none());
        assert_eq!(fetched.priority, 10);
    }

    #[tokio::test]
    async fn test_list_rules_by_type_filters_and_orders_by_priority_desc() {
        let db = setup_db().await;

        let cat_rule_low = make_rule(RuleType::Categorization, 1);
        let cat_rule_high = make_rule(RuleType::Categorization, 100);
        let mut corr_rule = make_rule(RuleType::Correlation, 50);
        corr_rule.target_correlation_type = Some(CorrelationType::Transfer);

        db.insert_rule(&cat_rule_low).await.unwrap();
        db.insert_rule(&cat_rule_high).await.unwrap();
        db.insert_rule(&corr_rule).await.unwrap();

        let cat_rules = db
            .list_rules_by_type(RuleType::Categorization)
            .await
            .unwrap();
        assert_eq!(cat_rules.len(), 2);
        // Highest priority first.
        assert_eq!(cat_rules[0].priority, 100);
        assert_eq!(cat_rules[1].priority, 1);

        let corr_rules = db.list_rules_by_type(RuleType::Correlation).await.unwrap();
        assert_eq!(corr_rules.len(), 1);
        assert_eq!(
            corr_rules[0].target_correlation_type,
            Some(CorrelationType::Transfer)
        );
    }

    #[tokio::test]
    async fn test_get_rule_by_id() {
        let db = setup_db().await;
        let rule = make_rule(RuleType::Categorization, 5);
        db.insert_rule(&rule).await.unwrap();

        let fetched = db.get_rule(rule.id).await.unwrap().unwrap();
        assert_eq!(fetched.id, rule.id);
        assert_eq!(fetched.priority, 5);
    }

    #[tokio::test]
    async fn test_update_rule_changes_fields() {
        let db = setup_db().await;

        let cat = make_category("Travel");
        db.insert_category(&cat).await.unwrap();

        let mut rule = make_rule(RuleType::Categorization, 5);
        db.insert_rule(&rule).await.unwrap();

        rule.match_field = MatchField::Description;
        rule.match_pattern = "Hotel.*".into();
        rule.target_category_id = Some(cat.id);
        rule.priority = 20;

        db.update_rule(&rule).await.unwrap();

        let fetched = db.get_rule(rule.id).await.unwrap().unwrap();
        assert_eq!(fetched.match_field, MatchField::Description);
        assert_eq!(fetched.match_pattern, "Hotel.*");
        assert_eq!(fetched.target_category_id, Some(cat.id));
        assert_eq!(fetched.priority, 20);
    }

    #[tokio::test]
    async fn test_delete_rule() {
        let db = setup_db().await;
        let rule = make_rule(RuleType::Categorization, 1);
        db.insert_rule(&rule).await.unwrap();

        db.delete_rule(rule.id).await.unwrap();

        let result = db.get_rule(rule.id).await.unwrap();
        assert!(result.is_none());
    }

    // -----------------------------------------------------------------------
    // Budget Period tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_insert_budget_period_and_list_roundtrip() {
        let db = setup_db().await;

        let cat = make_category("Utilities");
        db.insert_category(&cat).await.unwrap();

        let bp = make_budget_period(cat.id);
        db.insert_budget_period(&bp).await.unwrap();

        let all = db.list_budget_periods().await.unwrap();
        assert_eq!(all.len(), 1);

        let fetched = &all[0];
        assert_eq!(fetched.id, bp.id);
        assert_eq!(fetched.category_id, cat.id);
        assert_eq!(fetched.period_type, PeriodType::Monthly);
        assert_eq!(fetched.amount, dec!(500.00));
    }

    #[tokio::test]
    async fn test_get_budget_period_by_id() {
        let db = setup_db().await;

        let cat = make_category("Rent");
        db.insert_category(&cat).await.unwrap();

        let bp = make_budget_period(cat.id);
        db.insert_budget_period(&bp).await.unwrap();

        let fetched = db.get_budget_period(bp.id).await.unwrap().unwrap();
        assert_eq!(fetched.id, bp.id);
        assert_eq!(fetched.amount, dec!(500.00));
    }

    #[tokio::test]
    async fn test_update_budget_period_changes_fields() {
        let db = setup_db().await;

        let cat1 = make_category("Groceries");
        let cat2 = make_category("Dining");
        db.insert_category(&cat1).await.unwrap();
        db.insert_category(&cat2).await.unwrap();

        let mut bp = make_budget_period(cat1.id);
        db.insert_budget_period(&bp).await.unwrap();

        bp.category_id = cat2.id;
        bp.period_type = PeriodType::Annual;
        bp.amount = dec!(6000.00);

        db.update_budget_period(&bp).await.unwrap();

        let fetched = db.get_budget_period(bp.id).await.unwrap().unwrap();
        assert_eq!(fetched.category_id, cat2.id);
        assert_eq!(fetched.period_type, PeriodType::Annual);
        assert_eq!(fetched.amount, dec!(6000.00));
    }

    #[tokio::test]
    async fn test_delete_budget_period() {
        let db = setup_db().await;

        let cat = make_category("Insurance");
        db.insert_category(&cat).await.unwrap();

        let bp = make_budget_period(cat.id);
        db.insert_budget_period(&bp).await.unwrap();

        db.delete_budget_period(bp.id).await.unwrap();

        let result = db.get_budget_period(bp.id).await.unwrap();
        assert!(result.is_none());
    }

    // -----------------------------------------------------------------------
    // Budget Month tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_replace_budget_months_inserts_fresh_set() {
        let db = setup_db().await;

        let months = vec![
            make_budget_month(
                NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
                Some(NaiveDate::from_ymd_opt(2025, 1, 31).unwrap()),
            ),
            make_budget_month(
                NaiveDate::from_ymd_opt(2025, 2, 1).unwrap(),
                Some(NaiveDate::from_ymd_opt(2025, 2, 28).unwrap()),
            ),
        ];

        db.replace_budget_months(&months).await.unwrap();

        let all = db.list_budget_months().await.unwrap();
        assert_eq!(all.len(), 2);

        let ids: Vec<_> = all.iter().map(|m| m.id).collect();
        assert!(ids.contains(&months[0].id));
        assert!(ids.contains(&months[1].id));
    }

    #[tokio::test]
    async fn test_replace_budget_months_replaces_all() {
        let db = setup_db().await;

        // Insert initial set.
        let old_months = vec![
            make_budget_month(
                NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
                Some(NaiveDate::from_ymd_opt(2025, 1, 31).unwrap()),
            ),
            make_budget_month(
                NaiveDate::from_ymd_opt(2025, 2, 1).unwrap(),
                Some(NaiveDate::from_ymd_opt(2025, 2, 28).unwrap()),
            ),
        ];
        db.replace_budget_months(&old_months).await.unwrap();

        // Replace with a completely different set.
        let new_months = vec![make_budget_month(
            NaiveDate::from_ymd_opt(2025, 3, 1).unwrap(),
            Some(NaiveDate::from_ymd_opt(2025, 3, 31).unwrap()),
        )];
        db.replace_budget_months(&new_months).await.unwrap();

        let all = db.list_budget_months().await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, new_months[0].id);
        assert_eq!(
            all[0].start_date,
            NaiveDate::from_ymd_opt(2025, 3, 1).unwrap()
        );

        // Old months should be gone.
        let ids: Vec<_> = all.iter().map(|m| m.id).collect();
        assert!(!ids.contains(&old_months[0].id));
        assert!(!ids.contains(&old_months[1].id));
    }

    #[tokio::test]
    async fn test_list_budget_months_roundtrip() {
        let db = setup_db().await;

        let month = make_budget_month(NaiveDate::from_ymd_opt(2025, 4, 1).unwrap(), None);
        db.replace_budget_months(std::slice::from_ref(&month))
            .await
            .unwrap();

        let all = db.list_budget_months().await.unwrap();
        assert_eq!(all.len(), 1);

        let fetched = &all[0];
        assert_eq!(fetched.id, month.id);
        assert_eq!(
            fetched.start_date,
            NaiveDate::from_ymd_opt(2025, 4, 1).unwrap()
        );
        assert!(fetched.end_date.is_none());
        assert_eq!(fetched.salary_transactions_detected, 0);
    }

    // -----------------------------------------------------------------------
    // Project tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_insert_project_and_list_roundtrip() {
        let db = setup_db().await;

        let cat = make_category("Home Improvement");
        db.insert_category(&cat).await.unwrap();

        let proj = make_project(cat.id);
        db.insert_project(&proj).await.unwrap();

        let all = db.list_projects().await.unwrap();
        assert_eq!(all.len(), 1);

        let fetched = &all[0];
        assert_eq!(fetched.id, proj.id);
        assert_eq!(fetched.name, "Kitchen Renovation");
        assert_eq!(fetched.category_id, cat.id);
        assert_eq!(
            fetched.start_date,
            NaiveDate::from_ymd_opt(2025, 1, 1).unwrap()
        );
        assert!(fetched.end_date.is_none());
        assert_eq!(fetched.budget_amount, Some(dec!(10000.00)));
    }

    #[tokio::test]
    async fn test_get_project_by_id() {
        let db = setup_db().await;

        let cat = make_category("Education");
        db.insert_category(&cat).await.unwrap();

        let proj = make_project(cat.id);
        db.insert_project(&proj).await.unwrap();

        let fetched = db.get_project(proj.id).await.unwrap().unwrap();
        assert_eq!(fetched.id, proj.id);
        assert_eq!(fetched.name, "Kitchen Renovation");
    }

    #[tokio::test]
    async fn test_update_project_changes_fields() {
        let db = setup_db().await;

        let cat1 = make_category("Home");
        let cat2 = make_category("Garden");
        db.insert_category(&cat1).await.unwrap();
        db.insert_category(&cat2).await.unwrap();

        let mut proj = make_project(cat1.id);
        db.insert_project(&proj).await.unwrap();

        proj.name = "Garden Landscaping".into();
        proj.category_id = cat2.id;
        proj.end_date = Some(NaiveDate::from_ymd_opt(2025, 12, 31).unwrap());
        proj.budget_amount = Some(dec!(5000.00));

        db.update_project(&proj).await.unwrap();

        let fetched = db.get_project(proj.id).await.unwrap().unwrap();
        assert_eq!(fetched.name, "Garden Landscaping");
        assert_eq!(fetched.category_id, cat2.id);
        assert_eq!(
            fetched.end_date,
            Some(NaiveDate::from_ymd_opt(2025, 12, 31).unwrap())
        );
        assert_eq!(fetched.budget_amount, Some(dec!(5000.00)));
    }

    #[tokio::test]
    async fn test_delete_project() {
        let db = setup_db().await;

        let cat = make_category("Misc");
        db.insert_category(&cat).await.unwrap();

        let proj = make_project(cat.id);
        db.insert_project(&proj).await.unwrap();

        db.delete_project(proj.id).await.unwrap();

        let result = db.get_project(proj.id).await.unwrap();
        assert!(result.is_none());
    }
}
