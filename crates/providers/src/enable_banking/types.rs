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
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
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
    #[cfg_attr(feature = "openapi", schema(value_type = Option<Object>))]
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
    #[serde(default)]
    pub creditor_account: Option<AccountIdentificationTxn>,
    #[serde(default)]
    pub debtor_account: Option<AccountIdentificationTxn>,
    #[serde(default)]
    pub creditor_agent: Option<AgentIdentification>,
    #[serde(default)]
    pub debtor_agent: Option<AgentIdentification>,
    #[serde(default)]
    pub bank_transaction_code: Option<BankTransactionCode>,
    /// Account balance after this transaction. API returns `AmountType` (amount + currency).
    /// Source: Enable Banking `balance_after_transaction`
    #[serde(default)]
    pub balance_after_transaction: Option<Amount>,
    /// Structured payment reference (e.g. "RF07850352502356628678117").
    /// Source: Enable Banking `reference_number`
    #[serde(default)]
    pub reference_number: Option<String>,
    /// Scheme of the reference number. JSON key is `reference_number_schema` (not "scheme").
    /// Values: BERF (Belgian), FIRF (Finnish), INTL (ISO 11649/RF), NORF (Norwegian KID),
    /// SDDM (SEPA DD mandate), SEBG (Swedish Bankgiro OCR).
    /// Source: Enable Banking `reference_number_schema`
    #[serde(default)]
    pub reference_number_schema: Option<String>,
    /// Internal note made by PSU (Payment Service User), distinct from remittance info.
    /// Source: Enable Banking `note`
    #[serde(default)]
    pub note: Option<String>,
    /// Non-IBAN debtor account identifications (BBAN, card PAN, proprietary).
    /// Source: Enable Banking `debtor_account_additional_identification`
    #[serde(default)]
    pub debtor_account_additional_identification: Option<Vec<GenericIdentification>>,
    /// Non-IBAN creditor account identifications (BBAN, card PAN, proprietary).
    /// Source: Enable Banking `creditor_account_additional_identification`
    #[serde(default)]
    pub creditor_account_additional_identification: Option<Vec<GenericIdentification>>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct PartyIdentification {
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct AccountIdentificationTxn {
    #[serde(default)]
    pub iban: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct AgentIdentification {
    #[serde(default)]
    pub bic_fi: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct BankTransactionCode {
    #[serde(default)]
    pub description: Option<String>,
    /// ISO 20022 domain code (e.g. "PMNT" for payments).
    /// Source: Enable Banking `bank_transaction_code.code`
    #[serde(default)]
    pub code: Option<String>,
    /// ISO 20022 sub-family code (e.g. "ICDT-STDO" for standing order credit transfer).
    /// Source: Enable Banking `bank_transaction_code.sub_code`
    #[serde(default)]
    pub sub_code: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(clippy::struct_field_names)] // mirrors Enable Banking API's nested exchange_rate.exchange_rate
pub(crate) struct ExchangeRate {
    #[serde(default)]
    pub instructed_amount: Option<Amount>,
    /// ISO 4217 currency code of the currency in which the rate is expressed.
    /// Source: Enable Banking `exchange_rate.unit_currency`
    #[serde(default)]
    pub unit_currency: Option<String>,
    /// The actual FX rate applied (e.g. "1.0856"). Stored as string to preserve bank precision.
    /// JSON key is `exchange_rate.exchange_rate` (yes, nested same name as parent).
    /// Source: Enable Banking `exchange_rate.exchange_rate`
    #[serde(default)]
    pub exchange_rate: Option<String>,
    /// Rate type: AGRD (agreed/contract), SALE, or SPOT.
    /// Source: Enable Banking `exchange_rate.rate_type`
    #[serde(default)]
    pub rate_type: Option<String>,
    /// FX contract reference when `rate_type` is AGRD.
    /// Source: Enable Banking `exchange_rate.contract_identification`
    #[serde(default)]
    pub contract_identification: Option<String>,
}

/// Non-IBAN account identification (BBAN, card PAN, proprietary).
/// Source: Enable Banking `GenericIdentification` in `creditor/debtor_account_additional_identification`
#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct GenericIdentification {
    #[serde(default)]
    pub identification: Option<String>,
    /// Scheme: BBAN, CPAN, etc.
    #[serde(default)]
    pub scheme_name: Option<String>,
    #[serde(default)]
    pub issuer: Option<String>,
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
