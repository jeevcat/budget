mod accounts;
mod amazon;
mod categories;
mod connections;
mod error;
mod rules;
mod transactions;

pub use amazon::{AmazonEnrichment, AmazonEnrichmentStats};
pub use error::DbError;

use chrono::NaiveDate;
use rust_decimal::Decimal;
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row};

use budget_core::models::{
    Account, AccountType, BudgetConfig, BudgetMode, BudgetType, Categorization, Category,
    CategoryId, CategoryMethod, Connection, ConnectionStatus, Correlation, CorrelationType,
    ExchangeRateType, ReferenceNumberSchema, Rule, RuleCondition, RuleType, Transaction,
    TransactionId,
};

// ---------------------------------------------------------------------------
// Private parse helpers
// ---------------------------------------------------------------------------

fn parse_enum<T: std::str::FromStr>(row: &PgRow, col: &str) -> Result<T, DbError>
where
    T::Err: std::error::Error + Send + Sync + 'static,
{
    let s: String = row.try_get(col)?;
    Ok(s.parse::<T>().map_err(|e| sqlx::Error::ColumnDecode {
        index: col.to_owned(),
        source: Box::new(e),
    })?)
}

fn parse_enum_opt<T: std::str::FromStr>(row: &PgRow, col: &str) -> Result<Option<T>, DbError>
where
    T::Err: std::error::Error + Send + Sync + 'static,
{
    let s: Option<String> = row.try_get(col)?;
    Ok(s.map(|v| {
        v.parse::<T>().map_err(|e| sqlx::Error::ColumnDecode {
            index: col.to_owned(),
            source: Box::new(e),
        })
    })
    .transpose()?)
}

// ---------------------------------------------------------------------------
// Row-to-domain mappers
// ---------------------------------------------------------------------------

