use std::collections::{HashMap, HashSet};

use sqlx::Row;

use budget_core::models::PayPalAccountId;

use crate::{Db, DbError};

/// Aggregate statistics for `PayPal` enrichment.
#[derive(Debug, serde::Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PayPalEnrichmentStats {
    pub total_transactions: i64,
    pub matched_transactions: i64,
    pub unmatched_transactions: i64,
}

impl Db {
    // -----------------------------------------------------------------------
    // PayPal account CRUD
    // -----------------------------------------------------------------------

    /// Insert a new `PayPal` account with API credentials.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn insert_paypal_account(
        &self,
        account: &budget_core::models::PayPalAccount,
        client_id: &str,
        client_secret: &str,
    ) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO paypal_accounts (id, label, client_id, client_secret, sandbox)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(account.id)
        .bind(&account.label)
        .bind(client_id)
        .bind(client_secret)
        .bind(account.sandbox)
        .execute(&self.0)
        .await?;
        Ok(())
    }

    /// List all `PayPal` accounts (without secrets), ordered by creation time.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn list_paypal_accounts(
        &self,
    ) -> Result<Vec<budget_core::models::PayPalAccount>, DbError> {
        let rows =
            sqlx::query("SELECT id, label, sandbox FROM paypal_accounts ORDER BY created_at")
                .fetch_all(&self.0)
                .await?;

        rows.iter()
            .map(|row| {
                Ok(budget_core::models::PayPalAccount {
                    id: row.try_get("id")?,
                    label: row.try_get("label")?,
                    sandbox: row.try_get("sandbox")?,
                })
            })
            .collect()
    }

    /// Get a single `PayPal` account by ID (without secrets).
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn get_paypal_account(
        &self,
        id: PayPalAccountId,
    ) -> Result<Option<budget_core::models::PayPalAccount>, DbError> {
        let row = sqlx::query("SELECT id, label, sandbox FROM paypal_accounts WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.0)
            .await?;

        row.map(|r| {
            Ok(budget_core::models::PayPalAccount {
                id: r.try_get("id")?,
                label: r.try_get("label")?,
                sandbox: r.try_get("sandbox")?,
            })
        })
        .transpose()
    }

    /// Get `PayPal` API credentials for a specific account.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn get_paypal_credentials(
        &self,
        id: PayPalAccountId,
    ) -> Result<Option<(String, String, bool)>, DbError> {
        let row = sqlx::query(
            "SELECT client_id, client_secret, sandbox FROM paypal_accounts WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.0)
        .await?;

        row.map(|r| {
            Ok((
                r.try_get::<String, _>("client_id")?,
                r.try_get::<String, _>("client_secret")?,
                r.try_get::<bool, _>("sandbox")?,
            ))
        })
        .transpose()
    }

    /// Delete a `PayPal` account. Cascades through transactions and matches.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn delete_paypal_account(&self, id: PayPalAccountId) -> Result<(), DbError> {
        sqlx::query("DELETE FROM paypal_accounts WHERE id = $1")
            .bind(id)
            .execute(&self.0)
            .await?;
        Ok(())
    }

    /// Update a `PayPal` account's label.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn update_paypal_account_label(
        &self,
        id: PayPalAccountId,
        label: &str,
    ) -> Result<(), DbError> {
        sqlx::query("UPDATE paypal_accounts SET label = $2 WHERE id = $1")
            .bind(id)
            .bind(label)
            .execute(&self.0)
            .await?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // PayPal transactions
    // -----------------------------------------------------------------------

    /// Insert or update a `PayPal` transaction and its items.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn upsert_paypal_transaction(
        &self,
        account_id: PayPalAccountId,
        txn: &budget_paypal::PayPalTransaction,
    ) -> Result<uuid::Uuid, DbError> {
        let pool = &self.0;

        let row = sqlx::query(
            "INSERT INTO paypal_transactions
                (paypal_account_id, paypal_transaction_id, transaction_date,
                 amount, currency, merchant_name, event_code, status,
                 payer_email, payer_name)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
             ON CONFLICT (paypal_account_id, paypal_transaction_id) DO UPDATE SET
                merchant_name = EXCLUDED.merchant_name,
                event_code = EXCLUDED.event_code,
                payer_email = EXCLUDED.payer_email,
                payer_name = EXCLUDED.payer_name
             RETURNING id",
        )
        .bind(account_id)
        .bind(&txn.transaction_id)
        .bind(txn.transaction_date)
        .bind(txn.amount)
        .bind(&txn.currency)
        .bind(&txn.merchant_name)
        .bind(&txn.event_code)
        .bind(&txn.status)
        .bind(&txn.payer_email)
        .bind(&txn.payer_name)
        .fetch_one(pool)
        .await?;

        let db_id: uuid::Uuid = row.try_get("id")?;

        // Upsert items: delete old items and re-insert
        sqlx::query("DELETE FROM paypal_items WHERE paypal_transaction_id = $1")
            .bind(db_id)
            .execute(pool)
            .await?;

        for item in &txn.items {
            sqlx::query(
                "INSERT INTO paypal_items
                    (paypal_transaction_id, name, description, quantity,
                     unit_price, unit_price_currency)
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(db_id)
            .bind(&item.name)
            .bind(&item.description)
            .bind(&item.quantity)
            .bind(item.unit_price)
            .bind(&item.unit_price_currency)
            .execute(pool)
            .await?;
        }

        Ok(db_id)
    }

    /// Get known `PayPal` transaction IDs for incremental sync.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn get_paypal_dedup_keys(
        &self,
        account_id: PayPalAccountId,
    ) -> Result<HashSet<String>, DbError> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT paypal_transaction_id FROM paypal_transactions
             WHERE paypal_account_id = $1",
        )
        .bind(account_id)
        .fetch_all(&self.0)
        .await?;

        Ok(rows.into_iter().map(|(k,)| k).collect())
    }

    /// Get the earliest and latest transaction dates for a `PayPal` account.
    ///
    /// Returns `(None, None)` if no transactions exist yet.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn get_paypal_date_range(
        &self,
        account_id: PayPalAccountId,
    ) -> Result<(Option<chrono::NaiveDate>, Option<chrono::NaiveDate>), DbError> {
        let row: (Option<chrono::NaiveDate>, Option<chrono::NaiveDate>) = sqlx::query_as(
            "SELECT MIN(transaction_date), MAX(transaction_date)
             FROM paypal_transactions
             WHERE paypal_account_id = $1",
        )
        .bind(account_id)
        .fetch_one(&self.0)
        .await?;

        Ok(row)
    }

    /// Get bank transactions eligible for `PayPal` matching.
    ///
    /// Returns bank transactions whose merchant contains "paypal" and that
    /// have no existing `PayPal` match.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn get_paypal_matchable_bank_transactions(
        &self,
    ) -> Result<Vec<budget_paypal::BankCandidate>, DbError> {
        let rows = sqlx::query(
            "SELECT t.id, t.amount, t.posted_date, t.merchant_name, t.remittance_information
             FROM transactions t
             WHERE LOWER(t.merchant_name) LIKE '%paypal%'
               AND NOT EXISTS (
                   SELECT 1 FROM paypal_matches pm WHERE pm.bank_transaction_id = t.id
               )",
        )
        .fetch_all(&self.0)
        .await?;

        rows.iter()
            .map(|row| {
                Ok(budget_paypal::BankCandidate {
                    id: row.try_get("id")?,
                    amount: row.try_get("amount")?,
                    posted_date: row.try_get("posted_date")?,
                    merchant_name: row.try_get("merchant_name")?,
                    remittance_information: row.try_get("remittance_information")?,
                })
            })
            .collect()
    }

    /// Get unmatched `PayPal` transactions for an account.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn get_unmatched_paypal_transactions(
        &self,
        account_id: PayPalAccountId,
    ) -> Result<Vec<budget_paypal::PayPalTransaction>, DbError> {
        let rows = sqlx::query(
            "SELECT pt.paypal_transaction_id, pt.transaction_date, pt.amount,
                    pt.currency, pt.merchant_name, pt.event_code, pt.status,
                    pt.payer_email, pt.payer_name
             FROM paypal_transactions pt
             WHERE pt.paypal_account_id = $1
               AND NOT EXISTS (
                   SELECT 1 FROM paypal_matches pm WHERE pm.paypal_transaction_id = pt.id
               )",
        )
        .bind(account_id)
        .fetch_all(&self.0)
        .await?;

        let mut txns = Vec::new();
        for row in &rows {
            txns.push(budget_paypal::PayPalTransaction {
                transaction_id: row.try_get("paypal_transaction_id")?,
                transaction_date: row.try_get("transaction_date")?,
                amount: row.try_get("amount")?,
                currency: row.try_get("currency")?,
                merchant_name: row.try_get("merchant_name")?,
                event_code: row.try_get("event_code")?,
                status: row.try_get("status")?,
                payer_email: row.try_get("payer_email")?,
                payer_name: row.try_get("payer_name")?,
                items: vec![], // Items loaded separately if needed
            });
        }
        Ok(txns)
    }

    /// Insert `PayPal` match results.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn insert_paypal_matches(
        &self,
        matches: &[budget_paypal::MatchResult],
    ) -> Result<(), DbError> {
        for m in matches {
            sqlx::query(
                "INSERT INTO paypal_matches (paypal_transaction_id, bank_transaction_id)
                 SELECT pt.id, $2
                 FROM paypal_transactions pt
                 WHERE pt.paypal_transaction_id = $1
                 ON CONFLICT DO NOTHING",
            )
            .bind(&m.paypal_transaction_id)
            .bind(m.bank_transaction_id)
            .execute(&self.0)
            .await?;
        }
        Ok(())
    }

    /// Get item titles from `PayPal` enrichment for a batch of bank transactions.
    ///
    /// Returns a map from bank transaction ID to item title strings.
    /// Falls back to merchant name if no items exist.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn get_paypal_item_titles_for_transactions(
        &self,
        bank_txn_ids: &[uuid::Uuid],
    ) -> Result<HashMap<uuid::Uuid, Vec<String>>, DbError> {
        // First get item titles
        let item_rows: Vec<(uuid::Uuid, Option<String>)> = sqlx::query_as(
            "SELECT pm.bank_transaction_id, pi.name
             FROM paypal_matches pm
             JOIN paypal_transactions pt ON pm.paypal_transaction_id = pt.id
             JOIN paypal_items pi ON pi.paypal_transaction_id = pt.id
             WHERE pm.bank_transaction_id = ANY($1)
               AND pi.name IS NOT NULL
             ORDER BY pm.bank_transaction_id, pi.name",
        )
        .bind(bank_txn_ids)
        .fetch_all(&self.0)
        .await?;

        let mut map: HashMap<uuid::Uuid, Vec<String>> = HashMap::new();
        for (txn_id, name) in item_rows {
            if let Some(n) = name {
                map.entry(txn_id).or_default().push(n);
            }
        }

        // For matched transactions with no items, fall back to merchant name
        let merchant_rows: Vec<(uuid::Uuid, Option<String>)> = sqlx::query_as(
            "SELECT pm.bank_transaction_id, pt.merchant_name
             FROM paypal_matches pm
             JOIN paypal_transactions pt ON pm.paypal_transaction_id = pt.id
             WHERE pm.bank_transaction_id = ANY($1)
               AND pt.merchant_name IS NOT NULL",
        )
        .bind(bank_txn_ids)
        .fetch_all(&self.0)
        .await?;

        for (txn_id, merchant) in merchant_rows {
            if !map.contains_key(&txn_id)
                && let Some(m) = merchant
            {
                map.entry(txn_id).or_default().push(m);
            }
        }

        Ok(map)
    }

    /// Get `PayPal` enrichment stats for an account.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn paypal_enrichment_stats(
        &self,
        account_id: PayPalAccountId,
    ) -> Result<PayPalEnrichmentStats, DbError> {
        let total: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM paypal_transactions WHERE paypal_account_id = $1")
                .bind(account_id)
                .fetch_one(&self.0)
                .await?;

        let matched: (i64,) = sqlx::query_as(
            "SELECT COUNT(DISTINCT pm.paypal_transaction_id)
             FROM paypal_matches pm
             JOIN paypal_transactions pt ON pm.paypal_transaction_id = pt.id
             WHERE pt.paypal_account_id = $1",
        )
        .bind(account_id)
        .fetch_one(&self.0)
        .await?;

        Ok(PayPalEnrichmentStats {
            total_transactions: total.0,
            matched_transactions: matched.0,
            unmatched_transactions: total.0 - matched.0,
        })
    }
}
