use chrono::{DateTime, Duration, NaiveDate, Utc};
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use rust_decimal::Decimal;
use serde::Deserialize;
use tracing::{debug, info};

use crate::error::PayPalError;
use crate::types::{PayPalItem, PayPalTransaction};

const SANDBOX_BASE: &str = "https://api-m.sandbox.paypal.com";
const LIVE_BASE: &str = "https://api-m.paypal.com";

/// Maximum date range per API call (`PayPal` enforces 31 days).
const MAX_WINDOW_DAYS: i64 = 31;

struct AccessToken {
    token: String,
    expires_at: DateTime<Utc>,
}

/// HTTP client for the `PayPal` Transaction Search API.
///
/// Handles `OAuth2` client credentials flow and automatic token refresh.
pub struct PayPalClient {
    client: reqwest::Client,
    client_id: String,
    client_secret: String,
    base_url: String,
    access_token: Option<AccessToken>,
}

impl PayPalClient {
    /// Create a client for the live `PayPal` API.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be constructed.
    pub fn new(client_id: String, client_secret: String) -> Result<Self, PayPalError> {
        Self::with_base_url(client_id, client_secret, LIVE_BASE.to_owned())
    }

    /// Create a client for the `PayPal` sandbox API.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be constructed.
    pub fn sandbox(client_id: String, client_secret: String) -> Result<Self, PayPalError> {
        Self::with_base_url(client_id, client_secret, SANDBOX_BASE.to_owned())
    }

    fn with_base_url(
        client_id: String,
        client_secret: String,
        base_url: String,
    ) -> Result<Self, PayPalError> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;

        Ok(Self {
            client,
            client_id,
            client_secret,
            base_url,
            access_token: None,
        })
    }

    /// Obtain or refresh the `OAuth2` bearer token.
    ///
    /// # Errors
    ///
    /// Returns an error if the token request fails or credentials are invalid.
    pub(crate) async fn ensure_token(&mut self) -> Result<&str, PayPalError> {
        let needs_refresh = match &self.access_token {
            Some(token) => token.expires_at <= Utc::now() + Duration::seconds(30),
            None => true,
        };

        if needs_refresh {
            self.refresh_token().await?;
        }

        Ok(&self
            .access_token
            .as_ref()
            .expect("token was just set")
            .token)
    }

    async fn refresh_token(&mut self) -> Result<(), PayPalError> {
        debug!("requesting new PayPal OAuth token");

        let resp = self
            .client
            .post(format!("{}/v1/oauth2/token", self.base_url))
            .basic_auth(&self.client_id, Some(&self.client_secret))
            .header(ACCEPT, "application/json")
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body("grant_type=client_credentials")
            .send()
            .await?;

        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(PayPalError::InvalidCredentials);
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(PayPalError::AuthFailed {
                status: status.as_u16(),
                body,
            });
        }

        let token_resp: TokenResponse = resp.json().await?;
        let expires_in = token_resp.expires_in.unwrap_or(3600);
        let expires_at = Utc::now() + Duration::seconds(i64::from(expires_in));

        info!("obtained PayPal OAuth token, expires in {expires_in}s");

        self.access_token = Some(AccessToken {
            token: token_resp.access_token,
            expires_at,
        });

        Ok(())
    }

    /// Search transactions for a single date window (max 31 days).
    ///
    /// Handles pagination within the window automatically.
    ///
    /// # Errors
    ///
    /// Returns an error if the API call fails or the response cannot be parsed.
    pub async fn search_transactions(
        &mut self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<PayPalTransaction>, PayPalError> {
        let mut all_txns = Vec::new();
        let mut page = 1;

        loop {
            let token = self.ensure_token().await?.to_owned();

            let resp = self
                .client
                .get(format!("{}/v1/reporting/transactions", self.base_url))
                .header(AUTHORIZATION, format!("Bearer {token}"))
                .header(ACCEPT, "application/json")
                .query(&[
                    ("start_date", start.to_rfc3339()),
                    ("end_date", end.to_rfc3339()),
                    ("fields", "all".to_owned()),
                    ("page_size", "500".to_owned()),
                    ("page", page.to_string()),
                ])
                .send()
                .await?;

            let status = resp.status();
            if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                return Err(PayPalError::RateLimited);
            }
            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                return Err(PayPalError::Api {
                    status: status.as_u16(),
                    body,
                });
            }

            let body: TransactionSearchResponse = resp.json().await?;
            let page_txns = parse_transaction_details(&body.transaction_details);
            let total_pages = body.total_pages.unwrap_or(1);

            debug!(
                page,
                total_pages,
                count = page_txns.len(),
                "fetched PayPal transaction page"
            );

            all_txns.extend(page_txns);

            if page >= total_pages {
                break;
            }
            page += 1;
        }

        Ok(all_txns)
    }

    /// Fetch all transactions from `since` to now, automatically chunking
    /// into 31-day windows.
    ///
    /// # Errors
    ///
    /// Returns an error if any API call fails.
    pub async fn fetch_all_transactions(
        &mut self,
        since: DateTime<Utc>,
    ) -> Result<Vec<PayPalTransaction>, PayPalError> {
        let now = Utc::now();
        let mut all_txns = Vec::new();
        let mut window_start = since;

        while window_start < now {
            let window_end = (window_start + Duration::days(MAX_WINDOW_DAYS)).min(now);

            info!(
                start = %window_start.format("%Y-%m-%d"),
                end = %window_end.format("%Y-%m-%d"),
                "fetching PayPal transactions window"
            );

            let txns = self.search_transactions(window_start, window_end).await?;
            all_txns.extend(txns);

            window_start = window_end;
        }

        info!(total = all_txns.len(), "fetched all PayPal transactions");
        Ok(all_txns)
    }
}

