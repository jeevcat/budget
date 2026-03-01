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
    /// Array of free-text payment detail lines from the bank.
    /// Source: Enable Banking `remittance_information`
    pub remittance_information: Vec<String>,
    pub posted_date: NaiveDate,
    pub counterparty_name: Option<String>,
    pub counterparty_iban: Option<String>,
    pub counterparty_bic: Option<String>,
    /// Human-readable bank transaction label (e.g. "Gehalt/Rente").
    /// Source: Enable Banking `bank_transaction_code.description`
    pub bank_transaction_code: Option<String>,
    /// ISO 18245 MCC code (e.g. "5411" = grocery). Only present for card transactions.
    /// Source: Enable Banking `merchant_category_code`
    pub merchant_category_code: Option<String>,
    pub original_amount: Option<Decimal>,
    pub original_currency: Option<String>,
    /// ISO 20022 domain code (e.g. "PMNT" for payments).
    /// Source: Enable Banking `bank_transaction_code.code`
    pub bank_transaction_code_code: Option<String>,
    /// ISO 20022 sub-family code (e.g. "ICDT-STDO").
    /// Source: Enable Banking `bank_transaction_code.sub_code`
    pub bank_transaction_code_sub_code: Option<String>,
    /// Actual FX rate applied (e.g. "1.0856"), stored as string to preserve bank precision.
    /// Source: Enable Banking `exchange_rate.exchange_rate`
    pub exchange_rate: Option<String>,
    /// ISO 4217 currency code in which the exchange rate is expressed.
    /// Source: Enable Banking `exchange_rate.unit_currency`
    pub exchange_rate_unit_currency: Option<String>,
    /// FX rate type: AGRD (agreed/contract), SALE, or SPOT.
    /// Source: Enable Banking `exchange_rate.rate_type`
    pub exchange_rate_type: Option<String>,
    /// FX contract reference when `rate_type` is AGRD (agreed).
    /// Source: Enable Banking `exchange_rate.contract_identification`
    pub exchange_rate_contract_id: Option<String>,
    /// Structured payment reference (e.g. "RF07850352502356628678117").
    /// Source: Enable Banking `reference_number`
    pub reference_number: Option<String>,
    /// Scheme of the reference number: BERF, FIRF, INTL, NORF, SDDM, SEBG.
    /// Source: Enable Banking `reference_number_schema`
    pub reference_number_schema: Option<String>,
    /// Internal note made by PSU (Payment Service User), distinct from remittance info.
    /// Source: Enable Banking `note`
    pub note: Option<String>,
    /// Account balance after this transaction (amount component).
    /// Source: Enable Banking `balance_after_transaction.amount`
    pub balance_after_transaction: Option<Decimal>,
    /// Currency of the balance after transaction (usually same as account currency).
    /// Source: Enable Banking `balance_after_transaction.currency`
    pub balance_after_transaction_currency: Option<String>,
    /// Non-IBAN creditor account IDs as JSONB array of `{identification, scheme_name, issuer}`.
    /// Source: Enable Banking `creditor_account_additional_identification`
    pub creditor_account_additional_id: Option<serde_json::Value>,
    /// Non-IBAN debtor account IDs as JSONB array of `{identification, scheme_name, issuer}`.
    /// Source: Enable Banking `debtor_account_additional_identification`
    pub debtor_account_additional_id: Option<serde_json::Value>,
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
        since: Option<NaiveDate>,
    ) -> Result<Vec<Transaction>, ProviderError>;

    async fn get_balances(&self, account_id: &AccountId) -> Result<AccountBalance, ProviderError>;
}
