use chrono::NaiveDate;
use regex::Regex;
use rust_decimal::Decimal;
use scraper::{Html, Selector};
use std::sync::LazyLock;
use tracing::{debug, info, warn};

use crate::error::{AmazonError, Result};
use crate::types::{
    AmazonItem, AmazonOrder, AmazonTransaction, AmazonTransactionStatus, NextData, RawTransaction,
    TransactionsPageData,
};

// ---------------------------------------------------------------------------
// Amount and date parsing
// ---------------------------------------------------------------------------

static AMOUNT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([+-])?[^0-9]*([0-9]+(?:\.[0-9]+)?)$").unwrap());

/// Parse an Amazon formatted amount like `"-€42.91"` or `"+€80.99"` into a Decimal.
///
/// The sign prefix determines the sign: `-` means charge, `+` means refund,
/// no prefix means positive.
///
/// # Errors
///
/// Returns an error if the string cannot be parsed as an amount.
pub fn parse_amount(s: &str) -> Result<Decimal> {
    // Strip thousands separators (commas) first, e.g., "1,234.56" -> "1234.56"
    let cleaned = s.replace(',', "");
    let caps = AMOUNT_RE
        .captures(&cleaned)
        .ok_or_else(|| AmazonError::Parse(format!("cannot parse amount: {s:?}")))?;

    let sign = caps.get(1).map_or("", |m| m.as_str());
    let digits = &caps[2];

    let value: Decimal = digits
        .parse()
        .map_err(|e| AmazonError::Parse(format!("invalid decimal {digits:?}: {e}")))?;

    if sign == "-" { Ok(-value) } else { Ok(value) }
}

static DATE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\d{1,2})\s+(\w+)\s+(\d{4})").unwrap());

/// Parse an Amazon formatted date like `"07 Oct 2023"` into a `NaiveDate`.
///
/// # Errors
///
/// Returns an error if the string cannot be parsed as a date.
pub fn parse_date(s: &str) -> Result<NaiveDate> {
    let caps = DATE_RE
        .captures(s)
        .ok_or_else(|| AmazonError::Parse(format!("cannot parse date: {s:?}")))?;

    let day: u32 = caps[1]
        .parse()
        .map_err(|e| AmazonError::Parse(format!("invalid day: {e}")))?;
    let month_str = &caps[2];
    let year: i32 = caps[3]
        .parse()
        .map_err(|e| AmazonError::Parse(format!("invalid year: {e}")))?;

    let month = match month_str.to_lowercase().as_str() {
        "jan" | "january" => 1,
        "feb" | "february" => 2,
        "mar" | "march" => 3,
        "apr" | "april" => 4,
        "may" => 5,
        "jun" | "june" => 6,
        "jul" | "july" => 7,
        "aug" | "august" => 8,
        "sep" | "sept" | "september" => 9,
        "oct" | "october" => 10,
        "nov" | "november" => 11,
        "dec" | "december" => 12,
        other => {
            return Err(AmazonError::Parse(format!("unknown month: {other:?}")));
        }
    };

    NaiveDate::from_ymd_opt(year, month, day)
        .ok_or_else(|| AmazonError::Parse(format!("invalid date: {year}-{month}-{day}")))
}

static ASIN_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"/dp/([A-Z0-9]{10})").unwrap());

/// Extract an ASIN from an Amazon product URL.
#[must_use]
pub fn extract_asin(href: &str) -> Option<String> {
    ASIN_RE.captures(href).map(|c| c[1].to_owned())
}

static ORDER_ID_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d{3}-\d{7}-\d{7})").unwrap());

/// Extract an order ID from a display string like `"Order #304-3409393-5041100"`.
#[must_use]
pub fn extract_order_id(s: &str) -> Option<String> {
    ORDER_ID_RE.captures(s).map(|c| c[1].to_owned())
}

