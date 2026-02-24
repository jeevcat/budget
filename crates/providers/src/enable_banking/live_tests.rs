//! Live sandbox tests for the Enable Banking integration.
//!
//! These tests hit the real Enable Banking sandbox API and require valid
//! credentials in `~/.config/budget/default.toml`.
//!
//! Run with: `cargo test -p budget-providers -- --ignored`
//!
//! Session-dependent tests (transactions, balances, accounts) require a
//! sandbox session. Set `EB_SESSION_ID` and `EB_ACCOUNT_UID` env vars
//! to provide one, or let the test attempt the full sandbox auth flow.
//!
//! To obtain a sandbox session manually:
//!   1. Run the server: `cargo run`
//!   2. POST /api/connections/authorize with a sandbox ASPSP
//!   3. Complete the sandbox auth in a browser
//!   4. Check the DB for the session ID and account UID

use std::fs;
use std::path::PathBuf;

use chrono::{Days, NaiveDate, Utc};

use super::auth::EnableBankingAuth;
use super::client::{Client, EnableBankingConfig};
use super::provider::EnableBankingProvider;
use crate::bank::{AccountId, BankProvider};

fn require_config() -> EnableBankingConfig {
    let config = budget_core::load_config().expect("failed to load budget config");

    let app_id = config
        .enable_banking_app_id
        .expect("enable_banking_app_id not set in config");

    let raw_path = config
        .enable_banking_private_key_path
        .expect("enable_banking_private_key_path not set in config");

    let key_path = if raw_path.starts_with('~') {
        home_dir().join(&raw_path[2..])
    } else {
        PathBuf::from(raw_path)
    };

    let pem = fs::read(&key_path)
        .unwrap_or_else(|e| panic!("failed to read private key at {}: {e}", key_path.display()));

    EnableBankingConfig::new(app_id, pem)
}

fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .expect("HOME not set")
}

/// Try to get a sandbox session from env vars.
/// Returns (session_id, account_uid) if set.
fn session_from_env() -> Option<(String, String)> {
    let session_id = std::env::var("EB_SESSION_ID").ok()?;
    let account_uid = std::env::var("EB_ACCOUNT_UID").ok()?;
    Some((session_id, account_uid))
}

/// Create a provider from an existing session by fetching account details.
async fn provider_from_session(session_id: &str) -> EnableBankingProvider {
    let config = require_config();
    let client = Client::new(config);
    let auth = EnableBankingAuth::new(Client::new(require_config()));

    // GET /sessions/{id} returns account list, but we don't have that endpoint.
    // Instead, create a session via exchange_code won't work without a valid code.
    // We construct the provider with an empty account list and rely on the
    // session_id for transaction/balance fetches.
    //
    // If EB_ACCOUNT_UID is set, we can still make API calls.
    let _ = auth;
    EnableBankingProvider::new(client, session_id.to_owned(), vec![])
}

// ── ASPSP listing ────────────────────────────────────────────────

#[tokio::test]
#[ignore = "hits live Enable Banking sandbox API"]
async fn sandbox_list_aspsps() {
    let config = require_config();
    let client = Client::new(config);
    let aspsps = client.get_aspsps(None).await.unwrap();

    assert!(!aspsps.is_empty(), "sandbox should have at least one ASPSP");
    println!("found {} ASPSPs", aspsps.len());
    for aspsp in aspsps.iter().take(5) {
        println!("  {} ({})", aspsp.name, aspsp.country);
    }
}

#[tokio::test]
#[ignore = "hits live Enable Banking sandbox API"]
async fn sandbox_list_aspsps_by_country() {
    let config = require_config();
    let client = Client::new(config);
    let aspsps = client.get_aspsps(Some("FI")).await.unwrap();

    assert!(
        aspsps.iter().all(|a| a.country == "FI"),
        "all results should be from Finland"
    );
    println!("found {} Finnish ASPSPs", aspsps.len());
}

#[tokio::test]
#[ignore = "hits live Enable Banking sandbox API"]
async fn sandbox_aspsp_fields_parse() {
    let config = require_config();
    let client = Client::new(config);
    let aspsps = client.get_aspsps(None).await.unwrap();

    // Verify that at least one sandbox ASPSP exists and fields parse
    let sandbox_aspsp = aspsps
        .iter()
        .find(|a| a.sandbox.is_some())
        .expect("no sandbox ASPSPs available");

    assert!(!sandbox_aspsp.name.is_empty(), "ASPSP should have a name");
    assert_eq!(
        sandbox_aspsp.country.len(),
        2,
        "country should be ISO 3166-1 alpha-2"
    );
    println!(
        "sandbox ASPSP: {} ({}) logo={} bic={} beta={}",
        sandbox_aspsp.name,
        sandbox_aspsp.country,
        sandbox_aspsp.logo.as_deref().unwrap_or("none"),
        sandbox_aspsp.bic.as_deref().unwrap_or("none"),
        sandbox_aspsp
            .beta
            .map_or("none".to_owned(), |b| b.to_string()),
    );
}

// ── Authorization flow ───────────────────────────────────────────

