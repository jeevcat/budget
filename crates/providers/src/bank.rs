use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::error::ProviderError;

/// A bank account as reported by the provider, before mapping to domain types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub provider_account_id: String,
    pub name: String,
    pub institution: String,
    pub account_type: String,
    pub currency: String,
}

/// A transaction as reported by the provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub provider_transaction_id: String,
    pub amount: Decimal,
    pub currency: String,
    pub merchant_name: String,
    pub description: Option<String>,
    pub posted_date: NaiveDate,
    pub counterparty_name: Option<String>,
    pub merchant_category_code: Option<String>,
    pub original_amount: Option<Decimal>,
    pub original_currency: Option<String>,
}

/// Account balance as reported by the provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountBalance {
    pub account_id: String,
    pub available: Decimal,
    pub current: Decimal,
    pub currency: String,
}

/// Identifier for a provider-level account.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct AccountId(pub String);

impl AccountId {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[trait_variant::make(Send)]
pub trait BankProvider {
    async fn list_accounts(&self) -> Result<Vec<Account>, ProviderError>;

    async fn fetch_transactions(
        &self,
        account_id: &AccountId,
        since: NaiveDate,
    ) -> Result<Vec<Transaction>, ProviderError>;

    async fn get_balances(&self, account_id: &AccountId) -> Result<AccountBalance, ProviderError>;
}
