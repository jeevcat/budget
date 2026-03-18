use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// A transaction fetched from the `PayPal` Transaction Search API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PayPalTransaction {
    /// `PayPal`'s unique transaction identifier.
    pub transaction_id: String,
    pub transaction_date: NaiveDate,
    pub amount: Decimal,
    pub currency: String,
    /// The actual merchant/seller name (the primary enrichment value).
    pub merchant_name: Option<String>,
    /// `PayPal` event code (e.g. "T0006" = web payment).
    pub event_code: Option<String>,
    /// "S" = Success, "P" = Pending, "V" = Reversed, etc.
    pub status: String,
    pub items: Vec<PayPalItem>,
    pub payer_email: Option<String>,
    pub payer_name: Option<String>,
}

/// An item from a `PayPal` transaction's cart.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PayPalItem {
    pub name: Option<String>,
    pub description: Option<String>,
    pub quantity: Option<String>,
    #[cfg_attr(feature = "openapi", schema(value_type = Option<String>))]
    pub unit_price: Option<Decimal>,
    pub unit_price_currency: Option<String>,
}

/// A bank transaction eligible for `PayPal` matching.
#[derive(Debug, Clone)]
pub struct BankCandidate {
    pub id: uuid::Uuid,
    pub amount: Decimal,
    pub posted_date: NaiveDate,
    pub merchant_name: String,
    pub remittance_information: Vec<String>,
}

/// A successful match between a `PayPal` transaction and a bank transaction.
#[derive(Debug, Clone)]
pub struct MatchResult {
    pub paypal_transaction_id: String,
    pub bank_transaction_id: uuid::Uuid,
}