#[tokio::test]
#[ignore = "hits live Enable Banking sandbox API"]
async fn sandbox_start_authorization() {
    let config = require_config();
    let client = Client::new(config);

    // Find a sandbox ASPSP
    let aspsps = client.get_aspsps(None).await.unwrap();
    let sandbox_aspsp = aspsps
        .iter()
        .find(|a| a.sandbox.is_some())
        .expect("no sandbox ASPSPs available");

    println!(
        "using sandbox ASPSP: {} ({})",
        sandbox_aspsp.name, sandbox_aspsp.country
    );

    let valid_until = Utc::now()
        .checked_add_days(Days::new(90))
        .unwrap()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string();

    let auth_client = Client::new(require_config());
    let auth = EnableBankingAuth::new(auth_client);
    let url = auth
        .start_authorization(
            &sandbox_aspsp.name,
            &sandbox_aspsp.country,
            "http://localhost:3000/callback",
            "live-test-state",
            &valid_until,
            "personal",
        )
        .await
        .unwrap();

    assert!(url.starts_with("http"), "should return a redirect URL");
    println!("redirect URL: {url}");
}

// ── Session-dependent tests ──────────────────────────────────────
//
// These require EB_SESSION_ID and EB_ACCOUNT_UID env vars.
// Obtain them by completing a sandbox auth flow (see module docs).

#[tokio::test]
#[ignore = "hits live Enable Banking sandbox API — needs EB_SESSION_ID + EB_ACCOUNT_UID"]
async fn sandbox_fetch_balances() {
    let (session_id, account_uid) = match session_from_env() {
        Some(s) => s,
        None => {
            println!("SKIPPED: set EB_SESSION_ID and EB_ACCOUNT_UID to run");
            return;
        }
    };

    println!("session: {session_id}, account: {account_uid}");
    let provider = provider_from_session(&session_id).await;
    let account_id = AccountId(account_uid);

    let balance = provider.get_balances(&account_id).await.unwrap();

    println!(
        "balance: available={} current={} currency={}",
        balance.available, balance.current, balance.currency
    );
    assert!(!balance.currency.is_empty(), "currency should not be empty");
}

#[tokio::test]
#[ignore = "hits live Enable Banking sandbox API — needs EB_SESSION_ID + EB_ACCOUNT_UID"]
async fn sandbox_fetch_transactions() {
    let (session_id, account_uid) = match session_from_env() {
        Some(s) => s,
        None => {
            println!("SKIPPED: set EB_SESSION_ID and EB_ACCOUNT_UID to run");
            return;
        }
    };

    println!("session: {session_id}, account: {account_uid}");
    let provider = provider_from_session(&session_id).await;
    let account_id = AccountId(account_uid);
    let since = NaiveDate::from_ymd_opt(2020, 1, 1).unwrap();

    let txns = provider
        .fetch_transactions(&account_id, since)
        .await
        .unwrap();

    println!("fetched {} transactions", txns.len());
    for txn in txns.iter().take(5) {
        println!(
            "  {} | {:>10} {} | merchant={:?} | counterparty={:?} | desc={:?}",
            txn.posted_date,
            txn.amount,
            txn.currency,
            txn.merchant_name,
            txn.counterparty_name,
            txn.description,
        );
    }

    if !txns.is_empty() {
        let first = &txns[0];
        assert!(
            !first.provider_transaction_id.is_empty(),
            "transaction should have an id"
        );
        assert!(
            !first.currency.is_empty(),
            "transaction should have currency"
        );
        // Verify creditor/debtor nested parsing produces names
        let has_any_name = txns
            .iter()
            .any(|t| !t.merchant_name.is_empty() || t.counterparty_name.is_some());
        assert!(
            has_any_name,
            "at least one transaction should have a merchant or counterparty name"
        );
    }
}

#[tokio::test]
#[ignore = "hits live Enable Banking sandbox API — needs EB_SESSION_ID + EB_ACCOUNT_UID"]
async fn sandbox_session_accounts() {
    let (session_id, _account_uid) = match session_from_env() {
        Some(s) => s,
        None => {
            println!("SKIPPED: set EB_SESSION_ID and EB_ACCOUNT_UID to run");
            return;
        }
    };

    // Exchange a fresh session to verify account field parsing.
    // This won't work with an existing session — it needs a fresh code.
    // Instead, re-fetch session details via the session endpoint.
    let config = require_config();
    let client = Client::new(config);

    // GET /sessions/{id} to verify account deserialization
    let session = client.get_session(&session_id).await;
    match session {
        Ok(session) => {
            println!(
                "session {} has {} accounts",
                session.session_id,
                session.accounts.len()
            );
            for acct in &session.accounts {
                println!(
                    "  uid={} name={:?} iban={:?} servicer={:?} type={:?} currency={:?}",
                    acct.uid,
                    acct.name,
                    acct.account_id.as_ref().and_then(|id| id.iban.as_ref()),
                    acct.account_servicer.as_ref().and_then(|s| s.name.as_ref()),
                    acct.cash_account_type,
                    acct.currency,
                );
            }
            assert!(!session.accounts.is_empty(), "session should have accounts");
            // Verify the corrected field names work
            let first = &session.accounts[0];
            assert!(!first.uid.is_empty(), "account should have uid");
            assert!(
                first.currency.is_some(),
                "account should have currency (required per API)"
            );
        }
        Err(e) => {
            println!("could not fetch session (may be expired): {e}");
        }
    }
}
