//! Live sandbox tests for the Enable Banking integration.
//!
//! These tests hit the real Enable Banking sandbox API and require valid
//! credentials in `~/.config/budget/default.toml`.
//!
//! Run with: `cargo test -p budget-providers -- --ignored`

use std::fs;
use std::path::PathBuf;

use chrono::{Days, Utc};

use super::auth::EnableBankingAuth;
use super::client::{Client, EnableBankingConfig};

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
        )
        .await
        .unwrap();

    assert!(url.starts_with("http"), "should return a redirect URL");
    println!("redirect URL: {url}");
}
