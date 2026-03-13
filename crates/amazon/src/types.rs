use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Domain types owned by this crate
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AmazonTransactionStatus {
    Charged,
    Refunded,
    Declined,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmazonTransaction {
    pub date: NaiveDate,
    pub amount: Decimal,
    pub currency: String,
    pub statement_descriptor: String,
    pub status: AmazonTransactionStatus,
    pub payment_method: String,
    pub order_ids: Vec<String>,
    pub dedup_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmazonOrder {
    pub order_id: String,
    pub order_date: Option<NaiveDate>,
    pub grand_total: Option<Decimal>,
    pub subtotal: Option<Decimal>,
    pub shipping: Option<Decimal>,
    pub vat: Option<Decimal>,
    pub promotion: Option<Decimal>,
    pub items: Vec<AmazonItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmazonItem {
    pub title: String,
    pub asin: Option<String>,
    pub price: Option<Decimal>,
    pub quantity: u32,
    pub seller: Option<String>,
    pub image_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmazonCookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub expires: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct TransactionsPageData {
    pub token: String,
    pub transactions: Vec<AmazonTransaction>,
    pub has_more: bool,
    pub page_key: Option<String>,
}

// ---------------------------------------------------------------------------
// Matching types (pure, no DB dependency)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct BankCandidate {
    pub id: Uuid,
    pub amount: Decimal,
    pub posted_date: NaiveDate,
    pub merchant_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MatchConfidence {
    Exact,
    Approximate,
}

#[derive(Debug, Clone)]
pub struct MatchResult {
    pub amazon_dedup_key: String,
    pub bank_transaction_id: Uuid,
    pub confidence: MatchConfidence,
}

// ---------------------------------------------------------------------------
// Raw serde types for Amazon's JSON responses
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct NextData {
    pub props: NextDataProps,
}

#[derive(Debug, Deserialize)]
pub struct NextDataProps {
    #[serde(rename = "pageProps")]
    pub page_props: PageProps,
}

#[derive(Debug, Deserialize)]
pub struct PageProps {
    pub token: String,
    pub state: PageState,
}

#[derive(Debug, Deserialize)]
pub struct PageState {
    #[serde(rename = "transactionResponseState")]
    pub transaction_response_state: TransactionResponseState,
}

#[derive(Debug, Deserialize)]
pub struct TransactionResponseState {
    #[serde(rename = "visibleTransactionResponse")]
    pub visible_transaction_response: VisibleTransactionResponse,
    #[serde(rename = "hasMore", default)]
    pub has_more: bool,
    #[serde(rename = "lastEvaluatedPageKey")]
    pub last_evaluated_page_key: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct VisibleTransactionResponse {
    #[serde(rename = "transactionList", default)]
    pub transaction_list: Vec<RawTransaction>,
}

#[derive(Debug, Deserialize)]
pub struct RawTransaction {
    #[serde(rename = "formattedAmount")]
    pub formatted_amount: String,
    #[serde(rename = "formattedDate")]
    pub formatted_date: String,
    #[serde(rename = "orderData")]
    pub order_data: Option<Vec<OrderRef>>,
    #[serde(rename = "statementDescriptor")]
    pub statement_descriptor: Option<String>,
    #[serde(rename = "statusInfo")]
    pub status_info: Option<StatusInfo>,
    #[serde(rename = "paymentMethodDisplayStringData")]
    pub payment_method_data: Option<PaymentMethodData>,
}

#[derive(Debug, Deserialize)]
pub struct OrderRef {
    #[serde(rename = "orderDisplayString")]
    pub order_display_string: Option<String>,
    #[serde(rename = "orderDetailsUrl")]
    pub order_details_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct StatusInfo {
    pub status: Option<String>,
    pub label: Option<String>,
    #[serde(rename = "amountDetailsStatusType")]
    pub amount_details_status_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PaymentMethodData {
    #[serde(rename = "paymentMethodName")]
    pub payment_method_name: Option<String>,
    #[serde(rename = "paymentMethodNumber")]
    pub payment_method_number: Option<PaymentMethodNumber>,
}

#[derive(Debug, Deserialize)]
pub struct PaymentMethodNumber {
    pub prefix: Option<String>,
    #[serde(rename = "lastDigits")]
    pub last_digits: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PaginationResponse {
    #[serde(rename = "type")]
    pub response_type: Option<String>,
    #[serde(rename = "responseCode")]
    pub response_code: Option<String>,
    #[serde(rename = "displayResponse")]
    pub display_response: Option<PaginationDisplayResponse>,
}

#[derive(Debug, Deserialize)]
pub struct PaginationDisplayResponse {
    #[serde(rename = "transactionsList")]
    pub transactions_list: Option<Vec<RawTransaction>>,
    #[serde(rename = "lastEvaluatedPageKey")]
    pub last_evaluated_page_key: Option<String>,
}
