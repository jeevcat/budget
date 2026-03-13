use std::time::Duration;

use reqwest::Client;
use tracing::{debug, info, warn};

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

        info!(
            cookie_count = cookies.cookies().len(),
            expired = cookies.is_expired(),
            "created Amazon client"
        );

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
        info!(url = %url, "fetching transactions page");

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

        let status = resp.status();
        info!(status = %status, "transactions page response");

        if status.is_redirection() {
            let location = resp
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("(none)");
            warn!(status = %status, location = %location, "redirected — cookies likely expired");
            return Err(AmazonError::CookiesExpired);
        }

        let html = resp.text().await?;
        info!(
            html_len = html.len(),
            has_next_data = html.contains("__NEXT_DATA__"),
            "received transactions page HTML"
        );

        if html.len() < 1000 {
            warn!(html = %html, "suspiciously short response — likely an error page");
        }

        let title = extract_title(&html);
        if let Some(ref t) = title {
            debug!(title = %t, "page title");
        }

        let data = parse_next_data(&html).map_err(|e| {
            warn!(
                error = %e,
                html_len = html.len(),
                title = title.as_deref().unwrap_or("(none)"),
                html_preview = %truncate(&html, 2000),
                "failed to parse transactions page"
            );
            e
        })?;

        info!(
            token_len = data.token.len(),
            transaction_count = data.transactions.len(),
            has_more = data.has_more,
            "parsed transactions page"
        );

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
        let body = pagination_body(page_key);

        debug!(url = %url, page_key = %page_key, "fetching next transactions page");

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
        debug!(status = %status, "pagination response");

        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            warn!(status = %status, "pagination auth failure — JWT likely expired");
            return Err(AmazonError::JwtExpired);
        }
        if status == reqwest::StatusCode::SERVICE_UNAVAILABLE {
            warn!("pagination returned 503 — rate limited");
            return Err(AmazonError::RateLimited);
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            warn!(
                status = %status,
                body_preview = %truncate(&body, 2000),
                "unexpected pagination response status"
            );
            return Err(AmazonError::Parse(format!(
                "unexpected status {status} from pagination API"
            )));
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

        let transactions: Vec<_> = raw_txns
            .iter()
            .filter_map(|raw| convert_raw_transaction(raw).ok())
            .collect();

        info!(
            raw_count = raw_txns.len(),
            parsed_count = transactions.len(),
            has_next = next_key.is_some(),
            "parsed pagination response"
        );

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
                info!(dedup_key = %txn.dedup_key, "reached known transaction, stopping");
                stopped = true;
                break;
            }
            all_txns.push(txn);
        }

        if stopped || !first_page.has_more {
            info!(count = all_txns.len(), stopped, "finished first page");
            return Ok(all_txns);
        }

        let mut page_key = first_page.page_key;
        let mut page_num = 1u32;

        while let Some(key) = page_key.take() {
            page_num += 1;
            debug!(page = page_num, "rate limiting delay before next page");
            tokio::time::sleep(Duration::from_secs(1)).await;

            let (txns, next_key) = self.fetch_transactions_next(&key).await?;

            if txns.is_empty() {
                info!(page = page_num, "empty page, stopping pagination");
                break;
            }

            for txn in txns {
                if should_stop(&txn.dedup_key) {
                    info!(dedup_key = %txn.dedup_key, page = page_num, "reached known transaction, stopping");
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

        info!(
            total = all_txns.len(),
            pages = page_num,
            "finished fetching transactions"
        );
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

        let status = resp.status();
        debug!(status = %status, order_id = %order_id, "invoice response");

        if status.is_redirection() {
            let location = resp
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("(none)");
            warn!(order_id = %order_id, location = %location, "invoice redirected — cookies expired");
            return Err(AmazonError::CookiesExpired);
        }

        let html = resp.text().await?;
        debug!(order_id = %order_id, html_len = html.len(), "received invoice HTML");

        let order = parse_invoice_html(&html, order_id).map_err(|e| {
            warn!(
                order_id = %order_id,
                error = %e,
                html_len = html.len(),
                title = extract_title(&html).as_deref().unwrap_or("(none)"),
                html_preview = %truncate(&html, 2000),
                "failed to parse invoice"
            );
            e
        })?;

        info!(
            order_id = %order_id,
            item_count = order.items.len(),
            grand_total = ?order.grand_total,
            "parsed invoice"
        );

        Ok(order)
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

        let valid = !resp.status().is_redirection();
        info!(status = %resp.status(), valid, "cookie check");
        Ok(valid)
    }
}

/// Build the JSON body for the pagination API.
fn pagination_body(page_key: &str) -> serde_json::Value {
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

    serde_json::json!({
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
    })
}

/// Extract the <title> from an HTML page for diagnostics.
fn extract_title(html: &str) -> Option<String> {
    let start = html
        .find("<title")?
        .checked_add(html[html.find("<title")?..].find('>')?)?;
    let content = &html[start + 1..];
    let end = content.find("</title>")?;
    Some(content[..end].trim().to_owned())
}

/// Truncate a string for log output.
fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        // Find a valid char boundary
        let mut end = max;
        while !s.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        &s[..end]
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