/// Parse a price string like `"€4.52"` or `"€80.99"` into a Decimal.
///
/// # Errors
///
/// Returns an error if the string cannot be parsed as a price.
pub fn parse_price(s: &str) -> Result<Decimal> {
    // Strip everything except digits, '.', '-', '+'
    let cleaned: String = s
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-' || *c == '+')
        .collect();
    if cleaned.is_empty() {
        return Err(AmazonError::Parse(format!(
            "empty price after cleaning: {s:?}"
        )));
    }
    cleaned
        .parse()
        .map_err(|e| AmazonError::Parse(format!("invalid price {s:?}: {e}")))
}

/// Generate a deterministic dedup key from date, amount, and descriptor.
#[must_use]
pub fn dedup_key(date: NaiveDate, amount: Decimal, descriptor: &str) -> String {
    format!("{date}|{amount}|{descriptor}")
}

// ---------------------------------------------------------------------------
// __NEXT_DATA__ extraction
// ---------------------------------------------------------------------------

/// Parse the `__NEXT_DATA__` script tag from an Amazon transactions page HTML.
///
/// Extracts the JWT token, first page of transactions, pagination info.
///
/// # Errors
///
/// Returns an error if the script tag is missing or the JSON cannot be parsed.
pub fn parse_next_data(html: &str) -> Result<TransactionsPageData> {
    let document = Html::parse_document(html);
    let selector = Selector::parse("script#__NEXT_DATA__")
        .map_err(|e| AmazonError::Parse(format!("invalid selector: {e:?}")))?;

    let script = document
        .select(&selector)
        .next()
        .ok_or_else(|| AmazonError::Parse("no __NEXT_DATA__ script tag found".into()))?;

    let json_text = script.text().collect::<String>();
    debug!(json_len = json_text.len(), "__NEXT_DATA__ extracted");

    let next_data: NextData = serde_json::from_str(&json_text).map_err(|e| {
        warn!(
            error = %e,
            json_len = json_text.len(),
            json_body = %json_text,
            "failed to parse __NEXT_DATA__ JSON"
        );
        AmazonError::Json(e)
    })?;

    let page_props = next_data.props.page_props;
    let txn_state = page_props.state.transaction_response_state;
    let visible = txn_state.visible_transaction_response;

    info!(
        raw_transaction_count = visible.transaction_list.len(),
        has_more = txn_state.has_more,
        has_page_key = txn_state.last_evaluated_page_key.is_some(),
        "parsed __NEXT_DATA__ structure"
    );

    let mut parse_errors = 0u32;
    let transactions: Vec<_> = visible
        .transaction_list
        .into_iter()
        .filter_map(|raw| match convert_raw_transaction(&raw) {
            Ok(txn) => Some(txn),
            Err(e) => {
                parse_errors += 1;
                warn!(
                    error = %e,
                    amount = %raw.formatted_amount,
                    date = %raw.formatted_date,
                    "skipped unparseable transaction"
                );
                None
            }
        })
        .collect();

    if parse_errors > 0 {
        warn!(
            parse_errors,
            parsed = transactions.len(),
            "some transactions failed to parse"
        );
    }

    info!(
        token_len = page_props.token.len(),
        transaction_count = transactions.len(),
        has_more = txn_state.has_more,
        "parsed transactions from __NEXT_DATA__"
    );

    Ok(TransactionsPageData {
        token: page_props.token,
        transactions,
        has_more: txn_state.has_more,
        page_key: txn_state.last_evaluated_page_key,
    })
}