// ---------------------------------------------------------------------------
// API response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct TransactionSearchResponse {
    #[serde(default)]
    transaction_details: Vec<TransactionDetail>,
    total_pages: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[allow(clippy::struct_field_names)] // field names match PayPal API JSON
struct TransactionDetail {
    transaction_info: Option<TransactionInfo>,
    payer_info: Option<PayerInfo>,
    cart_info: Option<CartInfo>,
}

#[derive(Debug, Deserialize)]
#[allow(clippy::struct_field_names)] // field names match PayPal API JSON
struct TransactionInfo {
    transaction_id: Option<String>,
    transaction_event_code: Option<String>,
    transaction_initiation_date: Option<String>,
    transaction_amount: Option<MoneyAmount>,
    transaction_status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MoneyAmount {
    value: Option<String>,
    currency_code: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PayerInfo {
    email_address: Option<String>,
    payer_name: Option<PayerName>,
}

#[derive(Debug, Deserialize)]
struct PayerName {
    given_name: Option<String>,
    surname: Option<String>,
    alternate_full_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CartInfo {
    #[serde(default)]
    item_details: Vec<CartItemDetail>,
}

#[derive(Debug, Deserialize)]
#[allow(clippy::struct_field_names)] // field names match PayPal API JSON
struct CartItemDetail {
    item_name: Option<String>,
    item_description: Option<String>,
    item_quantity: Option<String>,
    item_unit_price: Option<MoneyAmount>,
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

fn parse_transaction_details(details: &[TransactionDetail]) -> Vec<PayPalTransaction> {
    let mut txns = Vec::new();

    for detail in details {
        let Some(ref info) = detail.transaction_info else {
            continue;
        };
        let Some(ref txn_id) = info.transaction_id else {
            continue;
        };

        let (amount, currency) = match &info.transaction_amount {
            Some(ma) => {
                let amt = ma
                    .value
                    .as_deref()
                    .and_then(|v| v.parse::<Decimal>().ok())
                    .unwrap_or_default();
                let cur = ma.currency_code.clone().unwrap_or_else(|| "EUR".to_owned());
                (amt, cur)
            }
            None => (Decimal::ZERO, "EUR".to_owned()),
        };

        let date = info
            .transaction_initiation_date
            .as_deref()
            .and_then(parse_paypal_date)
            .unwrap_or_else(|| NaiveDate::from_ymd_opt(2000, 1, 1).expect("valid date"));

        let payer_email = detail
            .payer_info
            .as_ref()
            .and_then(|p| p.email_address.clone());

        let payer_name = detail.payer_info.as_ref().and_then(|p| {
            p.payer_name.as_ref().and_then(|n| {
                n.alternate_full_name
                    .clone()
                    .or_else(|| match (&n.given_name, &n.surname) {
                        (Some(g), Some(s)) => Some(format!("{g} {s}")),
                        (Some(g), None) => Some(g.clone()),
                        (None, Some(s)) => Some(s.clone()),
                        (None, None) => None,
                    })
            })
        });

        // The merchant/seller name comes from the payer name in PayPal's model
        // (from the buyer's perspective, the "payer" in a sale is the merchant).
        // For outgoing payments, payer_info.payer_name is the recipient.
        let merchant_name = payer_name.clone();

        let items: Vec<PayPalItem> = detail
            .cart_info
            .as_ref()
            .map(|ci| {
                ci.item_details
                    .iter()
                    .map(|item| PayPalItem {
                        name: item.item_name.clone(),
                        description: item.item_description.clone(),
                        quantity: item.item_quantity.clone(),
                        unit_price: item
                            .item_unit_price
                            .as_ref()
                            .and_then(|p| p.value.as_deref())
                            .and_then(|v| v.parse().ok()),
                        unit_price_currency: item
                            .item_unit_price
                            .as_ref()
                            .and_then(|p| p.currency_code.clone()),
                    })
                    .collect()
            })
            .unwrap_or_default();

        txns.push(PayPalTransaction {
            transaction_id: txn_id.clone(),
            transaction_date: date,
            amount,
            currency,
            merchant_name,
            event_code: info.transaction_event_code.clone(),
            status: info
                .transaction_status
                .clone()
                .unwrap_or_else(|| "S".to_owned()),
            items,
            payer_email,
            payer_name,
        });
    }

    txns
}

/// Parse `PayPal`'s date format (ISO 8601 with timezone offset).
fn parse_paypal_date(s: &str) -> Option<NaiveDate> {
    // PayPal uses "2024-01-15T10:30:00+0000" or RFC 3339
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.date_naive())
        .or_else(|| {
            // Fallback: "2024-01-15T10:30:00+0000" (no colon in offset)
            DateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%z")
                .ok()
                .map(|dt| dt.date_naive())
        })
        .or_else(|| {
            // Just the date
            s.get(..10)
                .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rfc3339_date() {
        let d = parse_paypal_date("2024-01-15T10:30:00+0000");
        assert_eq!(d, NaiveDate::from_ymd_opt(2024, 1, 15));
    }

    #[test]
    fn parse_rfc3339_with_colon() {
        let d = parse_paypal_date("2024-01-15T10:30:00+00:00");
        assert_eq!(d, NaiveDate::from_ymd_opt(2024, 1, 15));
    }

    #[test]
    fn parse_date_only() {
        let d = parse_paypal_date("2024-01-15");
        assert_eq!(d, NaiveDate::from_ymd_opt(2024, 1, 15));
    }

    #[test]
    fn parse_empty_returns_none() {
        assert!(parse_paypal_date("").is_none());
    }

    #[test]
    fn parse_transaction_detail_minimal() {
        let detail = TransactionDetail {
            transaction_info: Some(TransactionInfo {
                transaction_id: Some("TXN123".to_owned()),
                transaction_event_code: Some("T0006".to_owned()),
                transaction_initiation_date: Some("2024-06-01T12:00:00+00:00".to_owned()),
                transaction_amount: Some(MoneyAmount {
                    value: Some("-12.99".to_owned()),
                    currency_code: Some("EUR".to_owned()),
                }),
                transaction_status: Some("S".to_owned()),
            }),
            payer_info: None,
            cart_info: None,
        };

        let txns = parse_transaction_details(&[detail]);
        assert_eq!(txns.len(), 1);
        assert_eq!(txns[0].transaction_id, "TXN123");
        assert_eq!(txns[0].amount, Decimal::new(-1299, 2));
        assert_eq!(txns[0].currency, "EUR");
        assert!(txns[0].merchant_name.is_none());
        assert!(txns[0].items.is_empty());
    }

    #[test]
    fn parse_transaction_detail_with_cart() {
        let detail = TransactionDetail {
            transaction_info: Some(TransactionInfo {
                transaction_id: Some("TXN456".to_owned()),
                transaction_event_code: None,
                transaction_initiation_date: Some("2024-06-01T12:00:00+00:00".to_owned()),
                transaction_amount: Some(MoneyAmount {
                    value: Some("-25.00".to_owned()),
                    currency_code: Some("EUR".to_owned()),
                }),
                transaction_status: None,
            }),
            payer_info: Some(PayerInfo {
                email_address: Some("seller@example.com".to_owned()),
                payer_name: Some(PayerName {
                    given_name: Some("Shop".to_owned()),
                    surname: Some("Owner".to_owned()),
                    alternate_full_name: None,
                }),
            }),
            cart_info: Some(CartInfo {
                item_details: vec![CartItemDetail {
                    item_name: Some("Widget".to_owned()),
                    item_description: Some("A nice widget".to_owned()),
                    item_quantity: Some("2".to_owned()),
                    item_unit_price: Some(MoneyAmount {
                        value: Some("12.50".to_owned()),
                        currency_code: Some("EUR".to_owned()),
                    }),
                }],
            }),
        };

        let txns = parse_transaction_details(&[detail]);
        assert_eq!(txns.len(), 1);
        assert_eq!(txns[0].merchant_name.as_deref(), Some("Shop Owner"));
        assert_eq!(txns[0].items.len(), 1);
        assert_eq!(txns[0].items[0].name.as_deref(), Some("Widget"));
    }
}
