use chrono::{DateTime, Utc};
use sqlx::Row;

use budget_core::models::{Connection, ConnectionId, ConnectionStatus};

use crate::{Db, row_to_connection};

impl Db {
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
        .bind(connection.id)
        .bind(&connection.provider)
        .bind(&connection.provider_session_id)
        .bind(&connection.institution_name)
        .bind(connection.valid_until)
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
        .bind(id)
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
        .bind(id)
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
            .bind(id)
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
        expires_at: DateTime<Utc>,
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