/// Convert a raw API transaction to our domain type.
///
/// # Errors
///
/// Returns an error if the amount or date cannot be parsed.
pub fn convert_raw_transaction(raw: &RawTransaction) -> Result<AmazonTransaction> {
    let amount = parse_amount(&raw.formatted_amount)?;
    let date = parse_date(&raw.formatted_date)?;

    let status = raw
        .status_info
        .as_ref()
        .and_then(|si| si.label.as_deref())
        .map_or(AmazonTransactionStatus::Charged, |label| {
            match label.to_lowercase().as_str() {
                "refunded" => AmazonTransactionStatus::Refunded,
                "declined" => AmazonTransactionStatus::Declined,
                _ => AmazonTransactionStatus::Charged,
            }
        });

    let descriptor = raw.statement_descriptor.as_deref().unwrap_or("").to_owned();

    let payment_method = raw
        .payment_method_data
        .as_ref()
        .map(|pm| {
            let name = pm.payment_method_name.as_deref().unwrap_or("");
            let last4 = pm
                .payment_method_number
                .as_ref()
                .and_then(|n| n.last_digits.as_deref())
                .unwrap_or("");
            if last4.is_empty() {
                name.to_owned()
            } else {
                format!("{name} ••••{last4}")
            }
        })
        .unwrap_or_default();

    let order_ids: Vec<String> = raw
        .order_data
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .filter_map(|o| o.order_display_string.as_deref().and_then(extract_order_id))
        .collect();

    let key = dedup_key(date, amount, &descriptor);

    Ok(AmazonTransaction {
        date,
        amount,
        currency: "EUR".to_owned(),
        statement_descriptor: descriptor,
        status,
        payment_method,
        order_ids,
        dedup_key: key,
    })
}

// ---------------------------------------------------------------------------
// Invoice HTML parsing
// ---------------------------------------------------------------------------

/// Parse an Amazon invoice page HTML to extract order details.
///
/// # Errors
///
/// Returns an error if the HTML cannot be parsed.
pub fn parse_invoice_html(html: &str, order_id: &str) -> Result<AmazonOrder> {
    debug!(order_id = %order_id, html_len = html.len(), "parsing invoice HTML");
    debug!(order_id = %order_id, html_body = %html, "full invoice HTML");

    let document = Html::parse_document(html);

    let items = parse_invoice_items(&document);
    let totals = parse_invoice_totals(&document);
    let order_date = parse_invoice_date(&document);

    info!(
        order_id = %order_id,
        item_count = items.len(),
        total_keys = ?totals.keys().collect::<Vec<_>>(),
        order_date = ?order_date,
        "parsed invoice"
    );

    Ok(AmazonOrder {
        order_id: order_id.to_owned(),
        order_date,
        grand_total: totals.get("grand_total").copied(),
        subtotal: totals.get("subtotal").copied(),
        shipping: totals.get("shipping").copied(),
        vat: totals.get("vat").copied(),
        promotion: totals.get("promotion").copied(),
        items,
    })
}

fn parse_invoice_items(document: &Html) -> Vec<AmazonItem> {
    let item_link_sel = Selector::parse("a.a-link-normal").unwrap();
    let price_sel = Selector::parse("span.a-price span.a-offscreen").unwrap();

    let mut items = Vec::new();

    for link in document.select(&item_link_sel) {
        let href = link.value().attr("href").unwrap_or("");
        if !href.contains("/dp/") {
            continue;
        }

        let title = link.text().collect::<String>().trim().to_owned();
        if title.is_empty() {
            continue;
        }

        let asin = extract_asin(href);

        // Try to find a price near this item by looking at the parent's subtree
        let price = link
            .parent()
            .and_then(|p| p.parent())
            .and_then(|grandparent| {
                let grandparent_ref = scraper::ElementRef::wrap(grandparent)?;
                grandparent_ref.select(&price_sel).next().and_then(|el| {
                    let text = el.text().collect::<String>();
                    parse_price(text.trim()).ok()
                })
            });

        items.push(AmazonItem {
            title,
            asin,
            price,
            quantity: 1,
            seller: None,
            image_url: None,
        });
    }

    items
}

