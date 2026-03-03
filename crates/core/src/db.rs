use std::collections::HashMap;

use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row};

use crate::models::{
    Account, AccountId, AccountType, BudgetMode, BudgetType, Category, CategoryId, CategoryMethod,
    Connection, ConnectionId, ConnectionStatus, CorrelationType, Rule, RuleCondition, RuleId,
    RuleType, Transaction, TransactionId,
};

// ---------------------------------------------------------------------------
// Private parse helpers
// ---------------------------------------------------------------------------

fn parse_enum<T: std::str::FromStr>(row: &PgRow, col: &str) -> Result<T, sqlx::Error>
where
    T::Err: std::error::Error + Send + Sync + 'static,
{
    let s: String = row.try_get(col)?;
    s.parse::<T>().map_err(|e| sqlx::Error::ColumnDecode {
        index: col.to_owned(),
        source: Box::new(e),
    })
}

fn parse_enum_opt<T: std::str::FromStr>(row: &PgRow, col: &str) -> Result<Option<T>, sqlx::Error>
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

fn row_to_account(row: &PgRow) -> Result<Account, sqlx::Error> {
    Ok(Account {
        id: row.try_get("id")?,
        provider_account_id: row.try_get("provider_account_id")?,
        name: row.try_get("name")?,
        nickname: row.try_get("nickname")?,
        institution: row.try_get("institution")?,
        account_type: parse_enum::<AccountType>(row, "account_type")?,
        currency: row.try_get("currency")?,
        connection_id: row.try_get("connection_id")?,
    })
}

fn row_to_connection(row: &PgRow) -> Result<Connection, sqlx::Error> {
    Ok(Connection {
        id: row.try_get("id")?,
        provider: row.try_get("provider")?,
        provider_session_id: row.try_get("provider_session_id")?,
        institution_name: row.try_get("institution_name")?,
        valid_until: row.try_get("valid_until")?,
        status: parse_enum::<ConnectionStatus>(row, "status")?,
    })
}

fn row_to_category(row: &PgRow) -> Result<Category, sqlx::Error> {
    Ok(Category {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        parent_id: row.try_get("parent_id")?,
        budget_mode: parse_enum_opt::<BudgetMode>(row, "budget_mode")?,
        budget_type: parse_enum_opt::<BudgetType>(row, "budget_type")?,
        budget_amount: row.try_get("budget_amount")?,
        project_start_date: row.try_get("project_start_date")?,
        project_end_date: row.try_get("project_end_date")?,
    })
}

/// Column list shared by all transaction SELECT queries.
const TXN_COLUMNS: &str = "id, account_id, category_id, amount, original_amount, original_currency,
                    merchant_name, remittance_information, posted_date,
                    correlation_id, correlation_type, category_method, suggested_category,
                    counterparty_name, counterparty_iban, counterparty_bic, bank_transaction_code,
                    llm_justification, skip_correlation,
                    merchant_category_code, bank_transaction_code_code, bank_transaction_code_sub_code,
                    exchange_rate, exchange_rate_unit_currency, exchange_rate_type,
                    exchange_rate_contract_id,
                    reference_number, reference_number_schema, note,
                    balance_after_transaction, balance_after_transaction_currency,
                    creditor_account_additional_id, debtor_account_additional_id";

fn row_to_transaction(row: &PgRow) -> Result<Transaction, sqlx::Error> {
    Ok(Transaction {
        id: row.try_get("id")?,
        account_id: row.try_get("account_id")?,
        category_id: row.try_get("category_id")?,
        amount: row.try_get("amount")?,
        original_amount: row.try_get("original_amount")?,
        original_currency: row.try_get("original_currency")?,
        merchant_name: row.try_get("merchant_name")?,
        remittance_information: row.try_get("remittance_information")?,
        posted_date: row.try_get("posted_date")?,
        correlation_id: row.try_get("correlation_id")?,
        correlation_type: parse_enum_opt::<CorrelationType>(row, "correlation_type")?,
        category_method: parse_enum_opt::<CategoryMethod>(row, "category_method")?,
        suggested_category: row.try_get("suggested_category")?,
        counterparty_name: row.try_get("counterparty_name")?,
        counterparty_iban: row.try_get("counterparty_iban")?,
        counterparty_bic: row.try_get("counterparty_bic")?,
        bank_transaction_code: row.try_get("bank_transaction_code")?,
        llm_justification: row.try_get("llm_justification")?,
        skip_correlation: row.try_get("skip_correlation")?,
        merchant_category_code: row.try_get("merchant_category_code")?,
        bank_transaction_code_code: row.try_get("bank_transaction_code_code")?,
        bank_transaction_code_sub_code: row.try_get("bank_transaction_code_sub_code")?,
        exchange_rate: row.try_get("exchange_rate")?,
        exchange_rate_unit_currency: row.try_get("exchange_rate_unit_currency")?,
        exchange_rate_type: row.try_get("exchange_rate_type")?,
        exchange_rate_contract_id: row.try_get("exchange_rate_contract_id")?,
        reference_number: row.try_get("reference_number")?,
        reference_number_schema: row.try_get("reference_number_schema")?,
        note: row.try_get("note")?,
        balance_after_transaction: row.try_get("balance_after_transaction")?,
        balance_after_transaction_currency: row.try_get("balance_after_transaction_currency")?,
        creditor_account_additional_id: row.try_get("creditor_account_additional_id")?,
        debtor_account_additional_id: row.try_get("debtor_account_additional_id")?,
    })
}

fn row_to_rule(row: &PgRow) -> Result<Rule, sqlx::Error> {
    let conditions_json: String = row.try_get("conditions")?;
    let conditions: Vec<RuleCondition> =
        serde_json::from_str(&conditions_json).map_err(|e| sqlx::Error::ColumnDecode {
            index: "conditions".to_owned(),
            source: Box::new(e),
        })?;

    Ok(Rule {
        id: row.try_get("id")?,
        rule_type: parse_enum::<RuleType>(row, "rule_type")?,
        conditions,
        target_category_id: row.try_get("target_category_id")?,
        target_correlation_type: parse_enum_opt::<CorrelationType>(row, "target_correlation_type")?,
        priority: row.try_get("priority")?,
    })
}

