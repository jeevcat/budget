use std::time::Duration;

use reqwest::Client;
use tracing::debug;

use crate::cookies::CookieStore;
use crate::error::{AmazonError, Result};
use crate::parser::{convert_raw_transaction, parse_invoice_html, parse_next_data};
use crate::types::{AmazonOrder, AmazonTransaction, PaginationResponse, TransactionsPageData};

const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
const BASE_URL: &str = "https://www.amazon.de";

/// HTTP client for Amazon's payments portal and invoice pages.
pub struct AmazonClient {
    client: Client,
    cookies: CookieStore,
    token: Option<String>,
}

impl AmazonClient {
    /// Create a new Amazon client with the given cookies.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be built.
    pub fn new(cookies: CookieStore) -> Result<Self> {
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(30))
            .build()?;

        Ok(Self {
            client,
            cookies,
            token: None,
        })
    }

    /// Fetch the initial transactions page and extract token + first page of data.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails, cookies are expired, or the
    /// page cannot be parsed.
    pub async fn fetch_transactions_page(&mut self) -> Result<TransactionsPageData> {
        let url = format!("{BASE_URL}/cpe/yourpayments/transactions");
        debug!(url = %url, "fetching transactions page");

        let resp = self
            .client
            .get(&url)
            .header("Cookie", self.cookies.cookie_header())
            .header("User-Agent", USER_AGENT)
            .header(
                "Accept",
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            )
            .header("Accept-Language", "en-GB,en;q=0.9,de;q=0.8")
            .send()
            .await?;

        if resp.status().is_redirection() {
            return Err(AmazonError::CookiesExpired);
        }

        let html = resp.text().await?;

        if html.contains("Enter the characters you see below") || html.contains("robot") {
            return Err(AmazonError::BotDetected);
        }

        let data = parse_next_data(&html)?;
        self.token = Some(data.token.clone());
        Ok(data)
    }

    /// Fetch the next page of transactions using the pagination API.
    ///
    /// # Errors
    ///
    /// Returns an error if the JWT is missing/expired, the request fails,
    /// or the response cannot be parsed.
    pub async fn fetch_transactions_next(
        &self,
        page_key: &str,
    ) -> Result<(Vec<AmazonTransaction>, Option<String>)> {
        let token = self.token.as_deref().ok_or(AmazonError::JwtExpired)?;

        let url =
            format!("{BASE_URL}/payments-portal/data/iris/live/v1/data/manage/get-transactions");

        let trace_id: String = (0..20)
            .map(|_| {
                let idx = rand_u8() % 36;
                if idx < 10 {
                    (b'0' + idx) as char
                } else {
                    (b'A' + idx - 10) as char
                }
            })
            .collect();

        let app_trace_id = format!(
            "A{}",
            (0..32)
                .map(|_| {
                    let idx = rand_u8() % 16;
                    if idx < 10 {
                        (b'0' + idx) as char
                    } else {
                        (b'a' + idx - 10) as char
                    }
                })
                .collect::<String>()
        );

        let body = serde_json::json!({
            "type": "GetTransactions",
            "locale": "en_GB",
            "surfaceInfo": {
                "surfaceType": "desktop",
                "clientApplicationType": "browser",
                "surfaceFeatures": [],
                "userAgent": USER_AGENT
            },
            "requestTraceId": trace_id,
            "applicationInstanceTraceId": app_trace_id,
            "transactionsViewRequest": {
                "filtersControls": {
                    "includeFilters": true
                }
            },
            "exclusiveStartKey": page_key,
            "widgetName": "ViewTransactions"
        });

        debug!(url = %url, "fetching next transactions page");

        let resp = self
            .client
            .post(&url)
            .header("Cookie", self.cookies.cookie_header())
            .header("User-Agent", USER_AGENT)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("x-amzn-upx-token", token)
            .header("Sec-Fetch-Dest", "empty")
            .header("Sec-Fetch-Mode", "cors")
            .header("Sec-Fetch-Site", "same-origin")
            .header(
                "Referer",
                format!("{BASE_URL}/cpe/yourpayments/transactions"),
            )
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(AmazonError::JwtExpired);
        }
        if status == reqwest::StatusCode::SERVICE_UNAVAILABLE {
            return Err(AmazonError::RateLimited);
        }

        let pagination: PaginationResponse = resp.json().await?;

        let (raw_txns, next_key) = match pagination.display_response {
            Some(dr) => {
                let txns = dr.transactions_list.unwrap_or_default();
                let key = dr.last_evaluated_page_key;
                (txns, key)
            }
            None => (Vec::new(), None),
        };

        let transactions = raw_txns
            .iter()
            .filter_map(|raw| convert_raw_transaction(raw).ok())
            .collect();

        Ok((transactions, next_key))
    }

    /// Fetch all transactions, paginating until `should_stop` returns `true`
    /// for a dedup key (indicating we've reached already-known transactions).
    ///
    /// # Errors
    ///
    /// Returns an error if any page fetch or parse fails.
    pub async fn fetch_all_transactions(
        &mut self,
        should_stop: impl Fn(&str) -> bool,
    ) -> Result<Vec<AmazonTransaction>> {
        let first_page = self.fetch_transactions_page().await?;
        let mut all_txns = Vec::new();
        let mut stopped = false;

        for txn in first_page.transactions {
            if should_stop(&txn.dedup_key) {
                stopped = true;
                break;
            }
            all_txns.push(txn);
        }

        if stopped || !first_page.has_more {
            return Ok(all_txns);
        }

        let mut page_key = first_page.page_key;

        while let Some(key) = page_key.take() {
            // Rate limiting delay
            tokio::time::sleep(Duration::from_secs(1)).await;

            let (txns, next_key) = self.fetch_transactions_next(&key).await?;

            if txns.is_empty() {
                break;
            }

            for txn in txns {
                if should_stop(&txn.dedup_key) {
                    stopped = true;
                    break;
                }
                all_txns.push(txn);
            }

            if stopped {
                break;
            }

            page_key = next_key;
        }

        Ok(all_txns)
    }

    /// Fetch order details from the invoice page.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails, cookies are expired, or the
    /// invoice page cannot be parsed.
    pub async fn fetch_order_details(&self, order_id: &str) -> Result<AmazonOrder> {
        let url = format!("{BASE_URL}/gp/css/summary/print.html?orderID={order_id}");
        debug!(url = %url, order_id = %order_id, "fetching invoice");

        let resp = self
            .client
            .get(&url)
            .header("Cookie", self.cookies.cookie_header())
            .header("User-Agent", USER_AGENT)
            .header(
                "Accept",
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            )
            .send()
            .await?;

        if resp.status().is_redirection() {
            return Err(AmazonError::CookiesExpired);
        }

        let html = resp.text().await?;

        if html.contains("Enter the characters you see below") {
            return Err(AmazonError::BotDetected);
        }

        parse_invoice_html(&html, order_id)
    }

    /// Check whether cookies are still valid by making a test request.
    ///
    /// Returns `true` if authenticated, `false` if redirected to sign-in.
    ///
    /// # Errors
    ///
    /// Returns an error if the network request fails.
    pub async fn check_cookies(&self) -> Result<bool> {
        let url = format!("{BASE_URL}/cpe/yourpayments/transactions");

        let resp = self
            .client
            .get(&url)
            .header("Cookie", self.cookies.cookie_header())
            .header("User-Agent", USER_AGENT)
            .send()
            .await?;

        Ok(!resp.status().is_redirection())
    }
}

/// Simple non-crypto random byte for trace IDs.
#[allow(clippy::cast_possible_truncation)]
fn rand_u8() -> u8 {
    use std::time::SystemTime;
    let t = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    (t.subsec_nanos() ^ t.as_secs() as u32) as u8
}