fn parse_invoice_totals(document: &Html) -> std::collections::HashMap<&'static str, Decimal> {
    let row_sel = Selector::parse("div.od-line-item-row").unwrap();
    let label_sel = Selector::parse("div.od-line-item-row-label span").unwrap();
    let value_sel = Selector::parse("div.od-line-item-row-content span").unwrap();

    let mut totals = std::collections::HashMap::new();

    for row in document.select(&row_sel) {
        let label = row
            .select(&label_sel)
            .next()
            .map(|el| el.text().collect::<String>())
            .unwrap_or_default();
        let value_text = row
            .select(&value_sel)
            .next()
            .map(|el| el.text().collect::<String>())
            .unwrap_or_default();

        let label_lower = label.to_lowercase();
        let key = if label_lower.contains("subtotal") {
            Some("subtotal")
        } else if label_lower.contains("postage")
            || label_lower.contains("shipping")
            || label_lower.contains("packing")
        {
            Some("shipping")
        } else if label_lower.contains("vat") || label_lower.contains("tax") {
            Some("vat")
        } else if label_lower.contains("grand total") {
            Some("grand_total")
        } else if label_lower.contains("promotion") {
            Some("promotion")
        } else {
            None
        };

        if let Some(k) = key
            && let Ok(val) = parse_price(value_text.trim())
        {
            totals.insert(k, val);
        }
    }

    totals
}

