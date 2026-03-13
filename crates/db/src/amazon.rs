use std::collections::HashSet;

use sqlx::Row;

use crate::{Db, DbError};

/// Amazon enrichment data for a bank transaction.
pub struct AmazonEnrichment {
    pub confidence: String,
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
    /// Insert or update an Amazon transaction, returning its database ID.
    ///
    /// Also inserts order ID associations into `amazon_transaction_orders`.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn upsert_amazon_transaction(
        &self,
        txn: &budget_amazon::AmazonTransaction,
    ) -> Result<uuid::Uuid, DbError> {
        let pool = &self.0;

        let id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO amazon_transactions (transaction_date, amount, currency, statement_descriptor, status, payment_method, dedup_key)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (dedup_key) DO UPDATE SET
                 transaction_date = excluded.transaction_date,
                 amount = excluded.amount,
                 status = excluded.status
             RETURNING id",
        )
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

    /// Get order IDs referenced by transactions but not yet fetched.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn get_unfetched_order_ids(&self) -> Result<Vec<String>, DbError> {
        let pool = &self.0;
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT DISTINCT ato.order_id
             FROM amazon_transaction_orders ato
             LEFT JOIN amazon_orders ao ON ato.order_id = ao.order_id
             WHERE ao.order_id IS NULL",
        )
        .fetch_all(pool)
        .await?;
        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    /// Get dedup keys for all known Amazon transactions.
    ///
    /// Used by the incremental sync to know when to stop fetching.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn get_amazon_dedup_keys(&self) -> Result<HashSet<String>, DbError> {
        let pool = &self.0;
        let rows: Vec<(String,)> = sqlx::query_as("SELECT dedup_key FROM amazon_transactions")
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

    /// Get unmatched Amazon transactions (those without an entry in `amazon_matches`).
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn get_unmatched_amazon_transactions(
        &self,
    ) -> Result<Vec<budget_amazon::AmazonTransaction>, DbError> {
        let pool = &self.0;
        let rows = sqlx::query(
            "SELECT at.transaction_date, at.amount, at.currency, at.statement_descriptor,
                    at.status, at.payment_method, at.dedup_key,
                    ARRAY_AGG(ato.order_id) FILTER (WHERE ato.order_id IS NOT NULL) as order_ids
             FROM amazon_transactions at
             LEFT JOIN amazon_matches am ON at.id = am.amazon_transaction_id
             LEFT JOIN amazon_transaction_orders ato ON at.id = ato.amazon_transaction_id
             WHERE am.id IS NULL
             GROUP BY at.id",
        )
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
                "INSERT INTO amazon_matches (amazon_transaction_id, bank_transaction_id, confidence)
                 SELECT at.id, $2, $3
                 FROM amazon_transactions at
                 WHERE at.dedup_key = $1
                 ON CONFLICT DO NOTHING",
            )
            .bind(&m.amazon_dedup_key)
            .bind(m.bank_transaction_id)
            .bind(match m.confidence {
                budget_amazon::MatchConfidence::Exact => "Exact",
                budget_amazon::MatchConfidence::Approximate => "Approximate",
            })
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
            "SELECT am.confidence, at.transaction_date, at.amount, at.dedup_key
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

        let confidence: String = mr.try_get("confidence")?;
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

        Ok(Some(AmazonEnrichment { confidence, orders }))
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

    /// Get aggregate statistics for Amazon enrichment.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn amazon_enrichment_stats(&self) -> Result<AmazonEnrichmentStats, DbError> {
        let pool = &self.0;

        let total_txns: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM amazon_transactions")
            .fetch_one(pool)
            .await?;

        let matched: (i64,) =
            sqlx::query_as("SELECT COUNT(DISTINCT amazon_transaction_id) FROM amazon_matches")
                .fetch_one(pool)
                .await?;

        let total_orders: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM amazon_orders")
            .fetch_one(pool)
            .await?;

        let total_items: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM amazon_items")
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
