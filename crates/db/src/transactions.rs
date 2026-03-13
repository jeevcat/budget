use chrono::NaiveDate;
use rust_decimal::Decimal;
use sqlx::Row;

use budget_core::models::{
    AccountId, CategoryId, CategoryMethod, CorrelationType, Transaction, TransactionId,
};

use crate::{Db, TXN_COLUMNS, row_to_transaction};

impl Db {
    /// Insert or update a transaction using provider-level deduplication.
    ///
    /// When `provider_transaction_id` is `Some`, uses `ON CONFLICT(account_id,
    /// provider_transaction_id)` to update provider-sourced fields while
    /// preserving locally-enriched fields (category, budget month, correlation).
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
                  merchant_name, remittance_information, posted_date,
                  correlation_id, correlation_type, provider_transaction_id, suggested_category,
                  counterparty_name, counterparty_iban, counterparty_bic, bank_transaction_code,
                  skip_correlation,
                  merchant_category_code, bank_transaction_code_code, bank_transaction_code_sub_code,
                  exchange_rate, exchange_rate_unit_currency, exchange_rate_type,
                  exchange_rate_contract_id,
                  reference_number, reference_number_schema, note,
                  balance_after_transaction, balance_after_transaction_currency,
                  creditor_account_additional_id, debtor_account_additional_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18,
                     $19, $20, $21, $22, $23, $24, $25, $26, $27, $28, $29, $30, $31, $32)
             ON CONFLICT(account_id, provider_transaction_id) DO UPDATE SET
                 amount = excluded.amount,
                 original_amount = excluded.original_amount,
                 original_currency = excluded.original_currency,
                 merchant_name = excluded.merchant_name,
                 remittance_information = excluded.remittance_information,
                 posted_date = excluded.posted_date,
                 counterparty_name = excluded.counterparty_name,
                 counterparty_iban = excluded.counterparty_iban,
                 counterparty_bic = excluded.counterparty_bic,
                 bank_transaction_code = excluded.bank_transaction_code,
                 merchant_category_code = excluded.merchant_category_code,
                 bank_transaction_code_code = excluded.bank_transaction_code_code,
                 bank_transaction_code_sub_code = excluded.bank_transaction_code_sub_code,
                 exchange_rate = excluded.exchange_rate,
                 exchange_rate_unit_currency = excluded.exchange_rate_unit_currency,
                 exchange_rate_type = excluded.exchange_rate_type,
                 exchange_rate_contract_id = excluded.exchange_rate_contract_id,
                 reference_number = excluded.reference_number,
                 reference_number_schema = excluded.reference_number_schema,
                 note = excluded.note,
                 balance_after_transaction = excluded.balance_after_transaction,
                 balance_after_transaction_currency = excluded.balance_after_transaction_currency,
                 creditor_account_additional_id = excluded.creditor_account_additional_id,
                 debtor_account_additional_id = excluded.debtor_account_additional_id",
        )
        .bind(txn.id)
        .bind(txn.account_id)
        .bind(txn.categorization.category_id())
        .bind(txn.amount)
        .bind(txn.original_amount)
        .bind(txn.original_currency.as_ref().map(AsRef::<str>::as_ref))
        .bind(&txn.merchant_name)
        .bind(&txn.remittance_information)
        .bind(txn.posted_date)
        .bind(txn.correlation.as_ref().map(|c| c.partner_id))
        .bind(txn.correlation.as_ref().map(|c| c.correlation_type.to_string()))
        .bind(provider_transaction_id)
        .bind(txn.suggested_category.as_deref())
        .bind(txn.counterparty_name.as_deref())
        .bind(txn.counterparty_iban.as_ref().map(AsRef::<str>::as_ref))
        .bind(txn.counterparty_bic.as_ref().map(AsRef::<str>::as_ref))
        .bind(txn.bank_transaction_code.as_deref())
        .bind(txn.skip_correlation)
        .bind(txn.merchant_category_code.as_ref().map(AsRef::<str>::as_ref))
        .bind(txn.bank_transaction_code_code.as_ref().map(AsRef::<str>::as_ref))
        .bind(txn.bank_transaction_code_sub_code.as_ref().map(AsRef::<str>::as_ref))
        .bind(txn.exchange_rate.as_deref())
        .bind(txn.exchange_rate_unit_currency.as_ref().map(AsRef::<str>::as_ref))
        .bind(txn.exchange_rate_type.as_ref().map(ToString::to_string))
        .bind(txn.exchange_rate_contract_id.as_deref())
        .bind(txn.reference_number.as_deref())
        .bind(txn.reference_number_schema.as_ref().map(ToString::to_string))
        .bind(txn.note.as_deref())
        .bind(txn.balance_after_transaction)
        .bind(txn.balance_after_transaction_currency.as_ref().map(AsRef::<str>::as_ref))
        .bind(&txn.creditor_account_additional_id)
        .bind(&txn.debtor_account_additional_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Count how many of the given provider transaction IDs already exist for
    /// an account. Used by CSV import to report duplicate counts.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn count_existing_provider_ids(
        &self,
        account_id: AccountId,
        ids: &[&str],
    ) -> Result<i64, sqlx::Error> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM transactions
             WHERE account_id = $1 AND provider_transaction_id = ANY($2)",
        )
        .bind(account_id)
        .bind(ids)
        .fetch_one(&self.0)
        .await?;
        Ok(row.0)
    }

    /// List all transactions.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn list_transactions(&self) -> Result<Vec<Transaction>, sqlx::Error> {
        let pool = &self.0;
        let rows = sqlx::query(&format!(
            "SELECT {TXN_COLUMNS}
             FROM transactions
             ORDER BY posted_date DESC, merchant_name ASC"
        ))
        .fetch_all(pool)
        .await?;
        rows.iter().map(row_to_transaction).collect()
    }

    /// List transactions with offset/limit pagination and optional filters.
    ///
    /// Returns `(transactions, total_matching_count)`.
    ///
    /// Filter parameters use empty strings to indicate "no filter". The special
    /// value `"__none"` for `category_id` and `category_method` matches rows
    /// where those columns are NULL.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if either query fails.
    pub async fn list_transactions_paginated(
        &self,
        limit: i64,
        offset: i64,
        search: &str,
        category_id: &str,
        account_id: &str,
        category_method: &str,
    ) -> Result<(Vec<Transaction>, i64), sqlx::Error> {
        let pool = &self.0;

        let where_clause = "WHERE ($1 = '' OR LOWER(merchant_name) LIKE '%' || LOWER($1) || '%'
                            OR LOWER(array_to_string(remittance_information, ' ')) LIKE '%' || LOWER($1) || '%')
               AND ($2 = '' OR ($2 = '__none' AND category_id IS NULL)
                            OR ($2 != '__none' AND category_id = $2::uuid))
               AND ($3 = '' OR account_id = $3::uuid)
               AND ($4 = '' OR ($4 = '__none' AND category_method IS NULL)
                            OR ($4 != '__none' AND category_method = $4))";

        let count_sql = format!("SELECT COUNT(*) as cnt FROM transactions {where_clause}");
        let total: i64 = sqlx::query_scalar(&count_sql)
            .bind(search)
            .bind(category_id)
            .bind(account_id)
            .bind(category_method)
            .fetch_one(pool)
            .await?;

        let data_sql = format!(
            "SELECT {TXN_COLUMNS}
             FROM transactions
             {where_clause}
             ORDER BY posted_date DESC, merchant_name ASC
             LIMIT $5 OFFSET $6"
        );
        let rows = sqlx::query(&data_sql)
            .bind(search)
            .bind(category_id)
            .bind(account_id)
            .bind(category_method)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await?;

        let transactions = rows
            .iter()
            .map(row_to_transaction)
            .collect::<Result<Vec<_>, _>>()?;
        Ok((transactions, total))
    }

    /// List transactions belonging to any of the given category IDs.
    ///
    /// Used for targeted fetches such as salary-category transactions needed
    /// for budget month boundary detection.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn list_transactions_by_category_ids(
        &self,
        category_ids: &[CategoryId],
    ) -> Result<Vec<Transaction>, sqlx::Error> {
        let pool = &self.0;
        let ids: Vec<CategoryId> = category_ids.to_vec();
        let rows = sqlx::query(&format!(
            "SELECT {TXN_COLUMNS}
             FROM transactions
             WHERE category_id = ANY($1)
             ORDER BY posted_date DESC, merchant_name ASC"
        ))
        .bind(&ids)
        .fetch_all(pool)
        .await?;
        rows.iter().map(row_to_transaction).collect()
    }

    /// List transactions with `posted_date >= since`.
    ///
    /// Used to fetch only the relevant time period for budget status
    /// computation instead of loading all historical transactions.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn list_transactions_since(
        &self,
        since: NaiveDate,
    ) -> Result<Vec<Transaction>, sqlx::Error> {
        let pool = &self.0;
        let rows = sqlx::query(&format!(
            "SELECT {TXN_COLUMNS}
             FROM transactions
             WHERE posted_date >= $1
             ORDER BY posted_date DESC, merchant_name ASC"
        ))
        .bind(since)
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
        let row = sqlx::query(&format!(
            "SELECT {TXN_COLUMNS}
             FROM transactions
             WHERE id = $1"
        ))
        .bind(id)
        .fetch_optional(pool)
        .await?;
        row.as_ref().map(row_to_transaction).transpose()
    }

    /// Get transactions eligible for rule evaluation.
    ///
    /// Returns transactions that are either uncategorized (`category_id IS NULL`)
    /// or were categorized by the LLM (`category_method = 'llm'`). Manual and
    /// rule-categorized transactions are left alone.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn get_rule_eligible_transactions(&self) -> Result<Vec<Transaction>, sqlx::Error> {
        let pool = &self.0;
        let rows = sqlx::query(&format!(
            "SELECT {TXN_COLUMNS}
             FROM transactions
             WHERE category_id IS NULL OR category_method = 'llm'"
        ))
        .fetch_all(pool)
        .await?;
        rows.iter().map(row_to_transaction).collect()
    }

    /// Get transactions that have not yet been categorized.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn get_uncategorized_transactions(&self) -> Result<Vec<Transaction>, sqlx::Error> {
        let pool = &self.0;
        let rows = sqlx::query(&format!(
            "SELECT {TXN_COLUMNS}
             FROM transactions
             WHERE category_id IS NULL"
        ))
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
        let rows = sqlx::query(&format!(
            "SELECT {TXN_COLUMNS}
             FROM transactions
             WHERE correlation_id IS NULL AND correlation_type IS NULL AND category_id IS NOT NULL
               AND skip_correlation = FALSE"
        ))
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
        reference_date: NaiveDate,
    ) -> Result<Vec<Transaction>, sqlx::Error> {
        let pool = &self.0;
        let rows = sqlx::query(
            &format!("SELECT {TXN_COLUMNS}
             FROM transactions
             WHERE correlation_id IS NULL AND correlation_type IS NULL
               AND category_id IS NOT NULL
               AND skip_correlation = FALSE
               AND amount = $1
               AND id != $2
               AND posted_date BETWEEN ($3 - INTERVAL '45 days')::date AND ($3 + INTERVAL '45 days')::date"),
        )
        .bind(opposite_amount)
        .bind(exclude_id)
        .bind(reference_date)
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
        method: CategoryMethod,
        justification: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        let pool = &self.0;
        sqlx::query(
            "UPDATE transactions SET category_id = $1, category_method = $2, llm_justification = $3 WHERE id = $4",
        )
        .bind(category_id)
        .bind(method.to_string())
        .bind(justification)
        .bind(id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Clear the category and method on a transaction so rules can re-evaluate it.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn clear_transaction_category(&self, id: TransactionId) -> Result<(), sqlx::Error> {
        let pool = &self.0;
        sqlx::query(
            "UPDATE transactions SET category_id = NULL, category_method = NULL, llm_justification = NULL WHERE id = $1",
        )
        .bind(id)
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
        .bind(correlation_id)
        .bind(correlation_type.to_string())
        .bind(id)
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
        justification: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        let pool = &self.0;
        sqlx::query(
            "UPDATE transactions SET suggested_category = $1, llm_justification = $2 WHERE id = $3",
        )
        .bind(suggested_category)
        .bind(justification)
        .bind(id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Atomically link two transactions as a correlation pair.
    ///
    /// Uses a transaction with `WHERE correlation_id IS NULL` guards on both
    /// UPDATEs, so if either side was already claimed by a concurrent job the
    /// whole operation is rolled back. Returns `true` if the pair was
    /// successfully linked, `false` if either side was already correlated.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query or transaction management fails.
    pub async fn link_correlation_pair(
        &self,
        id_a: TransactionId,
        id_b: TransactionId,
        correlation_type: CorrelationType,
    ) -> Result<bool, sqlx::Error> {
        let pool = &self.0;
        let mut tx = pool.begin().await?;

        let corr_type_str = correlation_type.to_string();

        let result_a = sqlx::query(
            "UPDATE transactions SET correlation_id = $1, correlation_type = $2
             WHERE id = $3 AND correlation_id IS NULL",
        )
        .bind(id_b)
        .bind(&corr_type_str)
        .bind(id_a)
        .execute(&mut *tx)
        .await?;

        if result_a.rows_affected() == 0 {
            tx.rollback().await?;
            return Ok(false);
        }

        let result_b = sqlx::query(
            "UPDATE transactions SET correlation_id = $1, correlation_type = $2
             WHERE id = $3 AND correlation_id IS NULL",
        )
        .bind(id_a)
        .bind(&corr_type_str)
        .bind(id_b)
        .execute(&mut *tx)
        .await?;

        if result_b.rows_affected() == 0 {
            tx.rollback().await?;
            return Ok(false);
        }

        tx.commit().await?;
        Ok(true)
    }

    /// Set or clear the `skip_correlation` flag on a transaction.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn set_skip_correlation(
        &self,
        id: TransactionId,
        skip: bool,
    ) -> Result<(), sqlx::Error> {
        let pool = &self.0;
        sqlx::query("UPDATE transactions SET skip_correlation = $1 WHERE id = $2")
            .bind(skip)
            .bind(id)
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

    /// Group categorized transactions by (category, merchant) for batch rule generation.
    ///
    /// Returns `(category_id, merchant_name, count)` tuples for merchants that
    /// appear at least twice with the same category. Ordered by count descending.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn get_categorized_merchant_groups(
        &self,
    ) -> Result<Vec<(CategoryId, String, i64)>, sqlx::Error> {
        let pool = &self.0;
        let rows = sqlx::query(
            "SELECT category_id, merchant_name, COUNT(*) as cnt
             FROM transactions
             WHERE category_id IS NOT NULL
             GROUP BY category_id, merchant_name
             HAVING COUNT(*) >= 2
             ORDER BY cnt DESC",
        )
        .fetch_all(pool)
        .await?;
        rows.iter()
            .map(|row| {
                let category_id: CategoryId = row.try_get("category_id")?;
                let merchant_name: String = row.try_get("merchant_name")?;
                let count: i64 = row.try_get("cnt")?;
                Ok((category_id, merchant_name, count))
            })
            .collect()
    }

    /// Get distinct merchant names for transactions in a given category.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn get_sibling_merchants(
        &self,
        category_id: CategoryId,
    ) -> Result<Vec<String>, sqlx::Error> {
        let pool = &self.0;
        let rows =
            sqlx::query("SELECT DISTINCT merchant_name FROM transactions WHERE category_id = $1")
                .bind(category_id)
                .fetch_all(pool)
                .await?;
        rows.iter()
            .map(|row| row.try_get("merchant_name"))
            .collect()
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
        let rows = sqlx::query(&format!(
            "SELECT {TXN_COLUMNS}
             FROM transactions
             WHERE account_id = $1"
        ))
        .bind(account_id)
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
        .bind(account_id)
        .fetch_optional(pool)
        .await?;
        match row {
            Some(r) => Ok(r.try_get("max_date")?),
            None => Ok(None),
        }
    }
}
