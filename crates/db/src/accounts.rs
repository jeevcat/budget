use budget_core::models::{Account, AccountId};

use crate::{Db, DbError, row_to_account};

impl Db {
    /// Insert or update an account by primary key.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn upsert_account(&self, account: &Account) -> Result<(), DbError> {
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
                 connection_id = excluded.connection_id
                 -- nickname is intentionally preserved across upserts",
        )
        .bind(account.id)
        .bind(&account.provider_account_id)
        .bind(&account.name)
        .bind(&account.institution)
        .bind(account.account_type.to_string())
        .bind(&account.currency)
        .bind(account.connection_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// List all accounts.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn list_accounts(&self) -> Result<Vec<Account>, DbError> {
        let pool = &self.0;
        let rows = sqlx::query(
            "SELECT id, provider_account_id, name, nickname, institution, account_type, currency, connection_id FROM accounts",
        )
        .fetch_all(pool)
        .await?;
        rows.iter().map(row_to_account).collect()
    }

    /// List accounts that have a bank connection (excludes CSV-only accounts).
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn list_connected_accounts(&self) -> Result<Vec<Account>, DbError> {
        let pool = &self.0;
        let rows = sqlx::query(
            "SELECT id, provider_account_id, name, nickname, institution, account_type, currency, connection_id \
             FROM accounts WHERE connection_id IS NOT NULL",
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
    /// Returns `DbError` if the query fails.
    pub async fn get_account(&self, id: AccountId) -> Result<Option<Account>, DbError> {
        let pool = &self.0;
        let row = sqlx::query(
            "SELECT id, provider_account_id, name, nickname, institution, account_type, currency, connection_id FROM accounts WHERE id = $1",
        )
        .bind(id)
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
    /// Returns `DbError` if the query fails.
    pub async fn get_account_by_provider_id(
        &self,
        provider_account_id: &str,
    ) -> Result<Option<Account>, DbError> {
        let pool = &self.0;
        let row = sqlx::query(
            "SELECT id, provider_account_id, name, nickname, institution, account_type, currency, connection_id
             FROM accounts WHERE provider_account_id = $1",
        )
        .bind(provider_account_id)
        .fetch_optional(pool)
        .await?;
        row.as_ref().map(row_to_account).transpose()
    }

    /// Set or clear the user-defined nickname for an account.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn update_account_nickname(
        &self,
        id: AccountId,
        nickname: Option<&str>,
    ) -> Result<(), DbError> {
        let pool = &self.0;
        sqlx::query("UPDATE accounts SET nickname = $1 WHERE id = $2")
            .bind(nickname)
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }
}
