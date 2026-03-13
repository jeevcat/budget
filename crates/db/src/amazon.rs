use std::collections::{HashMap, HashSet};

use sqlx::Row;

use budget_core::models::AmazonAccountId;

use crate::{Db, DbError};

/// Amazon enrichment data for a bank transaction.
pub struct AmazonEnrichment {
    pub orders: Vec<budget_amazon::AmazonOrder>,
}

/// Aggregate statistics for Amazon enrichment.
#[derive(Debug, serde::Serialize)]
pub struct AmazonEnrichmentStats {
    pub total_transactions: i64,
    pub matched_transactions: i64,
    pub unmatched_transactions: i64,
    pub total_orders: i64,
    pub total_items: i64,
}

impl Db {
    // -----------------------------------------------------------------------
    // Amazon account CRUD
    // -----------------------------------------------------------------------

    /// Insert a new Amazon account.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn insert_amazon_account(
        &self,
        account: &budget_core::models::AmazonAccount,
    ) -> Result<(), DbError> {
        sqlx::query("INSERT INTO amazon_accounts (id, label) VALUES ($1, $2)")
            .bind(account.id)
            .bind(&account.label)
            .execute(&self.0)
            .await?;
        Ok(())
    }

    /// List all Amazon accounts, ordered by creation time.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn list_amazon_accounts(
        &self,
    ) -> Result<Vec<budget_core::models::AmazonAccount>, DbError> {
        let rows = sqlx::query("SELECT id, label FROM amazon_accounts ORDER BY created_at")
            .fetch_all(&self.0)
            .await?;

        rows.iter()
            .map(|row| {
                Ok(budget_core::models::AmazonAccount {
                    id: row.try_get("id")?,
                    label: row.try_get("label")?,
                })
            })
            .collect()
    }

    /// Get a single Amazon account by ID.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn get_amazon_account(
        &self,
        id: AmazonAccountId,
    ) -> Result<Option<budget_core::models::AmazonAccount>, DbError> {
        let row = sqlx::query("SELECT id, label FROM amazon_accounts WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.0)
            .await?;

        row.map(|r| {
            Ok(budget_core::models::AmazonAccount {
                id: r.try_get("id")?,
                label: r.try_get("label")?,
            })
        })
        .transpose()
    }

    /// Delete an Amazon account. Cascades through transactions.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn delete_amazon_account(&self, id: AmazonAccountId) -> Result<(), DbError> {
        sqlx::query("DELETE FROM amazon_accounts WHERE id = $1")
            .bind(id)
            .execute(&self.0)
            .await?;
        Ok(())
    }

    /// Update an Amazon account's label.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn update_amazon_account_label(
        &self,
        id: AmazonAccountId,
        label: &str,
    ) -> Result<(), DbError> {
        sqlx::query("UPDATE amazon_accounts SET label = $2 WHERE id = $1")
            .bind(id)
            .bind(label)
            .execute(&self.0)
            .await?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Amazon transactions (now scoped to account)
    // -----------------------------------------------------------------------

    /// Insert or update an Amazon transaction, returning its database ID.
    ///
    /// Also inserts order ID associations into `amazon_transaction_orders`.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn upsert_amazon_transaction(
        &self,
        account_id: AmazonAccountId,
        txn: &budget_amazon::AmazonTransaction,
    ) -> Result<uuid::Uuid, DbError> {
        let pool = &self.0;

        let id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO amazon_transactions (amazon_account_id, transaction_date, amount, currency, statement_descriptor, status, payment_method, dedup_key)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (amazon_account_id, dedup_key) DO UPDATE SET
                 transaction_date = excluded.transaction_date,
                 amount = excluded.amount,
                 status = excluded.status
             RETURNING id",
        )
        .bind(account_id)
        .bind(txn.date)
        .bind(txn.amount)
        .bind(&txn.currency)
        .bind(&txn.statement_descriptor)
        .bind(match txn.status {
            budget_amazon::AmazonTransactionStatus::Charged => "Charged",
            budget_amazon::AmazonTransactionStatus::Refunded => "Refunded",
            budget_amazon::AmazonTransactionStatus::Declined => "Declined",
        })
        .bind(&txn.payment_method)
        .bind(&txn.dedup_key)
        .fetch_one(pool)
        .await?;

        for order_id in &txn.order_ids {
            sqlx::query(
                "INSERT INTO amazon_transaction_orders (amazon_transaction_id, order_id)
                 VALUES ($1, $2)
                 ON CONFLICT DO NOTHING",
            )
            .bind(id)
            .bind(order_id)
            .execute(pool)
            .await?;
        }

        Ok(id)
    }

    /// Insert or update an Amazon order and its items.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn upsert_amazon_order(
        &self,
        order: &budget_amazon::AmazonOrder,
    ) -> Result<(), DbError> {
        let pool = &self.0;

        sqlx::query(
            "INSERT INTO amazon_orders (order_id, order_date, grand_total, subtotal, shipping, vat, promotion)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (order_id) DO UPDATE SET
                 order_date = excluded.order_date,
                 grand_total = excluded.grand_total,
                 subtotal = excluded.subtotal,
                 shipping = excluded.shipping,
                 vat = excluded.vat,
                 promotion = excluded.promotion,
                 fetched_at = CURRENT_TIMESTAMP",
        )
        .bind(&order.order_id)
        .bind(order.order_date)
        .bind(order.grand_total)
        .bind(order.subtotal)
        .bind(order.shipping)
        .bind(order.vat)
        .bind(order.promotion)
        .execute(pool)
        .await?;

        // Replace items: delete existing then re-insert
        sqlx::query("DELETE FROM amazon_items WHERE order_id = $1")
            .bind(&order.order_id)
            .execute(pool)
            .await?;

        for item in &order.items {
            sqlx::query(
                "INSERT INTO amazon_items (order_id, title, asin, price, quantity, seller, image_url)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)",
            )
            .bind(&order.order_id)
            .bind(&item.title)
            .bind(item.asin.as_deref())
            .bind(item.price)
            .bind(i32::try_from(item.quantity).unwrap_or(1))
            .bind(item.seller.as_deref())
            .bind(item.image_url.as_deref())
            .execute(pool)
            .await?;
        }

        Ok(())
    }

    /// Get order IDs referenced by an account's transactions but not yet fetched.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn get_unfetched_order_ids(
        &self,
        account_id: AmazonAccountId,
    ) -> Result<Vec<String>, DbError> {
        let pool = &self.0;
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT DISTINCT ato.order_id
             FROM amazon_transaction_orders ato
             JOIN amazon_transactions at ON ato.amazon_transaction_id = at.id
             LEFT JOIN amazon_orders ao ON ato.order_id = ao.order_id
             WHERE ao.order_id IS NULL AND at.amazon_account_id = $1",
        )
        .bind(account_id)
        .fetch_all(pool)
        .await?;
        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    /// Get dedup keys for an account's Amazon transactions.
    ///
    /// Used by the incremental sync to know when to stop fetching.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn get_amazon_dedup_keys(
        &self,
        account_id: AmazonAccountId,
    ) -> Result<HashSet<String>, DbError> {
        let pool = &self.0;
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT dedup_key FROM amazon_transactions WHERE amazon_account_id = $1",
        )
        .bind(account_id)
        .fetch_all(pool)
        .await?;
        Ok(rows.into_iter().map(|(k,)| k).collect())
    }

    /// Get bank transactions that could match Amazon charges.
    ///
    /// Returns transactions whose merchant name contains "AMZN" or "Amazon"
    /// and that don't already have an Amazon match.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn get_amazon_matchable_bank_transactions(
        &self,
    ) -> Result<Vec<budget_amazon::BankCandidate>, DbError> {
        let pool = &self.0;
        let rows = sqlx::query(
            "SELECT t.id, t.amount, t.posted_date, t.merchant_name
             FROM transactions t
             LEFT JOIN amazon_matches am ON t.id = am.bank_transaction_id
             WHERE am.id IS NULL
               AND (LOWER(t.merchant_name) LIKE '%amzn%' OR LOWER(t.merchant_name) LIKE '%amazon%')",
        )
        .fetch_all(pool)
        .await?;

        rows.iter()
            .map(|row| {
                Ok(budget_amazon::BankCandidate {
                    id: row.try_get("id")?,
                    amount: row.try_get("amount")?,
                    posted_date: row.try_get("posted_date")?,
                    merchant_name: row.try_get("merchant_name")?,
                })
            })
            .collect()
    }

    /// Get unmatched Amazon transactions for a specific account.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn get_unmatched_amazon_transactions(
        &self,
        account_id: AmazonAccountId,
    ) -> Result<Vec<budget_amazon::AmazonTransaction>, DbError> {
        let pool = &self.0;
        let rows = sqlx::query(
            "SELECT at.transaction_date, at.amount, at.currency, at.statement_descriptor,
                    at.status, at.payment_method, at.dedup_key,
                    ARRAY_AGG(ato.order_id) FILTER (WHERE ato.order_id IS NOT NULL) as order_ids
             FROM amazon_transactions at
             LEFT JOIN amazon_matches am ON at.id = am.amazon_transaction_id
             LEFT JOIN amazon_transaction_orders ato ON at.id = ato.amazon_transaction_id
             WHERE am.id IS NULL AND at.amazon_account_id = $1
             GROUP BY at.id",
        )
        .bind(account_id)
        .fetch_all(pool)
        .await?;

        rows.iter()
            .map(|row| {
                let status_str: String = row.try_get("status")?;
                let status = match status_str.as_str() {
                    "Refunded" => budget_amazon::AmazonTransactionStatus::Refunded,
                    "Declined" => budget_amazon::AmazonTransactionStatus::Declined,
                    _ => budget_amazon::AmazonTransactionStatus::Charged,
                };
                let order_ids: Option<Vec<String>> = row.try_get("order_ids")?;

                Ok(budget_amazon::AmazonTransaction {
                    date: row.try_get("transaction_date")?,
                    amount: row.try_get("amount")?,
                    currency: row.try_get("currency")?,
                    statement_descriptor: row.try_get("statement_descriptor")?,
                    status,
                    payment_method: row.try_get("payment_method")?,
                    order_ids: order_ids.unwrap_or_default(),
                    dedup_key: row.try_get("dedup_key")?,
                })
            })
            .collect()
    }

    /// Insert Amazon-to-bank match results.
    ///
    /// Returns the number of matches inserted.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn insert_amazon_matches(
        &self,
        matches: &[budget_amazon::types::MatchResult],
    ) -> Result<usize, DbError> {
        let pool = &self.0;
        let mut count = 0;

        for m in matches {
            let result = sqlx::query(
                "INSERT INTO amazon_matches (amazon_transaction_id, bank_transaction_id)
                 SELECT at.id, $2
                 FROM amazon_transactions at
                 WHERE at.dedup_key = $1
                 ON CONFLICT DO NOTHING",
            )
            .bind(&m.amazon_dedup_key)
            .bind(m.bank_transaction_id)
            .execute(pool)
            .await?;

            #[allow(clippy::cast_possible_truncation)]
            {
                count += result.rows_affected() as usize;
            }
        }

        Ok(count)
    }

    /// Get Amazon enrichment data for a specific bank transaction.
    ///
    /// Returns order details with items for matched Amazon charges.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn get_amazon_enrichment_for_transaction(
        &self,
        bank_txn_id: uuid::Uuid,
    ) -> Result<Option<AmazonEnrichment>, DbError> {
        let pool = &self.0;

        let match_row = sqlx::query(
            "SELECT at.dedup_key
             FROM amazon_matches am
             JOIN amazon_transactions at ON am.amazon_transaction_id = at.id
             WHERE am.bank_transaction_id = $1
             LIMIT 1",
        )
        .bind(bank_txn_id)
        .fetch_optional(pool)
        .await?;

        let Some(mr) = match_row else {
            return Ok(None);
        };

        let dedup_key: String = mr.try_get("dedup_key")?;

        // Get order IDs for this Amazon transaction
        let order_ids: Vec<(String,)> = sqlx::query_as(
            "SELECT ato.order_id
             FROM amazon_transaction_orders ato
             JOIN amazon_transactions at ON ato.amazon_transaction_id = at.id
             WHERE at.dedup_key = $1",
        )
        .bind(&dedup_key)
        .fetch_all(pool)
        .await?;

        let mut orders = Vec::new();
        for (order_id,) in &order_ids {
            let order_row = sqlx::query(
                "SELECT order_date, grand_total, subtotal, shipping, vat, promotion
                 FROM amazon_orders WHERE order_id = $1",
            )
            .bind(order_id)
            .fetch_optional(pool)
            .await?;

            let items_rows = sqlx::query(
                "SELECT title, asin, price, quantity, seller, image_url
                 FROM amazon_items WHERE order_id = $1",
            )
            .bind(order_id)
            .fetch_all(pool)
            .await?;

            let items: Vec<budget_amazon::AmazonItem> = items_rows
                .iter()
                .map(|row| {
                    Ok(budget_amazon::AmazonItem {
                        title: row.try_get("title")?,
                        asin: row.try_get("asin")?,
                        price: row.try_get("price")?,
                        quantity: row
                            .try_get::<i32, _>("quantity")
                            .map(|q| u32::try_from(q).unwrap_or(1))
                            .unwrap_or(1),
                        seller: row.try_get("seller")?,
                        image_url: row.try_get("image_url")?,
                    })
                })
                .collect::<Result<Vec<_>, DbError>>()?;

            orders.push(budget_amazon::AmazonOrder {
                order_id: order_id.clone(),
                order_date: order_row
                    .as_ref()
                    .and_then(|r| r.try_get("order_date").ok())
                    .flatten(),
                grand_total: order_row
                    .as_ref()
                    .and_then(|r| r.try_get("grand_total").ok())
                    .flatten(),
                subtotal: order_row
                    .as_ref()
                    .and_then(|r| r.try_get("subtotal").ok())
                    .flatten(),
                shipping: order_row
                    .as_ref()
                    .and_then(|r| r.try_get("shipping").ok())
                    .flatten(),
                vat: order_row
                    .as_ref()
                    .and_then(|r| r.try_get("vat").ok())
                    .flatten(),
                promotion: order_row
                    .as_ref()
                    .and_then(|r| r.try_get("promotion").ok())
                    .flatten(),
                items,
            });
        }

        Ok(Some(AmazonEnrichment { orders }))
    }

    /// Get all bank transaction IDs that have an Amazon match.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn get_matched_bank_transaction_ids(&self) -> Result<Vec<uuid::Uuid>, DbError> {
        let rows: Vec<(uuid::Uuid,)> = sqlx::query_as(
            "SELECT DISTINCT bank_transaction_id FROM amazon_matches ORDER BY bank_transaction_id",
        )
        .fetch_all(&self.0)
        .await?;

        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    /// Get Amazon item titles for a batch of bank transactions.
    ///
    /// Returns a map from bank transaction ID to the list of item titles
    /// from matched Amazon orders.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn get_amazon_item_titles_for_transactions(
        &self,
        bank_txn_ids: &[uuid::Uuid],
    ) -> Result<HashMap<uuid::Uuid, Vec<String>>, DbError> {
        let rows: Vec<(uuid::Uuid, String)> = sqlx::query_as(
            "SELECT am.bank_transaction_id, ai.title
             FROM amazon_matches am
             JOIN amazon_transactions at ON am.amazon_transaction_id = at.id
             JOIN amazon_transaction_orders ato ON at.id = ato.amazon_transaction_id
             JOIN amazon_items ai ON ato.order_id = ai.order_id
             WHERE am.bank_transaction_id = ANY($1)
             ORDER BY am.bank_transaction_id, ai.title",
        )
        .bind(bank_txn_ids)
        .fetch_all(&self.0)
        .await?;

        let mut map: HashMap<uuid::Uuid, Vec<String>> = HashMap::new();
        for (txn_id, title) in rows {
            map.entry(txn_id).or_default().push(title);
        }
        Ok(map)
    }

    /// Get aggregate statistics for an Amazon account.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn amazon_enrichment_stats(
        &self,
        account_id: AmazonAccountId,
    ) -> Result<AmazonEnrichmentStats, DbError> {
        let pool = &self.0;

        let total_txns: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM amazon_transactions WHERE amazon_account_id = $1")
                .bind(account_id)
                .fetch_one(pool)
                .await?;

        let matched: (i64,) = sqlx::query_as(
            "SELECT COUNT(DISTINCT am.amazon_transaction_id)
             FROM amazon_matches am
             JOIN amazon_transactions at ON am.amazon_transaction_id = at.id
             WHERE at.amazon_account_id = $1",
        )
        .bind(account_id)
        .fetch_one(pool)
        .await?;

        let total_orders: (i64,) = sqlx::query_as(
            "SELECT COUNT(DISTINCT ao.order_id)
             FROM amazon_orders ao
             JOIN amazon_transaction_orders ato ON ao.order_id = ato.order_id
             JOIN amazon_transactions at ON ato.amazon_transaction_id = at.id
             WHERE at.amazon_account_id = $1",
        )
        .bind(account_id)
        .fetch_one(pool)
        .await?;

        let total_items: (i64,) = sqlx::query_as(
            "SELECT COUNT(ai.id)
             FROM amazon_items ai
             JOIN amazon_transaction_orders ato ON ai.order_id = ato.order_id
             JOIN amazon_transactions at ON ato.amazon_transaction_id = at.id
             WHERE at.amazon_account_id = $1",
        )
        .bind(account_id)
        .fetch_one(pool)
        .await?;

        Ok(AmazonEnrichmentStats {
            total_transactions: total_txns.0,
            matched_transactions: matched.0,
            unmatched_transactions: total_txns.0 - matched.0,
            total_orders: total_orders.0,
            total_items: total_items.0,
        })
    }
}
