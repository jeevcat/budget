use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

// ── Authorization flow ────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct AuthorizationRequest {
    pub access: AccessRequest,
    pub aspsp: AspspRequest,
    pub state: String,
    pub redirect_url: String,
    pub psu_type: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct AccessRequest {
    pub valid_until: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct AspspRequest {
    pub name: String,
    pub country: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AuthorizationResponse {
    pub url: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct SessionCreateRequest {
    pub code: String,
}

#[derive(Debug, Deserialize)]
pub struct SessionResponse {
    pub session_id: String,
    pub accounts: Vec<SessionAccount>,
}

#[derive(Debug, Deserialize)]
pub struct SessionAccount {
    pub uid: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub account_id: Option<AccountIdentification>,
    #[serde(default)]
    pub account_servicer: Option<AccountServicer>,
    #[serde(default)]
    pub product: Option<String>,
    #[serde(default)]
    pub cash_account_type: Option<String>,
    #[serde(default)]
    pub currency: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AccountIdentification {
    #[serde(default)]
    pub iban: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AccountServicer {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub bic_fi: Option<String>,
}

// ── ASPSPs ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AspspsResponse {
    pub aspsps: Vec<AspspEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AspspEntry {
    pub name: String,
    pub country: String,
    #[serde(default)]
    pub logo: Option<String>,
    #[serde(default)]
    pub bic: Option<String>,
    #[serde(default)]
    pub beta: Option<bool>,
    #[serde(default)]
    pub sandbox: Option<serde_json::Value>,
}

// ── Accounts and balances ─────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(crate) struct BalanceResponse {
    pub balances: Vec<Balance>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Balance {
    pub balance_amount: Amount,
    pub balance_type: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Amount {
    pub amount: Decimal,
    pub currency: String,
}

// ── Transactions ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(crate) struct TransactionResponse {
    pub transactions: Vec<ApiTransaction>,
    #[serde(default)]
    pub continuation_key: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ApiTransaction {
    #[serde(default)]
    pub transaction_id: Option<String>,
    #[serde(default)]
    pub entry_reference: Option<String>,
    pub status: String,
    pub credit_debit_indicator: String,
    pub transaction_amount: Amount,
    #[serde(default)]
    pub booking_date: Option<NaiveDate>,
    #[serde(default)]
    pub value_date: Option<NaiveDate>,
    #[serde(default)]
    pub transaction_date: Option<NaiveDate>,
    #[serde(default)]
    pub remittance_information: Vec<String>,
    #[serde(default)]
    pub creditor: Option<PartyIdentification>,
    #[serde(default)]
    pub debtor: Option<PartyIdentification>,
    #[serde(default)]
    pub merchant_category_code: Option<String>,
    #[serde(default)]
    pub exchange_rate: Option<ExchangeRate>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct PartyIdentification {
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ExchangeRate {
    #[serde(default)]
    pub instructed_amount: Option<Amount>,
}

// ── Error response ────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(crate) struct ApiErrorResponse {
    #[serde(default)]
    pub code: Option<serde_json::Value>,
    #[serde(default, alias = "message")]
    pub description: Option<String>,
}

impl ApiErrorResponse {
    pub fn code_string(&self) -> Option<String> {
        self.code.as_ref().map(|v| match v {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        })
    }
}