fn row_to_account(row: &PgRow) -> Result<Account, DbError> {
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

fn row_to_connection(row: &PgRow) -> Result<Connection, DbError> {
    Ok(Connection {
        id: row.try_get("id")?,
        provider: row.try_get("provider")?,
        provider_session_id: row.try_get("provider_session_id")?,
        institution_name: row.try_get("institution_name")?,
        valid_until: row.try_get("valid_until")?,
        status: parse_enum::<ConnectionStatus>(row, "status")?,
    })
}

fn row_to_category(row: &PgRow) -> Result<Category, DbError> {
    let budget_mode = parse_enum_opt::<BudgetMode>(row, "budget_mode")?;
    let budget_type = parse_enum_opt::<BudgetType>(row, "budget_type")?;
    let budget_amount: Option<Decimal> = row.try_get("budget_amount")?;
    let project_start_date: Option<NaiveDate> = row.try_get("project_start_date")?;
    let project_end_date: Option<NaiveDate> = row.try_get("project_end_date")?;

    Ok(Category {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        parent_id: row.try_get("parent_id")?,
        budget: BudgetConfig::from_parts(
            budget_mode,
            budget_type,
            budget_amount,
            project_start_date,
            project_end_date,
        ),
    })
}

/// Column list shared by all transaction SELECT queries.
const TXN_COLUMNS: &str = "id, account_id, category_id, amount, original_amount, original_currency,
                    merchant_name, remittance_information, posted_date,
                    correlation_id, correlation_type, category_method, suggested_category,
                    counterparty_name, counterparty_iban, counterparty_bic, bank_transaction_code,
                    llm_justification, llm_title, skip_correlation,
                    merchant_category_code, bank_transaction_code_code, bank_transaction_code_sub_code,
                    exchange_rate, exchange_rate_unit_currency, exchange_rate_type,
                    exchange_rate_contract_id,
                    reference_number, reference_number_schema, note,
                    balance_after_transaction, balance_after_transaction_currency,
                    creditor_account_additional_id, debtor_account_additional_id";

fn row_to_transaction(row: &PgRow) -> Result<Transaction, DbError> {
    let correlation_id: Option<TransactionId> = row.try_get("correlation_id")?;
    let correlation_type = parse_enum_opt::<CorrelationType>(row, "correlation_type")?;
    let correlation = match (correlation_id, correlation_type) {
        (Some(partner_id), Some(ct)) => Some(Correlation {
            partner_id,
            correlation_type: ct,
        }),
        _ => None,
    };

    let category_id: Option<CategoryId> = row.try_get("category_id")?;
    let category_method = parse_enum_opt::<CategoryMethod>(row, "category_method")?;

    Ok(Transaction {
        id: row.try_get("id")?,
        account_id: row.try_get("account_id")?,
        categorization: Categorization::from_parts(category_id, category_method),
        amount: row.try_get("amount")?,
        original_amount: row.try_get("original_amount")?,
        original_currency: row.try_get("original_currency")?,
        merchant_name: row.try_get("merchant_name")?,
        remittance_information: row.try_get("remittance_information")?,
        posted_date: row.try_get("posted_date")?,
        correlation,
        suggested_category: row.try_get("suggested_category")?,
        counterparty_name: row.try_get("counterparty_name")?,
        counterparty_iban: row.try_get("counterparty_iban")?,
        counterparty_bic: row.try_get("counterparty_bic")?,
        bank_transaction_code: row.try_get("bank_transaction_code")?,
        llm_justification: row.try_get("llm_justification")?,
        llm_title: row.try_get("llm_title")?,
        skip_correlation: row.try_get("skip_correlation")?,
        merchant_category_code: row.try_get("merchant_category_code")?,
        bank_transaction_code_code: row.try_get("bank_transaction_code_code")?,
        bank_transaction_code_sub_code: row.try_get("bank_transaction_code_sub_code")?,
        exchange_rate: row.try_get("exchange_rate")?,
        exchange_rate_unit_currency: row.try_get("exchange_rate_unit_currency")?,
        exchange_rate_type: parse_enum_opt::<ExchangeRateType>(row, "exchange_rate_type")?,
        exchange_rate_contract_id: row.try_get("exchange_rate_contract_id")?,
        reference_number: row.try_get("reference_number")?,
        reference_number_schema: {
            let s: Option<String> = row.try_get("reference_number_schema")?;
            s.map(|v| v.parse::<ReferenceNumberSchema>().expect("infallible"))
        },
        note: row.try_get("note")?,
        balance_after_transaction: row.try_get("balance_after_transaction")?,
        balance_after_transaction_currency: row.try_get("balance_after_transaction_currency")?,
        creditor_account_additional_id: row.try_get("creditor_account_additional_id")?,
        debtor_account_additional_id: row.try_get("debtor_account_additional_id")?,
        amazon_item_titles: Vec::new(),
    })
}

fn row_to_rule(row: &PgRow) -> Result<Rule, DbError> {
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
    /// Returns `DbError` if the connection fails.
    pub async fn connect(url: &str) -> Result<Self, DbError> {
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
    /// Returns `DbError` if any migration fails.
    pub async fn run_migrations(&self) -> Result<(), DbError> {
        let mut migrator = sqlx::migrate!("../../migrations");
        migrator.set_ignore_missing(true);
        migrator.run(&self.0).await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use chrono::NaiveDate;
    use rust_decimal_macros::dec;

    use budget_core::models::{
        Account, AccountId, AccountType, Category, CategoryId, CategoryMethod, CategoryName,
        CorrelationType, CurrencyCode, MatchField, Priority, Rule, RuleCondition, RuleId, RuleType,
        Transaction, TransactionId,
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
            currency: CurrencyCode::new("EUR").unwrap(),
            connection_id: None,
        }
    }

    fn make_category(name: &str) -> Category {
        Category {
            id: CategoryId::new(),
            name: CategoryName::new(name).expect("valid test category name"),
            parent_id: None,
            budget: BudgetConfig::None,
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
            priority: Priority::new(priority).unwrap(),
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
        assert_eq!(fetched.categorization, Categorization::Uncategorized);
        assert!(fetched.correlation.is_none());
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
        assert_eq!(fetched.categorization.category_id(), Some(cat.id));
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
        txn_cat.categorization = Categorization::Manual(cat.id);
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
        txn_uncorr.categorization = Categorization::Manual(cat.id);
        db.upsert_transaction(&txn_uncorr, None).await.unwrap();

        let mut txn_corr = make_transaction(acct.id);
        txn_corr.categorization = Categorization::Manual(cat.id);
        txn_corr.correlation = Some(Correlation {
            partner_id: txn_uncorr.id,
            correlation_type: CorrelationType::Transfer,
        });
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
        assert_eq!(all[0].categorization, Categorization::Manual(cat.id));
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
            assert_eq!(
                fetched.categorization,
                Categorization::from_parts(Some(cat.id), Some(method))
            );
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
        assert_eq!(fetched.categorization, Categorization::Uncategorized);
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
        assert_eq!(fetched[0].categorization, Categorization::Rule(cat.id));
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
        let corr = a.correlation.as_ref().expect("should be correlated");
        assert_eq!(corr.partner_id, txn_b.id);
        assert_eq!(corr.correlation_type, CorrelationType::Transfer);
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
        cat.budget = BudgetConfig::Monthly {
            amount: dec!(1500.00),
            budget_type: BudgetType::Variable,
        };
        db.insert_category(&cat).await.unwrap();

        let fetched = db.get_category(cat.id).await.unwrap().unwrap();
        assert_eq!(fetched.budget.mode(), Some(BudgetMode::Monthly));
        assert_eq!(fetched.budget.amount(), Some(dec!(1500.00)));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_update_category_changes_budget_fields(pool: PgPool) {
        let db = wrap(pool);

        let mut cat = make_category("Utilities");
        cat.budget = BudgetConfig::Monthly {
            amount: dec!(200.00),
            budget_type: BudgetType::Variable,
        };
        db.insert_category(&cat).await.unwrap();

        cat.budget = BudgetConfig::Annual {
            amount: dec!(2400.00),
            budget_type: BudgetType::Variable,
        };
        db.update_category(&cat).await.unwrap();

        let fetched = db.get_category(cat.id).await.unwrap().unwrap();
        assert_eq!(fetched.budget.mode(), Some(BudgetMode::Annual));
        assert_eq!(fetched.budget.amount(), Some(dec!(2400.00)));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_insert_project_category(pool: PgPool) {
        let db = wrap(pool);

        let mut cat = make_category("Kitchen Renovation");
        cat.budget = BudgetConfig::Project {
            amount: dec!(10000.00),
            start_date: NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
            end_date: Some(NaiveDate::from_ymd_opt(2025, 6, 30).unwrap()),
        };
        db.insert_category(&cat).await.unwrap();

        let fetched = db.get_category(cat.id).await.unwrap().unwrap();
        assert_eq!(fetched.budget.mode(), Some(BudgetMode::Project));
        assert_eq!(fetched.budget.amount(), Some(dec!(10000.00)));
        match &fetched.budget {
            BudgetConfig::Project {
                start_date,
                end_date,
                ..
            } => {
                assert_eq!(*start_date, NaiveDate::from_ymd_opt(2025, 1, 1).unwrap());
                assert_eq!(
                    *end_date,
                    Some(NaiveDate::from_ymd_opt(2025, 6, 30).unwrap())
                );
            }
            other => panic!("expected Project, got {other:?}"),
        }
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
        assert_eq!(fetched.priority, Priority::new(10).unwrap());
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
        assert_eq!(cat_rules[0].priority, Priority::new(100).unwrap());
        assert_eq!(cat_rules[1].priority, Priority::new(1).unwrap());

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
        assert_eq!(fetched.priority, Priority::new(5).unwrap());
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
        rule.priority = Priority::new(20).unwrap();

        db.update_rule(&rule).await.unwrap();

        let fetched = db.get_rule(rule.id).await.unwrap().unwrap();
        assert_eq!(fetched.conditions.len(), 1);
        assert_eq!(fetched.conditions[0].field, MatchField::Description);
        assert_eq!(fetched.conditions[0].pattern, "Hotel.*");
        assert_eq!(fetched.target_category_id, Some(cat.id));
        assert_eq!(fetched.priority, Priority::new(20).unwrap());
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
        txn.categorization = Categorization::Manual(cat.id);
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
            txn.categorization = Categorization::Manual(cat.id);
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
            txn.categorization = Categorization::Manual(cat.id);
            txn.merchant_name = "ALDI".into();
            db.upsert_transaction(&txn, None).await.unwrap();
        }

        for _ in 0..4 {
            let mut txn = make_transaction(acct.id);
            txn.categorization = Categorization::Manual(cat.id);
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
        txn_a.categorization = Categorization::Manual(cat.id);
        txn_a.amount = dec!(-500.00);
        db.upsert_transaction(&txn_a, None).await.unwrap();

        let mut txn_b = make_transaction(acct.id);
        txn_b.categorization = Categorization::Manual(cat.id);
        txn_b.amount = dec!(500.00);
        db.upsert_transaction(&txn_b, None).await.unwrap();

        let linked = db
            .link_correlation_pair(txn_a.id, txn_b.id, CorrelationType::Transfer)
            .await
            .unwrap();
        assert!(linked, "first link should succeed");

        let a = db.get_transaction_by_id(txn_a.id).await.unwrap().unwrap();
        let b = db.get_transaction_by_id(txn_b.id).await.unwrap().unwrap();

        let ca = a.correlation.as_ref().expect("a should be correlated");
        let cb = b.correlation.as_ref().expect("b should be correlated");
        assert_eq!(ca.partner_id, txn_b.id);
        assert_eq!(cb.partner_id, txn_a.id);
        assert_eq!(ca.correlation_type, CorrelationType::Transfer);
        assert_eq!(cb.correlation_type, CorrelationType::Transfer);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_link_correlation_pair_rejects_already_linked(pool: PgPool) {
        let db = wrap(pool);
        let acct = make_account();
        db.upsert_account(&acct).await.unwrap();

        let cat = make_category("Transfers");
        db.insert_category(&cat).await.unwrap();

        let mut txn_a = make_transaction(acct.id);
        txn_a.categorization = Categorization::Manual(cat.id);
        txn_a.amount = dec!(-500.00);
        db.upsert_transaction(&txn_a, None).await.unwrap();

        let mut txn_b = make_transaction(acct.id);
        txn_b.categorization = Categorization::Manual(cat.id);
        txn_b.amount = dec!(500.00);
        db.upsert_transaction(&txn_b, None).await.unwrap();

        let mut txn_c = make_transaction(acct.id);
        txn_c.categorization = Categorization::Manual(cat.id);
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
        assert!(c.correlation.is_none());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_skip_correlation_excludes_from_candidates(pool: PgPool) {
        let db = wrap(pool);
        let acct = make_account();
        db.upsert_account(&acct).await.unwrap();

        let cat = make_category("Transfers");
        db.insert_category(&cat).await.unwrap();

        let mut txn_a = make_transaction(acct.id);
        txn_a.categorization = Categorization::Manual(cat.id);
        txn_a.amount = dec!(-100.00);
        db.upsert_transaction(&txn_a, None).await.unwrap();

        let mut txn_b = make_transaction(acct.id);
        txn_b.categorization = Categorization::Manual(cat.id);
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

    /// Verify the TEXT -> UUID migration preserves data and FK integrity.
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
        assert_eq!(
            txns[0].categorization.category_id().unwrap().to_string(),
            cat_id
        );

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
        txn_near.categorization = Categorization::Manual(cat.id);
        txn_near.amount = dec!(100.00);
        txn_near.posted_date = NaiveDate::from_ymd_opt(2025, 6, 20).unwrap();
        db.upsert_transaction(&txn_near, None).await.unwrap();

        // Far candidate: outside 45 days
        let mut txn_far = make_transaction(acct.id);
        txn_far.categorization = Categorization::Manual(cat.id);
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