// ---------------------------------------------------------------------------
// Db wrapper
// ---------------------------------------------------------------------------

/// Database handle wrapping the connection pool.
///
/// All query functions are methods on this struct so that the pool type is
/// private. Callers never depend on `PgPool` directly.
#[derive(Clone)]
pub struct Db(PgPool);

impl Db {
    /// Open a connection pool to the database at `url`.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the connection fails.
    pub async fn connect(url: &str) -> Result<Self, sqlx::Error> {
        Ok(Self(PgPool::connect(url).await?))
    }

    /// Wrap an existing pool as a `Db` handle.
    #[must_use]
    pub fn from_pool(pool: PgPool) -> Self {
        Self(pool)
    }

    /// Expose the inner pool for callers that need direct pool access
    /// (e.g. running raw queries against the apalis `Jobs` table).
    #[must_use]
    pub fn pool(&self) -> &PgPool {
        &self.0
    }

    /// Run domain migrations.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if any migration fails.
    pub async fn run_migrations(&self) -> Result<(), sqlx::Error> {
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
    /// Returns `sqlx::Error` if the query fails.
    pub async fn list_accounts(&self) -> Result<Vec<Account>, sqlx::Error> {
        let pool = &self.0;
        let rows = sqlx::query(
            "SELECT id, provider_account_id, name, nickname, institution, account_type, currency, connection_id FROM accounts",
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
    /// Returns `sqlx::Error` if the query fails.
    pub async fn get_account_by_provider_id(
        &self,
        provider_account_id: &str,
    ) -> Result<Option<Account>, sqlx::Error> {
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
    /// Returns `sqlx::Error` if the query fails.
    pub async fn update_account_nickname(
        &self,
        id: AccountId,
        nickname: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        let pool = &self.0;
        sqlx::query("UPDATE accounts SET nickname = $1 WHERE id = $2")
            .bind(nickname)
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }

    // ---------------------------------------------------------------------------
    // Transactions
    // ---------------------------------------------------------------------------

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
        .bind(txn.category_id)
        .bind(txn.amount)
        .bind(txn.original_amount)
        .bind(txn.original_currency.as_deref())
        .bind(&txn.merchant_name)
        .bind(&txn.remittance_information)
        .bind(txn.posted_date)
        .bind(txn.correlation_id)
        .bind(txn.correlation_type.map(|ct| ct.to_string()))
        .bind(provider_transaction_id)
        .bind(txn.suggested_category.as_deref())
        .bind(txn.counterparty_name.as_deref())
        .bind(txn.counterparty_iban.as_deref())
        .bind(txn.counterparty_bic.as_deref())
        .bind(txn.bank_transaction_code.as_deref())
        .bind(txn.skip_correlation)
        .bind(txn.merchant_category_code.as_deref())
        .bind(txn.bank_transaction_code_code.as_deref())
        .bind(txn.bank_transaction_code_sub_code.as_deref())
        .bind(txn.exchange_rate.as_deref())
        .bind(txn.exchange_rate_unit_currency.as_deref())
        .bind(txn.exchange_rate_type.as_deref())
        .bind(txn.exchange_rate_contract_id.as_deref())
        .bind(txn.reference_number.as_deref())
        .bind(txn.reference_number_schema.as_deref())
        .bind(txn.note.as_deref())
        .bind(txn.balance_after_transaction)
        .bind(txn.balance_after_transaction_currency.as_deref())
        .bind(&txn.creditor_account_additional_id)
        .bind(&txn.debtor_account_additional_id)
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

    /// List all category names in fully-qualified `"Parent:Child"` format.
    ///
    /// Used to pass existing categories to the LLM so it maps to known names.
    /// Handles both naming conventions: categories whose `name` already
    /// contains the parent prefix and those that store only the leaf name
    /// with a `parent_id` reference.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn list_category_names(&self) -> Result<Vec<String>, sqlx::Error> {
        let categories = self.list_categories().await?;
        let qualified = crate::models::build_qualified_name_map(&categories);
        let mut names: Vec<String> = qualified.into_values().collect();
        names.sort();
        Ok(names)
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
        sqlx::query(
            "INSERT INTO categories (id, name, parent_id, budget_mode, budget_type, budget_amount, project_start_date, project_end_date)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(category.id)
        .bind(&category.name)
        .bind(category.parent_id)
        .bind(category.budget_mode.map(|m| m.to_string()))
        .bind(category.budget_type.map(|t| t.to_string()))
        .bind(category.budget_amount)
        .bind(category.project_start_date)
        .bind(category.project_end_date)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Update all mutable fields of an existing category.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn update_category(&self, category: &Category) -> Result<(), sqlx::Error> {
        let pool = &self.0;
        sqlx::query(
            "UPDATE categories SET name = $1, parent_id = $2, budget_mode = $3, budget_type = $4,
                    budget_amount = $5, project_start_date = $6, project_end_date = $7
             WHERE id = $8",
        )
        .bind(&category.name)
        .bind(category.parent_id)
        .bind(category.budget_mode.map(|m| m.to_string()))
        .bind(category.budget_type.map(|t| t.to_string()))
        .bind(category.budget_amount)
        .bind(category.project_start_date)
        .bind(category.project_end_date)
        .bind(category.id)
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
        let rows = sqlx::query(
            "SELECT id, name, parent_id, budget_mode, budget_type, budget_amount, project_start_date, project_end_date FROM categories ORDER BY name",
        )
        .fetch_all(pool)
        .await?;
        rows.iter().map(row_to_category).collect()
    }

    /// Count transactions per category.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the query fails.
    pub async fn category_transaction_counts(
        &self,
    ) -> Result<HashMap<CategoryId, i64>, sqlx::Error> {
        let pool = &self.0;
        let rows = sqlx::query(
            "SELECT category_id, COUNT(*) as cnt FROM transactions WHERE category_id IS NOT NULL GROUP BY category_id",
        )
        .fetch_all(pool)
        .await?;
        let mut map = HashMap::new();
        for row in &rows {
            let id: CategoryId = row.try_get("category_id")?;
            let count: i64 = row.try_get("cnt")?;
            map.insert(id, count);
        }
        Ok(map)
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
        let row = sqlx::query(
            "SELECT id, name, parent_id, budget_mode, budget_type, budget_amount, project_start_date, project_end_date
             FROM categories WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;
        row.as_ref().map(row_to_category).transpose()
    }

    /// Find a category by name, supporting both storage conventions.
    ///
    /// Resolution order:
    /// 1. Exact match on `name` (handles legacy categories stored as `"Food:Groceries"`).
    /// 2. If `name` contains a colon (e.g. `"Food:Groceries"`), look for a child
    ///    named `"Groceries"` whose parent is named `"Food"`.
    ///
    /// Returns `None` if no match is found.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if a database query fails.
    pub async fn get_category_by_name(&self, name: &str) -> Result<Option<Category>, sqlx::Error> {
        let pool = &self.0;

        // 1. Exact match on stored name
        let row = sqlx::query(
            "SELECT id, name, parent_id, budget_mode, budget_type, budget_amount, project_start_date, project_end_date
             FROM categories WHERE name = $1",
        )
        .bind(name)
        .fetch_optional(pool)
        .await?;

        if let Some(r) = row.as_ref() {
            return row_to_category(r).map(Some);
        }

        // 2. Colon-qualified lookup: "Parent:Child" → find child under parent
        if let Some((parent_name, child_name)) = crate::models::parse_qualified_name(name) {
            let row = sqlx::query(
                "SELECT c.id, c.name, c.parent_id, c.budget_mode, c.budget_type, c.budget_amount, c.project_start_date, c.project_end_date
                 FROM categories c
                 JOIN categories p ON c.parent_id = p.id
                 WHERE p.name = $1 AND c.name = $2",
            )
            .bind(parent_name)
            .bind(child_name)
            .fetch_optional(pool)
            .await?;

            if let Some(r) = row.as_ref() {
                return row_to_category(r).map(Some);
            }
        }

        Ok(None)
    }

    /// Delete a category by its ID, clearing all foreign-key references first.
    ///
    /// Nullifies `category_id` on transactions, `target_category_id` on rules,
    /// and `parent_id` on child categories before removing the row.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if any query fails.
    pub async fn delete_category(&self, id: CategoryId) -> Result<(), sqlx::Error> {
        let pool = &self.0;
        let mut tx = pool.begin().await?;

        sqlx::query("UPDATE transactions SET category_id = NULL WHERE category_id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE rules SET target_category_id = NULL WHERE target_category_id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE categories SET parent_id = NULL WHERE parent_id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM categories WHERE id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
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
        let conditions_json = serde_json::to_string(&rule.conditions)
            .map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
        sqlx::query(
            "INSERT INTO rules (id, rule_type, conditions, target_category_id, target_correlation_type, priority)
             VALUES ($1, $2, $3::jsonb, $4, $5, $6)",
        )
        .bind(rule.id)
        .bind(rule.rule_type.to_string())
        .bind(&conditions_json)
        .bind(rule.target_category_id)
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
            "SELECT id, rule_type, conditions::text as conditions, target_category_id, target_correlation_type, priority
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
            "SELECT id, rule_type, conditions::text as conditions, target_category_id, target_correlation_type, priority
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
            "SELECT id, rule_type, conditions::text as conditions, target_category_id, target_correlation_type, priority
             FROM rules WHERE id = $1",
        )
        .bind(id)
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
        let conditions_json = serde_json::to_string(&rule.conditions)
            .map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
        sqlx::query(
            "UPDATE rules SET rule_type = $1, conditions = $2::jsonb,
                    target_category_id = $3, target_correlation_type = $4, priority = $5
             WHERE id = $6",
        )
        .bind(rule.rule_type.to_string())
        .bind(&conditions_json)
        .bind(rule.target_category_id)
        .bind(rule.target_correlation_type.map(|ct| ct.to_string()))
        .bind(rule.priority)
        .bind(rule.id)
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
            .bind(id)
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

#[cfg(test)]
mod tests {
    use super::*;

    use chrono::NaiveDate;
    use rust_decimal_macros::dec;

    use crate::models::{
        Account, AccountId, AccountType, Category, CategoryId, CategoryMethod, CategoryName,
        CorrelationType, MatchField, Rule, RuleCondition, RuleId, RuleType, Transaction,
        TransactionId,
    };

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn wrap(pool: PgPool) -> Db {
        Db(pool)
    }

    fn make_account() -> Account {
        Account {
            id: AccountId::new(),
            provider_account_id: "prov-acct-001".into(),
            name: "My Checking".into(),
            nickname: None,
            institution: "Test Bank".into(),
            account_type: AccountType::Checking,
            currency: "EUR".into(),
            connection_id: None,
        }
    }

    fn make_category(name: &str) -> Category {
        Category {
            id: CategoryId::new(),
            name: CategoryName::new(name).expect("valid test category name"),
            parent_id: None,
            budget_mode: None,
            budget_type: None,
            budget_amount: None,
            project_start_date: None,
            project_end_date: None,
        }
    }

    fn make_transaction(account_id: AccountId) -> Transaction {
        Transaction {
            account_id,
            amount: dec!(-42.50),
            merchant_name: "Coffee Shop".into(),
            remittance_information: vec!["Morning coffee".into()],
            posted_date: NaiveDate::from_ymd_opt(2025, 3, 15).unwrap(),
            ..Default::default()
        }
    }

    fn make_rule(rule_type: RuleType, priority: i32) -> Rule {
        Rule {
            id: RuleId::new(),
            rule_type,
            conditions: vec![RuleCondition {
                field: MatchField::Merchant,
                pattern: "Coffee.*".into(),
            }],
            target_category_id: None,
            target_correlation_type: None,
            priority,
        }
    }

    // -----------------------------------------------------------------------
    // Account tests
    // -----------------------------------------------------------------------

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_upsert_account_roundtrip(pool: PgPool) {
        let db = wrap(pool);
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

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_upsert_account_replaces_existing(pool: PgPool) {
        let db = wrap(pool);
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

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_list_accounts_returns_all(pool: PgPool) {
        let db = wrap(pool);

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

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_get_account_returns_none_for_nonexistent(pool: PgPool) {
        let db = wrap(pool);
        let result = db.get_account(AccountId::new()).await.unwrap();
        assert!(result.is_none());
    }

    // -----------------------------------------------------------------------
    // Transaction tests
    // -----------------------------------------------------------------------

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_upsert_transaction_roundtrip(pool: PgPool) {
        let db = wrap(pool);
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
        assert_eq!(fetched.remittance_information, vec!["Morning coffee"]);
        assert_eq!(
            fetched.posted_date,
            NaiveDate::from_ymd_opt(2025, 3, 15).unwrap()
        );
        assert!(fetched.category_id.is_none());
        assert!(fetched.correlation_id.is_none());
        assert!(fetched.correlation_type.is_none());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_upsert_transaction_dedup_by_provider_id(pool: PgPool) {
        let db = wrap(pool);
        let acct = make_account();
        db.upsert_account(&acct).await.unwrap();

        let txn1 = make_transaction(acct.id);
        db.upsert_transaction(&txn1, Some("PROV-TXN-001"))
            .await
            .unwrap();

        let mut txn2 = make_transaction(acct.id);
        txn2.merchant_name = "Updated Coffee Shop".into();
        txn2.amount = dec!(-45.00);
        db.upsert_transaction(&txn2, Some("PROV-TXN-001"))
            .await
            .unwrap();

        let all = db.list_transactions().await.unwrap();
        assert_eq!(all.len(), 1, "dedup should prevent duplicate rows");

        let fetched = &all[0];
        assert_eq!(fetched.merchant_name, "Updated Coffee Shop");
        assert_eq!(fetched.amount, dec!(-45.00));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_upsert_transaction_dedup_preserves_local_fields(pool: PgPool) {
        let db = wrap(pool);
        let acct = make_account();
        db.upsert_account(&acct).await.unwrap();

        let cat = make_category("Food & Drink");
        db.insert_category(&cat).await.unwrap();

        let txn = make_transaction(acct.id);
        db.upsert_transaction(&txn, Some("PROV-TXN-002"))
            .await
            .unwrap();

        db.update_transaction_category(txn.id, cat.id, CategoryMethod::Manual, None)
            .await
            .unwrap();

        let mut txn_updated = make_transaction(acct.id);
        txn_updated.merchant_name = "Coffee Shop (corrected)".into();
        txn_updated.amount = dec!(-43.00);
        db.upsert_transaction(&txn_updated, Some("PROV-TXN-002"))
            .await
            .unwrap();

        let all = db.list_transactions().await.unwrap();
        assert_eq!(all.len(), 1);

        let fetched = &all[0];
        assert_eq!(fetched.merchant_name, "Coffee Shop (corrected)");
        assert_eq!(fetched.amount, dec!(-43.00));
        assert_eq!(fetched.category_id, Some(cat.id));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_get_uncategorized_transactions(pool: PgPool) {
        let db = wrap(pool);
        let acct = make_account();
        db.upsert_account(&acct).await.unwrap();

        let cat = make_category("Groceries");
        db.insert_category(&cat).await.unwrap();

        let txn_uncat = make_transaction(acct.id);
        db.upsert_transaction(&txn_uncat, None).await.unwrap();

        let mut txn_cat = make_transaction(acct.id);
        txn_cat.category_id = Some(cat.id);
        db.upsert_transaction(&txn_cat, None).await.unwrap();

        let uncat = db.get_uncategorized_transactions().await.unwrap();
        assert_eq!(uncat.len(), 1);
        assert_eq!(uncat[0].id, txn_uncat.id);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_get_uncorrelated_transactions(pool: PgPool) {
        let db = wrap(pool);
        let acct = make_account();
        db.upsert_account(&acct).await.unwrap();

        let cat = make_category("Transfers");
        db.insert_category(&cat).await.unwrap();

        let txn_uncat = make_transaction(acct.id);
        db.upsert_transaction(&txn_uncat, None).await.unwrap();

        let mut txn_uncorr = make_transaction(acct.id);
        txn_uncorr.category_id = Some(cat.id);
        db.upsert_transaction(&txn_uncorr, None).await.unwrap();

        let mut txn_corr = make_transaction(acct.id);
        txn_corr.category_id = Some(cat.id);
        txn_corr.correlation_id = Some(txn_uncorr.id);
        txn_corr.correlation_type = Some(CorrelationType::Transfer);
        db.upsert_transaction(&txn_corr, None).await.unwrap();

        let uncorr = db.get_uncorrelated_transactions().await.unwrap();
        assert_eq!(uncorr.len(), 1);
        assert_eq!(uncorr[0].id, txn_uncorr.id);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_update_transaction_category(pool: PgPool) {
        let db = wrap(pool);
        let acct = make_account();
        db.upsert_account(&acct).await.unwrap();

        let cat = make_category("Dining");
        db.insert_category(&cat).await.unwrap();

        let txn = make_transaction(acct.id);
        db.upsert_transaction(&txn, None).await.unwrap();

        db.update_transaction_category(txn.id, cat.id, CategoryMethod::Manual, None)
            .await
            .unwrap();

        let all = db.list_transactions().await.unwrap();
        assert_eq!(all[0].category_id, Some(cat.id));
        assert_eq!(all[0].category_method, Some(CategoryMethod::Manual));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_update_transaction_category_method_all_variants(pool: PgPool) {
        let db = wrap(pool);
        let acct = make_account();
        db.upsert_account(&acct).await.unwrap();

        let cat = make_category("Food");
        db.insert_category(&cat).await.unwrap();

        for method in [
            CategoryMethod::Manual,
            CategoryMethod::Rule,
            CategoryMethod::Llm,
        ] {
            let txn = make_transaction(acct.id);
            db.upsert_transaction(&txn, None).await.unwrap();

            db.update_transaction_category(txn.id, cat.id, method, None)
                .await
                .unwrap();

            let fetched = db.get_transaction_by_id(txn.id).await.unwrap().unwrap();
            assert_eq!(fetched.category_method, Some(method));
        }
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_new_transaction_has_no_category_method(pool: PgPool) {
        let db = wrap(pool);
        let acct = make_account();
        db.upsert_account(&acct).await.unwrap();

        let txn = make_transaction(acct.id);
        db.upsert_transaction(&txn, None).await.unwrap();

        let fetched = db.get_transaction_by_id(txn.id).await.unwrap().unwrap();
        assert!(fetched.category_method.is_none());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_category_method_survives_provider_resync(pool: PgPool) {
        let db = wrap(pool);
        let acct = make_account();
        db.upsert_account(&acct).await.unwrap();

        let cat = make_category("Transport");
        db.insert_category(&cat).await.unwrap();

        let txn = make_transaction(acct.id);
        db.upsert_transaction(&txn, Some("PROV-METHOD-001"))
            .await
            .unwrap();

        db.update_transaction_category(txn.id, cat.id, CategoryMethod::Rule, None)
            .await
            .unwrap();

        let mut txn_updated = make_transaction(acct.id);
        txn_updated.merchant_name = "Updated Merchant".into();
        db.upsert_transaction(&txn_updated, Some("PROV-METHOD-001"))
            .await
            .unwrap();

        let fetched = db.list_transactions().await.unwrap();
        assert_eq!(fetched.len(), 1);
        assert_eq!(fetched[0].merchant_name, "Updated Merchant");
        assert_eq!(fetched[0].category_id, Some(cat.id));
        assert_eq!(fetched[0].category_method, Some(CategoryMethod::Rule));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_update_transaction_correlation(pool: PgPool) {
        let db = wrap(pool);
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

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_list_transactions_by_account(pool: PgPool) {
        let db = wrap(pool);

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

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_get_rule_eligible_transactions(pool: PgPool) {
        let db = wrap(pool);
        let acct = make_account();
        db.upsert_account(&acct).await.unwrap();

        let cat = make_category("Food");
        db.insert_category(&cat).await.unwrap();

        // Uncategorized — should be included
        let txn_uncat = make_transaction(acct.id);
        db.upsert_transaction(&txn_uncat, None).await.unwrap();

        // LLM-categorized — should be included
        let txn_llm = make_transaction(acct.id);
        db.upsert_transaction(&txn_llm, None).await.unwrap();
        db.update_transaction_category(txn_llm.id, cat.id, CategoryMethod::Llm, None)
            .await
            .unwrap();

        // Rule-categorized — should be excluded
        let txn_rule = make_transaction(acct.id);
        db.upsert_transaction(&txn_rule, None).await.unwrap();
        db.update_transaction_category(txn_rule.id, cat.id, CategoryMethod::Rule, None)
            .await
            .unwrap();

        // Manual-categorized — should be excluded
        let txn_manual = make_transaction(acct.id);
        db.upsert_transaction(&txn_manual, None).await.unwrap();
        db.update_transaction_category(txn_manual.id, cat.id, CategoryMethod::Manual, None)
            .await
            .unwrap();

        let eligible = db.get_rule_eligible_transactions().await.unwrap();
        let ids: Vec<_> = eligible.iter().map(|t| t.id).collect();

        assert_eq!(eligible.len(), 2);
        assert!(ids.contains(&txn_uncat.id));
        assert!(ids.contains(&txn_llm.id));
        assert!(!ids.contains(&txn_rule.id));
        assert!(!ids.contains(&txn_manual.id));
    }

    // -----------------------------------------------------------------------
    // Category tests
    // -----------------------------------------------------------------------

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_insert_category_and_list_roundtrip(pool: PgPool) {
        let db = wrap(pool);

        let cat1 = make_category("Groceries");
        let cat2 = make_category("Transport");

        db.insert_category(&cat1).await.unwrap();
        db.insert_category(&cat2).await.unwrap();

        let all = db.list_categories().await.unwrap();
        assert_eq!(all.len(), 2);

        let names: Vec<_> = all.iter().map(|c| c.name.as_ref()).collect();
        assert!(names.contains(&"Groceries"));
        assert!(names.contains(&"Transport"));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_get_category_by_id(pool: PgPool) {
        let db = wrap(pool);
        let cat = make_category("Entertainment");
        db.insert_category(&cat).await.unwrap();

        let fetched = db.get_category(cat.id).await.unwrap().unwrap();
        assert_eq!(fetched.id, cat.id);
        assert_eq!(fetched.name, "Entertainment");
        assert!(fetched.parent_id.is_none());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_get_category_by_name(pool: PgPool) {
        let db = wrap(pool);
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

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_get_category_by_name_returns_none_for_nonexistent(pool: PgPool) {
        let db = wrap(pool);
        let result = db.get_category_by_name("Nonexistent").await.unwrap();
        assert!(result.is_none());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_delete_category(pool: PgPool) {
        let db = wrap(pool);
        let cat = make_category("ToDelete");
        db.insert_category(&cat).await.unwrap();

        db.delete_category(cat.id).await.unwrap();

        let result = db.get_category(cat.id).await.unwrap();
        assert!(result.is_none());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_insert_category_with_budget_fields(pool: PgPool) {
        let db = wrap(pool);

        let mut cat = make_category("Rent");
        cat.budget_mode = Some(BudgetMode::Monthly);
        cat.budget_amount = Some(dec!(1500.00));
        db.insert_category(&cat).await.unwrap();

        let fetched = db.get_category(cat.id).await.unwrap().unwrap();
        assert_eq!(fetched.budget_mode, Some(BudgetMode::Monthly));
        assert_eq!(fetched.budget_amount, Some(dec!(1500.00)));
        assert!(fetched.project_start_date.is_none());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_update_category_changes_budget_fields(pool: PgPool) {
        let db = wrap(pool);

        let mut cat = make_category("Utilities");
        cat.budget_mode = Some(BudgetMode::Monthly);
        cat.budget_amount = Some(dec!(200.00));
        db.insert_category(&cat).await.unwrap();

        cat.budget_mode = Some(BudgetMode::Annual);
        cat.budget_amount = Some(dec!(2400.00));
        db.update_category(&cat).await.unwrap();

        let fetched = db.get_category(cat.id).await.unwrap().unwrap();
        assert_eq!(fetched.budget_mode, Some(BudgetMode::Annual));
        assert_eq!(fetched.budget_amount, Some(dec!(2400.00)));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_insert_project_category(pool: PgPool) {
        let db = wrap(pool);

        let mut cat = make_category("Kitchen Renovation");
        cat.budget_mode = Some(BudgetMode::Project);
        cat.budget_amount = Some(dec!(10000.00));
        cat.project_start_date = Some(NaiveDate::from_ymd_opt(2025, 1, 1).unwrap());
        cat.project_end_date = Some(NaiveDate::from_ymd_opt(2025, 6, 30).unwrap());
        db.insert_category(&cat).await.unwrap();

        let fetched = db.get_category(cat.id).await.unwrap().unwrap();
        assert_eq!(fetched.budget_mode, Some(BudgetMode::Project));
        assert_eq!(fetched.budget_amount, Some(dec!(10000.00)));
        assert_eq!(
            fetched.project_start_date,
            Some(NaiveDate::from_ymd_opt(2025, 1, 1).unwrap())
        );
        assert_eq!(
            fetched.project_end_date,
            Some(NaiveDate::from_ymd_opt(2025, 6, 30).unwrap())
        );
    }

    // -----------------------------------------------------------------------
    // Rule tests
    // -----------------------------------------------------------------------

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_insert_rule_and_list_roundtrip(pool: PgPool) {
        let db = wrap(pool);

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
        assert_eq!(fetched.conditions.len(), 1);
        assert_eq!(fetched.conditions[0].field, MatchField::Merchant);
        assert_eq!(fetched.conditions[0].pattern, "Coffee.*");
        assert_eq!(fetched.target_category_id, Some(cat.id));
        assert!(fetched.target_correlation_type.is_none());
        assert_eq!(fetched.priority, 10);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_list_rules_by_type_filters_and_orders_by_priority_desc(pool: PgPool) {
        let db = wrap(pool);

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
        assert_eq!(cat_rules[0].priority, 100);
        assert_eq!(cat_rules[1].priority, 1);

        let corr_rules = db.list_rules_by_type(RuleType::Correlation).await.unwrap();
        assert_eq!(corr_rules.len(), 1);
        assert_eq!(
            corr_rules[0].target_correlation_type,
            Some(CorrelationType::Transfer)
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_get_rule_by_id(pool: PgPool) {
        let db = wrap(pool);
        let rule = make_rule(RuleType::Categorization, 5);
        db.insert_rule(&rule).await.unwrap();

        let fetched = db.get_rule(rule.id).await.unwrap().unwrap();
        assert_eq!(fetched.id, rule.id);
        assert_eq!(fetched.priority, 5);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_update_rule_changes_fields(pool: PgPool) {
        let db = wrap(pool);

        let cat = make_category("Travel");
        db.insert_category(&cat).await.unwrap();

        let mut rule = make_rule(RuleType::Categorization, 5);
        db.insert_rule(&rule).await.unwrap();

        rule.conditions = vec![RuleCondition {
            field: MatchField::Description,
            pattern: "Hotel.*".into(),
        }];
        rule.target_category_id = Some(cat.id);
        rule.priority = 20;

        db.update_rule(&rule).await.unwrap();

        let fetched = db.get_rule(rule.id).await.unwrap().unwrap();
        assert_eq!(fetched.conditions.len(), 1);
        assert_eq!(fetched.conditions[0].field, MatchField::Description);
        assert_eq!(fetched.conditions[0].pattern, "Hotel.*");
        assert_eq!(fetched.target_category_id, Some(cat.id));
        assert_eq!(fetched.priority, 20);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_delete_rule(pool: PgPool) {
        let db = wrap(pool);
        let rule = make_rule(RuleType::Categorization, 1);
        db.insert_rule(&rule).await.unwrap();

        db.delete_rule(rule.id).await.unwrap();

        let result = db.get_rule(rule.id).await.unwrap();
        assert!(result.is_none());
    }

    // -----------------------------------------------------------------------
    // Budget Month tests
    // -----------------------------------------------------------------------

    // -----------------------------------------------------------------------
    // Categorized merchant groups
    // -----------------------------------------------------------------------

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_get_categorized_merchant_groups_empty(pool: PgPool) {
        let db = wrap(pool);
        let groups = db.get_categorized_merchant_groups().await.unwrap();
        assert!(groups.is_empty());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_get_categorized_merchant_groups_excludes_single_occurrence(pool: PgPool) {
        let db = wrap(pool);
        let acct = make_account();
        db.upsert_account(&acct).await.unwrap();

        let cat = make_category("Groceries");
        db.insert_category(&cat).await.unwrap();

        let mut txn = make_transaction(acct.id);
        txn.category_id = Some(cat.id);
        txn.merchant_name = "LIDL 1234".into();
        db.upsert_transaction(&txn, None).await.unwrap();

        let groups = db.get_categorized_merchant_groups().await.unwrap();
        assert!(groups.is_empty());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_get_categorized_merchant_groups_returns_qualifying(pool: PgPool) {
        let db = wrap(pool);
        let acct = make_account();
        db.upsert_account(&acct).await.unwrap();

        let cat = make_category("Groceries");
        db.insert_category(&cat).await.unwrap();

        for _ in 0..3 {
            let mut txn = make_transaction(acct.id);
            txn.category_id = Some(cat.id);
            txn.merchant_name = "LIDL 1234".into();
            db.upsert_transaction(&txn, None).await.unwrap();
        }

        let groups = db.get_categorized_merchant_groups().await.unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].0, cat.id);
        assert_eq!(groups[0].1, "LIDL 1234");
        assert_eq!(groups[0].2, 3);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_get_categorized_merchant_groups_excludes_uncategorized(pool: PgPool) {
        let db = wrap(pool);
        let acct = make_account();
        db.upsert_account(&acct).await.unwrap();

        for _ in 0..2 {
            let mut txn = make_transaction(acct.id);
            txn.merchant_name = "LIDL 1234".into();
            db.upsert_transaction(&txn, None).await.unwrap();
        }

        let groups = db.get_categorized_merchant_groups().await.unwrap();
        assert!(groups.is_empty());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_get_categorized_merchant_groups_orders_by_count_desc(pool: PgPool) {
        let db = wrap(pool);
        let acct = make_account();
        db.upsert_account(&acct).await.unwrap();

        let cat = make_category("Food");
        db.insert_category(&cat).await.unwrap();

        for _ in 0..2 {
            let mut txn = make_transaction(acct.id);
            txn.category_id = Some(cat.id);
            txn.merchant_name = "ALDI".into();
            db.upsert_transaction(&txn, None).await.unwrap();
        }

        for _ in 0..4 {
            let mut txn = make_transaction(acct.id);
            txn.category_id = Some(cat.id);
            txn.merchant_name = "LIDL".into();
            db.upsert_transaction(&txn, None).await.unwrap();
        }

        let groups = db.get_categorized_merchant_groups().await.unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].1, "LIDL");
        assert_eq!(groups[0].2, 4);
        assert_eq!(groups[1].1, "ALDI");
        assert_eq!(groups[1].2, 2);
    }

    // -----------------------------------------------------------------------
    // Correlation system tests
    // -----------------------------------------------------------------------

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_link_correlation_pair_atomic(pool: PgPool) {
        let db = wrap(pool);
        let acct = make_account();
        db.upsert_account(&acct).await.unwrap();

        let cat = make_category("Transfers");
        db.insert_category(&cat).await.unwrap();

        let mut txn_a = make_transaction(acct.id);
        txn_a.category_id = Some(cat.id);
        txn_a.amount = dec!(-500.00);
        db.upsert_transaction(&txn_a, None).await.unwrap();

        let mut txn_b = make_transaction(acct.id);
        txn_b.category_id = Some(cat.id);
        txn_b.amount = dec!(500.00);
        db.upsert_transaction(&txn_b, None).await.unwrap();

        let linked = db
            .link_correlation_pair(txn_a.id, txn_b.id, CorrelationType::Transfer)
            .await
            .unwrap();
        assert!(linked, "first link should succeed");

        let a = db.get_transaction_by_id(txn_a.id).await.unwrap().unwrap();
        let b = db.get_transaction_by_id(txn_b.id).await.unwrap().unwrap();

        assert_eq!(a.correlation_id, Some(txn_b.id));
        assert_eq!(b.correlation_id, Some(txn_a.id));
        assert_eq!(a.correlation_type, Some(CorrelationType::Transfer));
        assert_eq!(b.correlation_type, Some(CorrelationType::Transfer));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_link_correlation_pair_rejects_already_linked(pool: PgPool) {
        let db = wrap(pool);
        let acct = make_account();
        db.upsert_account(&acct).await.unwrap();

        let cat = make_category("Transfers");
        db.insert_category(&cat).await.unwrap();

        let mut txn_a = make_transaction(acct.id);
        txn_a.category_id = Some(cat.id);
        txn_a.amount = dec!(-500.00);
        db.upsert_transaction(&txn_a, None).await.unwrap();

        let mut txn_b = make_transaction(acct.id);
        txn_b.category_id = Some(cat.id);
        txn_b.amount = dec!(500.00);
        db.upsert_transaction(&txn_b, None).await.unwrap();

        let mut txn_c = make_transaction(acct.id);
        txn_c.category_id = Some(cat.id);
        txn_c.amount = dec!(500.00);
        db.upsert_transaction(&txn_c, None).await.unwrap();

        // First pair succeeds
        let linked = db
            .link_correlation_pair(txn_a.id, txn_b.id, CorrelationType::Transfer)
            .await
            .unwrap();
        assert!(linked);

        // Second pair trying to re-link txn_a should fail
        let rejected = db
            .link_correlation_pair(txn_a.id, txn_c.id, CorrelationType::Transfer)
            .await
            .unwrap();
        assert!(!rejected, "should reject when side A is already linked");

        // txn_c should remain uncorrelated
        let c = db.get_transaction_by_id(txn_c.id).await.unwrap().unwrap();
        assert!(c.correlation_id.is_none());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_skip_correlation_excludes_from_candidates(pool: PgPool) {
        let db = wrap(pool);
        let acct = make_account();
        db.upsert_account(&acct).await.unwrap();

        let cat = make_category("Transfers");
        db.insert_category(&cat).await.unwrap();

        let mut txn_a = make_transaction(acct.id);
        txn_a.category_id = Some(cat.id);
        txn_a.amount = dec!(-100.00);
        db.upsert_transaction(&txn_a, None).await.unwrap();

        let mut txn_b = make_transaction(acct.id);
        txn_b.category_id = Some(cat.id);
        txn_b.amount = dec!(100.00);
        db.upsert_transaction(&txn_b, None).await.unwrap();

        // Before skip: txn_b appears as a candidate
        let candidates = db
            .get_correlation_candidates(dec!(100.00), txn_a.id, txn_a.posted_date)
            .await
            .unwrap();
        assert_eq!(candidates.len(), 1);

        // Set skip on txn_b
        db.set_skip_correlation(txn_b.id, true).await.unwrap();

        // After skip: txn_b no longer appears
        let candidates = db
            .get_correlation_candidates(dec!(100.00), txn_a.id, txn_a.posted_date)
            .await
            .unwrap();
        assert!(candidates.is_empty());

        // Also excluded from get_uncorrelated_transactions
        let uncorr = db.get_uncorrelated_transactions().await.unwrap();
        let ids: Vec<_> = uncorr.iter().map(|t| t.id).collect();
        assert!(!ids.contains(&txn_b.id));
    }

    /// Verify the TEXT → UUID migration preserves data and FK integrity.
    ///
    /// Runs all migrations up to (but not including) the UUID migration,
    /// seeds rows with TEXT UUIDs, applies the UUID migration, then reads
    /// back through the typed `Db` layer.
    #[sqlx::test(migrations = false)]
    async fn test_text_to_uuid_migration_preserves_data(pool: PgPool) {
        // The UUID migration has version 20260227400000.
        const UUID_MIGRATION_VERSION: i64 = 20_260_227_400_000;

        let all = sqlx::migrate!("../../migrations");

        // 1. Run every migration BEFORE the UUID one.
        let pre: Vec<_> = all
            .migrations
            .iter()
            .filter(|m| m.version < UUID_MIGRATION_VERSION)
            .cloned()
            .collect();

        let pre_migrator = sqlx::migrate::Migrator {
            migrations: pre.into(),
            ignore_missing: true,
            locking: true,
            no_tx: false,
        };
        pre_migrator.run(&pool).await.unwrap();

        // 2. Seed TEXT UUID data using raw SQL (schema is still TEXT).
        let cat_id = uuid::Uuid::new_v4().to_string();
        let child_cat_id = uuid::Uuid::new_v4().to_string();
        let conn_id = uuid::Uuid::new_v4().to_string();
        let acct_id = uuid::Uuid::new_v4().to_string();
        let txn_id = uuid::Uuid::new_v4().to_string();
        let rule_id = uuid::Uuid::new_v4().to_string();

        sqlx::query("INSERT INTO categories (id, name) VALUES ($1, 'Food')")
            .bind(&cat_id)
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query("INSERT INTO categories (id, name, parent_id) VALUES ($1, 'Restaurants', $2)")
            .bind(&child_cat_id)
            .bind(&cat_id)
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query(
            "INSERT INTO connections (id, provider, provider_session_id, institution_name, valid_until, status) \
             VALUES ($1, 'mock', 'sess-1', 'Test Bank', NOW() + INTERVAL '1 day', 'active')",
        )
        .bind(&conn_id)
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO accounts (id, provider_account_id, name, institution, account_type, currency, connection_id) \
             VALUES ($1, 'prov-1', 'Checking', 'Test Bank', 'checking', 'EUR', $2)",
        )
        .bind(&acct_id)
        .bind(&conn_id)
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO transactions (id, account_id, category_id, amount, merchant_name, description, posted_date) \
             VALUES ($1, $2, $3, -12.50, 'Cafe', 'Lunch', '2025-06-15')",
        )
        .bind(&txn_id)
        .bind(&acct_id)
        .bind(&cat_id)
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO rules (id, rule_type, conditions, target_category_id, priority) \
             VALUES ($1, 'categorization', '[{\"field\":\"merchant\",\"pattern\":\"Cafe.*\"}]'::jsonb, $2, 10)",
        )
        .bind(&rule_id)
        .bind(&cat_id)
        .execute(&pool)
        .await
        .unwrap();

        // 3. Run the UUID migration (and any after it).
        let post: Vec<_> = all
            .migrations
            .iter()
            .filter(|m| m.version >= UUID_MIGRATION_VERSION)
            .cloned()
            .collect();

        let post_migrator = sqlx::migrate::Migrator {
            migrations: post.into(),
            ignore_missing: true,
            locking: true,
            no_tx: false,
        };
        post_migrator.run(&pool).await.unwrap();

        // 4. Read back through the typed Db layer — proves UUID columns work.
        let db = wrap(pool);

        let accounts = db.list_accounts().await.unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].name, "Checking");
        assert_eq!(accounts[0].id.to_string(), acct_id);
        assert_eq!(accounts[0].connection_id.unwrap().to_string(), conn_id);

        let cats = db.list_categories().await.unwrap();
        assert_eq!(cats.len(), 2);
        let child = cats
            .iter()
            .find(|c| c.name.as_ref() == "Restaurants")
            .unwrap();
        assert_eq!(child.parent_id.unwrap().to_string(), cat_id);

        let (txns, total) = db
            .list_transactions_paginated(10, 0, "", "", "", "")
            .await
            .unwrap();
        assert_eq!(total, 1);
        assert_eq!(txns[0].id.to_string(), txn_id);
        assert_eq!(txns[0].account_id.to_string(), acct_id);
        assert_eq!(txns[0].category_id.unwrap().to_string(), cat_id);

        let rules = db.list_rules().await.unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].id.to_string(), rule_id);
        assert_eq!(rules[0].target_category_id.unwrap().to_string(), cat_id);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_date_proximity_window_excludes_distant(pool: PgPool) {
        let db = wrap(pool);
        let acct = make_account();
        db.upsert_account(&acct).await.unwrap();

        let cat = make_category("Transfers");
        db.insert_category(&cat).await.unwrap();

        let reference_date = NaiveDate::from_ymd_opt(2025, 6, 15).unwrap();

        // Near candidate: within 45 days
        let mut txn_near = make_transaction(acct.id);
        txn_near.category_id = Some(cat.id);
        txn_near.amount = dec!(100.00);
        txn_near.posted_date = NaiveDate::from_ymd_opt(2025, 6, 20).unwrap();
        db.upsert_transaction(&txn_near, None).await.unwrap();

        // Far candidate: outside 45 days
        let mut txn_far = make_transaction(acct.id);
        txn_far.category_id = Some(cat.id);
        txn_far.amount = dec!(100.00);
        txn_far.posted_date = NaiveDate::from_ymd_opt(2025, 3, 1).unwrap();
        db.upsert_transaction(&txn_far, None).await.unwrap();

        let anchor = TransactionId::new();
        let candidates = db
            .get_correlation_candidates(dec!(100.00), anchor, reference_date)
            .await
            .unwrap();

        let ids: Vec<_> = candidates.iter().map(|t| t.id).collect();
        assert!(
            ids.contains(&txn_near.id),
            "near transaction should be included"
        );
        assert!(
            !ids.contains(&txn_far.id),
            "far transaction should be excluded"
        );
    }
}