fn parse_invoice_date(document: &Html) -> Option<NaiveDate> {
    // Look for text containing "Order Placed" or "Bestellung aufgegeben"
    // followed by a date
    let body = document.root_element().text().collect::<String>();
    for line in body.lines() {
        let trimmed = line.trim();
        if (trimmed.contains("Order Placed")
            || trimmed.contains("Bestellung aufgegeben")
            || trimmed.contains("Ordered on"))
            && let Ok(date) = parse_date(trimmed)
        {
            return Some(date);
        }
    }

    // Fallback: scan all text for date patterns
    for line in body.lines() {
        if let Ok(date) = parse_date(line.trim()) {
            return Some(date);
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    // -- Amount parsing --

    #[test]
    fn parse_amount_charge() {
        assert_eq!(parse_amount("-€42.91").unwrap(), dec!(-42.91));
    }

    #[test]
    fn parse_amount_refund() {
        assert_eq!(parse_amount("+€80.99").unwrap(), dec!(80.99));
    }

    #[test]
    fn parse_amount_no_sign() {
        assert_eq!(parse_amount("€12.99").unwrap(), dec!(12.99));
    }

    #[test]
    fn parse_amount_with_thousands_separator() {
        assert_eq!(parse_amount("-€1,234.56").unwrap(), dec!(-1234.56));
    }

    #[test]
    fn parse_amount_invalid() {
        assert!(parse_amount("abc").is_err());
    }

    // -- Date parsing --

    #[test]
    fn parse_date_standard() {
        assert_eq!(
            parse_date("07 Oct 2023").unwrap(),
            NaiveDate::from_ymd_opt(2023, 10, 7).unwrap()
        );
    }

    #[test]
    fn parse_date_full_month() {
        assert_eq!(
            parse_date("15 January 2024").unwrap(),
            NaiveDate::from_ymd_opt(2024, 1, 15).unwrap()
        );
    }

    #[test]
    fn parse_date_single_digit_day() {
        assert_eq!(
            parse_date("3 Mar 2025").unwrap(),
            NaiveDate::from_ymd_opt(2025, 3, 3).unwrap()
        );
    }

    #[test]
    fn parse_date_december() {
        assert_eq!(
            parse_date("25 Dec 2023").unwrap(),
            NaiveDate::from_ymd_opt(2023, 12, 25).unwrap()
        );
    }

    #[test]
    fn parse_date_invalid_month() {
        assert!(parse_date("07 Foo 2023").is_err());
    }

    // -- ASIN extraction --

    #[test]
    fn extract_asin_from_url() {
        assert_eq!(
            extract_asin("/dp/B07BLNQKVZ?ref=ppx_something"),
            Some("B07BLNQKVZ".to_owned())
        );
    }

    #[test]
    fn extract_asin_full_url() {
        assert_eq!(
            extract_asin("https://www.amazon.de/-/en/dp/B00LN803K0/ref=ppx_printOD"),
            Some("B00LN803K0".to_owned())
        );
    }

    #[test]
    fn extract_asin_no_match() {
        assert_eq!(extract_asin("/gp/css/summary/print.html"), None);
    }

    // -- Order ID extraction --

    #[test]
    fn extract_order_id_from_display_string() {
        assert_eq!(
            extract_order_id("Order #304-3409393-5041100"),
            Some("304-3409393-5041100".to_owned())
        );
    }

    #[test]
    fn extract_order_id_no_match() {
        assert_eq!(extract_order_id("Some random text"), None);
    }

    // -- Price parsing --

    #[test]
    fn parse_price_euro() {
        assert_eq!(parse_price("€4.52").unwrap(), dec!(4.52));
    }

    #[test]
    fn parse_price_with_sign() {
        assert_eq!(parse_price("-€4.99").unwrap(), dec!(-4.99));
    }

    // -- Dedup key --

    #[test]
    fn dedup_key_deterministic() {
        let date = NaiveDate::from_ymd_opt(2023, 10, 7).unwrap();
        let key1 = dedup_key(date, dec!(-42.91), "AMZN Mktp DE");
        let key2 = dedup_key(date, dec!(-42.91), "AMZN Mktp DE");
        assert_eq!(key1, key2);
        assert_eq!(key1, "2023-10-07|-42.91|AMZN Mktp DE");
    }

    // -- __NEXT_DATA__ parsing --

    #[test]
    fn parse_next_data_extracts_token_and_transactions() {
        let html = include_str!("../tests/fixtures/next_data.html");
        let result = parse_next_data(html).unwrap();

        assert_eq!(result.token, "eyJhbGciOiJFUzM4NCIsInR5cCI6IkpXVCJ9.test");
        assert_eq!(result.transactions.len(), 2);
        assert!(result.has_more);
        assert_eq!(
            result.page_key.as_deref(),
            Some("eyJsYXN0RXZhbHVhdGVkS2V5IjoidGVzdCJ9")
        );

        let t1 = &result.transactions[0];
        assert_eq!(t1.amount, dec!(-42.91));
        assert_eq!(t1.date, NaiveDate::from_ymd_opt(2023, 10, 7).unwrap());
        assert_eq!(t1.statement_descriptor, "AMZN Mktp DE");
        assert_eq!(t1.status, AmazonTransactionStatus::Charged);
        assert_eq!(t1.order_ids, vec!["304-3409393-5041100"]);

        let t2 = &result.transactions[1];
        assert_eq!(t2.amount, dec!(80.99));
        assert_eq!(t2.status, AmazonTransactionStatus::Refunded);
    }

    // -- Invoice HTML parsing --

    #[test]
    fn parse_invoice_extracts_items_and_totals() {
        let html = include_str!("../tests/fixtures/invoice.html");
        let order = parse_invoice_html(html, "304-3409393-5041100").unwrap();

        assert_eq!(order.order_id, "304-3409393-5041100");
        assert_eq!(order.items.len(), 2);

        let item1 = &order.items[0];
        assert_eq!(item1.title, "Varta AAA Batteries (40 pack)");
        assert_eq!(item1.asin.as_deref(), Some("B07BLNQKVZ"));
        assert_eq!(item1.price, Some(dec!(16.49)));

        let item2 = &order.items[1];
        assert_eq!(item2.title, "USB-C Cable 2m");
        assert_eq!(item2.asin.as_deref(), Some("B00LN803K0"));
        assert_eq!(item2.price, Some(dec!(12.99)));

        assert_eq!(order.subtotal, Some(dec!(29.48)));
        assert_eq!(order.shipping, Some(dec!(0.00)));
        assert_eq!(order.vat, Some(dec!(4.71)));
        assert_eq!(order.grand_total, Some(dec!(34.19)));
    }
}
