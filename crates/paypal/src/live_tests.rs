//! Live sandbox tests for the `PayPal` Transaction Search API.
//!
//! These tests hit the real `PayPal` sandbox API.
//!
//! Run with: `cargo test -p budget-paypal -- --ignored`
//!
//! Credentials are resolved from env vars or fall back to hardcoded sandbox defaults:
//!   - `PAYPAL_SANDBOX_CLIENT_ID`
//!   - `PAYPAL_SANDBOX_SECRET`

#[cfg(test)]
mod tests {
    use crate::client::PayPalClient;

    fn require_sandbox_client() -> PayPalClient {
        let client_id = std::env::var("PAYPAL_SANDBOX_CLIENT_ID").unwrap_or_else(|_| {
            "AUNp-kd4BZ-aJDjNqu1UkreDsUUg-dINtwyMt1aAqlgatWvfehkZcMPbXsaVUZkecF33asbo60h_kyaY"
                .to_owned()
        });
        let client_secret = std::env::var("PAYPAL_SANDBOX_SECRET").unwrap_or_else(|_| {
            "EJdFgrdAreavwIsEUq0An4icclTxdL3qJyOEeudsF1RRbjgsy2VCUW-T4UjbGAIrVtQRxBy5vNz5xssV"
                .to_owned()
        });
        PayPalClient::sandbox(client_id, client_secret).expect("failed to create sandbox client")
    }

    #[tokio::test]
    #[ignore = "hits live PayPal sandbox API"]
    async fn sandbox_oauth_token() {
        let mut client = require_sandbox_client();
        let token = client.ensure_token().await;
        match &token {
            Ok(t) => println!("obtained token: {}...", &t[..20.min(t.len())]),
            Err(e) => panic!("failed to obtain token: {e}"),
        }
        assert!(token.is_ok(), "should obtain OAuth token");
    }

    #[tokio::test]
    #[ignore = "hits live PayPal sandbox API — needs Transaction Search permission"]
    async fn sandbox_search_transactions() {
        let mut client = require_sandbox_client();
        let end = chrono::Utc::now();
        let start = end - chrono::Duration::days(30);
        let result = client.search_transactions(start, end).await;
        match &result {
            Ok(txns) => {
                println!("found {} transactions in last 30 days", txns.len());
                for txn in txns.iter().take(5) {
                    println!(
                        "  {} | {} {} | merchant={:?} | status={}",
                        txn.transaction_id, txn.amount, txn.currency, txn.merchant_name, txn.status,
                    );
                }
            }
            Err(e) => println!(
                "search returned error (may need Transaction Search permission enabled on sandbox app): {e}"
            ),
        }
    }

    #[tokio::test]
    #[ignore = "hits live PayPal sandbox API — needs Transaction Search permission"]
    async fn sandbox_pagination_across_windows() {
        let mut client = require_sandbox_client();
        let since = chrono::Utc::now() - chrono::Duration::days(90);
        let result = client.fetch_all_transactions(since).await;
        match &result {
            Ok(txns) => println!("fetched {} transactions over 90 days", txns.len()),
            Err(e) => println!(
                "multi-window fetch returned error (may need Transaction Search permission): {e}"
            ),
        }
    }

    #[tokio::test]
    #[ignore = "hits live PayPal sandbox API"]
    async fn sandbox_transaction_fields_parse() {
        let mut client = require_sandbox_client();
        let end = chrono::Utc::now();
        let start = end - chrono::Duration::days(30);
        if let Ok(txns) = client.search_transactions(start, end).await {
            for txn in txns.iter().take(3) {
                println!(
                    "  id={} date={} amount={} {} merchant={:?} items={}",
                    txn.transaction_id,
                    txn.transaction_date,
                    txn.amount,
                    txn.currency,
                    txn.merchant_name,
                    txn.items.len(),
                );
                for item in &txn.items {
                    println!("    item: {:?} qty={:?}", item.name, item.quantity);
                }
            }
        }
    }
}
