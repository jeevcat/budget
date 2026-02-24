use chrono::{NaiveDate, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::bank::{Account, AccountBalance, AccountId, BankProvider, Transaction};
use crate::error::ProviderError;

/// Generate a date relative to today. Positive values mean days ago.
fn days_ago(n: i64) -> NaiveDate {
    Utc::now().date_naive() - chrono::Duration::days(n)
}

const CHECKING_ID: &str = "mock-checking-001";
const CREDIT_CARD_ID: &str = "mock-credit-001";

/// A mock bank provider that returns hardcoded account and transaction data.
///
/// Useful for development and testing without real bank API credentials.
pub struct MockBankProvider;

impl MockBankProvider {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for MockBankProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl BankProvider for MockBankProvider {
    async fn list_accounts(&self) -> Result<Vec<Account>, ProviderError> {
        Ok(vec![
            Account {
                provider_account_id: CHECKING_ID.to_owned(),
                name: "Primary Checking".to_owned(),
                institution: "Mock Bank".to_owned(),
                account_type: "checking".to_owned(),
                currency: "USD".to_owned(),
            },
            Account {
                provider_account_id: CREDIT_CARD_ID.to_owned(),
                name: "Rewards Credit Card".to_owned(),
                institution: "Mock Bank".to_owned(),
                account_type: "credit_card".to_owned(),
                currency: "USD".to_owned(),
            },
        ])
    }

    async fn fetch_transactions(
        &self,
        account_id: &AccountId,
        since: Option<NaiveDate>,
    ) -> Result<Vec<Transaction>, ProviderError> {
        let all = match account_id.as_str() {
            CHECKING_ID => checking_transactions(),
            CREDIT_CARD_ID => credit_card_transactions(),
            _ => {
                return Err(ProviderError::NotFound(format!(
                    "account {}",
                    account_id.as_str()
                )));
            }
        };

        Ok(match since {
            Some(d) => all.into_iter().filter(|t| t.posted_date >= d).collect(),
            None => all,
        })
    }

    async fn get_balances(&self, account_id: &AccountId) -> Result<AccountBalance, ProviderError> {
        match account_id.as_str() {
            CHECKING_ID => Ok(AccountBalance {
                account_id: CHECKING_ID.to_owned(),
                available: dec!(4_250.00),
                current: dec!(4_250.00),
                currency: "USD".to_owned(),
            }),
            CREDIT_CARD_ID => Ok(AccountBalance {
                account_id: CREDIT_CARD_ID.to_owned(),
                available: dec!(8_500.00),
                current: dec!(-1_500.00),
                currency: "USD".to_owned(),
            }),
            _ => Err(ProviderError::NotFound(format!(
                "account {}",
                account_id.as_str()
            ))),
        }
    }
}

fn txn(id: &str, amount: Decimal, merchant: &str, desc: Option<&str>, ago: i64) -> Transaction {
    Transaction {
        provider_transaction_id: id.to_owned(),
        amount,
        currency: "USD".to_owned(),
        merchant_name: merchant.to_owned(),
        description: desc.map(ToOwned::to_owned),
        posted_date: days_ago(ago),
        counterparty_name: None,
        merchant_category_code: None,
        original_amount: None,
        original_currency: None,
    }
}

fn checking_transactions() -> Vec<Transaction> {
    vec![
        // Salary deposits
        txn(
            "chk-001",
            dec!(3_500.00),
            "ACME CORP PAYROLL",
            Some("Salary deposit"),
            30,
        ),
        txn(
            "chk-002",
            dec!(3_500.00),
            "ACME CORP PAYROLL",
            Some("Salary deposit"),
            1,
        ),
        // Regular expenses
        txn(
            "chk-003",
            dec!(-85.50),
            "CITY WATER UTILITY",
            Some("Monthly water bill"),
            35,
        ),
        txn(
            "chk-004",
            dec!(-120.00),
            "STATE FARM INSURANCE",
            Some("Auto insurance premium"),
            33,
        ),
        txn(
            "chk-005",
            dec!(-1_200.00),
            "PARKVIEW APARTMENTS",
            Some("Rent payment"),
            40,
        ),
        // Transfer to credit card (matches a credit card payment)
        txn(
            "chk-006",
            dec!(-1_500.00),
            "CHASE CREDIT CRD AUTOPAY",
            Some("Credit card payment"),
            18,
        ),
        // ATM withdrawal
        txn("chk-007", dec!(-60.00), "ATM WITHDRAWAL", None, 22),
        // Miscellaneous
        txn(
            "chk-008",
            dec!(-45.99),
            "AMAZON.COM",
            Some("Household supplies"),
            15,
        ),
        txn(
            "chk-009",
            dec!(250.00),
            "VENMO PAYMENT",
            Some("Reimbursement from friend"),
            10,
        ),
    ]
}

fn credit_card_transactions() -> Vec<Transaction> {
    vec![
        // Groceries
        txn(
            "cc-001",
            dec!(-72.34),
            "WHOLE FOODS MARKET",
            Some("Weekly groceries"),
            38,
        ),
        txn("cc-002", dec!(-58.12), "TRADER JOES", Some("Groceries"), 28),
        txn(
            "cc-003",
            dec!(-134.56),
            "COSTCO WHOLESALE",
            Some("Bulk groceries"),
            20,
        ),
        // Dining
        txn("cc-004", dec!(-42.50), "CHIPOTLE MEXICAN GRILL", None, 36),
        txn(
            "cc-005",
            dec!(-87.25),
            "THE CHEESECAKE FACTORY",
            Some("Dinner out"),
            26,
        ),
        // Gas
        txn("cc-006", dec!(-52.10), "SHELL OIL", Some("Gas"), 32),
        // Subscriptions
        txn(
            "cc-007",
            dec!(-15.99),
            "NETFLIX.COM",
            Some("Monthly subscription"),
            42,
        ),
        txn(
            "cc-008",
            dec!(-9.99),
            "SPOTIFY USA",
            Some("Monthly subscription"),
            42,
        ),
        // Shopping
        txn(
            "cc-009",
            dec!(-199.99),
            "AMAZON.COM",
            Some("Electronics purchase"),
            24,
        ),
        txn("cc-010", dec!(-65.00), "TARGET", Some("Clothing"), 16),
        // Credit card payment received (matches checking transfer)
        txn(
            "cc-011",
            dec!(1_500.00),
            "PAYMENT RECEIVED",
            Some("Thank you for your payment"),
            18,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn list_accounts_returns_two_accounts() {
        let provider = MockBankProvider::new();
        let accounts = provider.list_accounts().await.unwrap();
        assert_eq!(accounts.len(), 2);
        assert_eq!(accounts[0].account_type, "checking");
        assert_eq!(accounts[1].account_type, "credit_card");
    }

    #[tokio::test]
    async fn fetch_checking_transactions_returns_all_when_since_is_old() {
        let provider = MockBankProvider::new();
        let account_id = AccountId(CHECKING_ID.to_owned());
        let since = NaiveDate::from_ymd_opt(2020, 1, 1);
        let txns = provider
            .fetch_transactions(&account_id, since)
            .await
            .unwrap();
        assert_eq!(txns.len(), 9);
    }

    #[tokio::test]
    async fn fetch_credit_card_transactions_returns_all_when_since_is_old() {
        let provider = MockBankProvider::new();
        let account_id = AccountId(CREDIT_CARD_ID.to_owned());
        let since = NaiveDate::from_ymd_opt(2020, 1, 1);
        let txns = provider
            .fetch_transactions(&account_id, since)
            .await
            .unwrap();
        assert_eq!(txns.len(), 11);
    }

    #[tokio::test]
    async fn fetch_transactions_filters_by_date() {
        let provider = MockBankProvider::new();
        let account_id = AccountId(CHECKING_ID.to_owned());
        // Only include transactions from the last 12 days.
        // Should include: chk-009 (10 days ago) and chk-002 (1 day ago)
        let since = Some(days_ago(12));
        let txns = provider
            .fetch_transactions(&account_id, since)
            .await
            .unwrap();
        assert_eq!(txns.len(), 2);
    }

    #[tokio::test]
    async fn fetch_transactions_unknown_account_returns_not_found() {
        let provider = MockBankProvider::new();
        let account_id = AccountId("nonexistent".to_owned());
        let since = NaiveDate::from_ymd_opt(2020, 1, 1);
        let result = provider.fetch_transactions(&account_id, since).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn get_checking_balance() {
        let provider = MockBankProvider::new();
        let account_id = AccountId(CHECKING_ID.to_owned());
        let balance = provider.get_balances(&account_id).await.unwrap();
        assert_eq!(balance.current, dec!(4_250.00));
        assert_eq!(balance.currency, "USD");
    }

    #[tokio::test]
    async fn get_credit_card_balance() {
        let provider = MockBankProvider::new();
        let account_id = AccountId(CREDIT_CARD_ID.to_owned());
        let balance = provider.get_balances(&account_id).await.unwrap();
        assert_eq!(balance.current, dec!(-1_500.00));
        assert_eq!(balance.available, dec!(8_500.00));
    }

    #[tokio::test]
    async fn get_balance_unknown_account_returns_not_found() {
        let provider = MockBankProvider::new();
        let account_id = AccountId("nonexistent".to_owned());
        let result = provider.get_balances(&account_id).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn checking_has_salary_deposits() {
        let provider = MockBankProvider::new();
        let account_id = AccountId(CHECKING_ID.to_owned());
        let since = NaiveDate::from_ymd_opt(2020, 1, 1);
        let txns = provider
            .fetch_transactions(&account_id, since)
            .await
            .unwrap();
        let salary_txns: Vec<_> = txns
            .iter()
            .filter(|t| t.merchant_name.contains("PAYROLL"))
            .collect();
        assert_eq!(salary_txns.len(), 2);
        assert!(salary_txns.iter().all(|t| t.amount > Decimal::ZERO));
    }

    #[tokio::test]
    async fn transfer_pair_exists_between_accounts() {
        let provider = MockBankProvider::new();
        let since = NaiveDate::from_ymd_opt(2020, 1, 1);

        let checking_txns = provider
            .fetch_transactions(&AccountId(CHECKING_ID.to_owned()), since)
            .await
            .unwrap();
        let cc_txns = provider
            .fetch_transactions(&AccountId(CREDIT_CARD_ID.to_owned()), since)
            .await
            .unwrap();

        // Checking has a -1500 payment, credit card has a +1500 payment received
        let outgoing = checking_txns
            .iter()
            .find(|t| t.provider_transaction_id == "chk-006");
        let incoming = cc_txns
            .iter()
            .find(|t| t.provider_transaction_id == "cc-011");

        assert!(outgoing.is_some());
        assert!(incoming.is_some());
        assert_eq!(outgoing.unwrap().amount, dec!(-1_500.00));
        assert_eq!(incoming.unwrap().amount, dec!(1_500.00));
    }
}
