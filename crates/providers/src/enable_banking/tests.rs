use chrono::NaiveDate;
use rust_decimal_macros::dec;
use serde_json::json;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::bank::{AccountId, BankProvider};
use crate::enable_banking::EnableBankingConfig;
use crate::enable_banking::client::Client;
use crate::enable_banking::provider::EnableBankingProvider;
use crate::enable_banking::types::{AccountIdentification, AccountServicer, SessionAccount};
use crate::error::ProviderError;

use super::auth::EnableBankingAuth;

fn test_config(base_url: &str) -> EnableBankingConfig {
    EnableBankingConfig {
        app_id: "test-app".to_owned(),
        private_key_pem: include_bytes!("test_fixtures/test_key.pem").to_vec(),
        base_url: base_url.to_owned(),
    }
}

fn test_session_account() -> SessionAccount {
    SessionAccount {
        uid: "acct-001".to_owned(),
        name: Some("My Checking".to_owned()),
        account_id: Some(AccountIdentification {
            iban: Some("FI1234567890".to_owned()),
        }),
        account_servicer: Some(AccountServicer {
            name: Some("Test Bank".to_owned()),
            bic_fi: None,
        }),
        product: None,
        cash_account_type: Some("CACC".to_owned()),
        currency: Some("EUR".to_owned()),
    }
}

fn provider_with(base_url: &str) -> EnableBankingProvider {
    let config = test_config(base_url);
    let client = Client::new(config);
    EnableBankingProvider::new(client, "sess-001".to_owned(), vec![test_session_account()])
}

// ── Transaction fetching ──────────────────────────────────────────

#[tokio::test]
async fn single_page_transactions() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/accounts/acct-001/transactions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "transactions": [
                {
                    "transaction_id": "t1",
                    "status": "BOOK",
                    "credit_debit_indicator": "DBIT",
                    "transaction_amount": { "amount": "25.00", "currency": "EUR" },
                    "booking_date": "2025-03-10",
                    "creditor": { "name": "Shop A" }
                },
                {
                    "transaction_id": "t2",
                    "status": "BOOK",
                    "credit_debit_indicator": "CRDT",
                    "transaction_amount": { "amount": "100.00", "currency": "EUR" },
                    "booking_date": "2025-03-11",
                    "debtor": { "name": "Employer" }
                }
            ]
        })))
        .mount(&server)
        .await;

    let provider = provider_with(&server.uri());
    let account_id = AccountId("acct-001".to_owned());
    let since = NaiveDate::from_ymd_opt(2025, 3, 1).unwrap();
    let txns = provider
        .fetch_transactions(&account_id, since)
        .await
        .unwrap();

    assert_eq!(txns.len(), 2);
    assert_eq!(txns[0].amount, dec!(-25.00));
    assert_eq!(txns[0].merchant_name, "Shop A");
    assert_eq!(txns[1].amount, dec!(100.00));
    assert_eq!(txns[1].merchant_name, "Employer");
}

#[tokio::test]
async fn multi_page_pagination() {
    let server = MockServer::start().await;

    // First page — has a continuation_key
    Mock::given(method("GET"))
        .and(path("/accounts/acct-001/transactions"))
        .and(query_param("date_from", "2025-03-01"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "transactions": [
                {
                    "transaction_id": "t1",
                    "status": "BOOK",
                    "credit_debit_indicator": "DBIT",
                    "transaction_amount": { "amount": "10.00", "currency": "EUR" },
                    "booking_date": "2025-03-01",
                    "creditor": { "name": "Shop" }
                }
            ],
            "continuation_key": "page2"
        })))
        .up_to_n_times(1)
        .mount(&server)
        .await;

    // Second page — no continuation_key
    Mock::given(method("GET"))
        .and(path("/accounts/acct-001/transactions"))
        .and(query_param("continuation_key", "page2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "transactions": [
                {
                    "transaction_id": "t2",
                    "status": "BOOK",
                    "credit_debit_indicator": "CRDT",
                    "transaction_amount": { "amount": "50.00", "currency": "EUR" },
                    "booking_date": "2025-03-02",
                    "debtor": { "name": "Employer" }
                }
            ]
        })))
        .mount(&server)
        .await;

    let provider = provider_with(&server.uri());
    let account_id = AccountId("acct-001".to_owned());
    let since = NaiveDate::from_ymd_opt(2025, 3, 1).unwrap();
    let txns = provider
        .fetch_transactions(&account_id, since)
        .await
        .unwrap();

    assert_eq!(txns.len(), 2);
    assert_eq!(txns[0].provider_transaction_id, "t1");
    assert_eq!(txns[1].provider_transaction_id, "t2");
}

#[tokio::test]
async fn pending_transactions_filtered_out() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/accounts/acct-001/transactions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "transactions": [
                {
                    "transaction_id": "t1",
                    "status": "BOOK",
                    "credit_debit_indicator": "DBIT",
                    "transaction_amount": { "amount": "10.00", "currency": "EUR" },
                    "booking_date": "2025-03-01",
                    "creditor": { "name": "Shop" }
                },
                {
                    "transaction_id": "t2",
                    "status": "PDNG",
                    "credit_debit_indicator": "DBIT",
                    "transaction_amount": { "amount": "99.00", "currency": "EUR" },
                    "booking_date": "2025-03-02",
                    "creditor": { "name": "Pending Shop" }
                }
            ]
        })))
        .mount(&server)
        .await;

    let provider = provider_with(&server.uri());
    let account_id = AccountId("acct-001".to_owned());
    let since = NaiveDate::from_ymd_opt(2025, 3, 1).unwrap();
    let txns = provider
        .fetch_transactions(&account_id, since)
        .await
        .unwrap();

    assert_eq!(txns.len(), 1);
    assert_eq!(txns[0].provider_transaction_id, "t1");
}

// ── Error handling ────────────────────────────────────────────────

#[tokio::test]
async fn unauthorized_maps_to_session_expired() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/accounts/acct-001/transactions"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let provider = provider_with(&server.uri());
    let account_id = AccountId("acct-001".to_owned());
    let since = NaiveDate::from_ymd_opt(2025, 3, 1).unwrap();
    let err = provider
        .fetch_transactions(&account_id, since)
        .await
        .unwrap_err();

    assert!(matches!(err, ProviderError::SessionExpired));
}

#[tokio::test]
async fn too_many_requests_maps_to_rate_limited() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/accounts/acct-001/balances"))
        .respond_with(ResponseTemplate::new(429))
        .mount(&server)
        .await;

    let provider = provider_with(&server.uri());
    let account_id = AccountId("acct-001".to_owned());
    let err = provider.get_balances(&account_id).await.unwrap_err();

    assert!(matches!(err, ProviderError::RateLimited));
}

#[tokio::test]
async fn server_error_maps_to_api_error() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/accounts/acct-001/balances"))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({
            "code": "INTERNAL",
            "message": "something broke"
        })))
        .mount(&server)
        .await;

    let provider = provider_with(&server.uri());
    let account_id = AccountId("acct-001".to_owned());
    let err = provider.get_balances(&account_id).await.unwrap_err();

    match err {
        ProviderError::ApiError { code, description } => {
            assert_eq!(code, "INTERNAL");
            assert_eq!(description, "something broke");
        }
        other => panic!("expected ApiError, got {other:?}"),
    }
}

// ── Balances ──────────────────────────────────────────────────────

#[tokio::test]
async fn balances_maps_types_correctly() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/accounts/acct-001/balances"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "balances": [
                {
                    "balance_amount": { "amount": "100.00", "currency": "EUR" },
                    "balance_type": "ITAV"
                },
                {
                    "balance_amount": { "amount": "200.00", "currency": "EUR" },
                    "balance_type": "CLAV"
                },
                {
                    "balance_amount": { "amount": "150.00", "currency": "EUR" },
                    "balance_type": "CLBD"
                }
            ]
        })))
        .mount(&server)
        .await;

    let provider = provider_with(&server.uri());
    let account_id = AccountId("acct-001".to_owned());
    let balance = provider.get_balances(&account_id).await.unwrap();

    // CLAV takes priority over ITAV for available
    assert_eq!(balance.available, dec!(200.00));
    assert_eq!(balance.current, dec!(150.00));
    assert_eq!(balance.currency, "EUR");
}

// ── Auth flow ─────────────────────────────────────────────────────

#[tokio::test]
async fn auth_start_authorization() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/auth"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "url": "https://bank.example.com/authorize?token=abc123"
        })))
        .mount(&server)
        .await;

    let config = test_config(&server.uri());
    let client = Client::new(config);
    let auth = EnableBankingAuth::new(client);

    let url = auth
        .start_authorization(
            "Test Bank",
            "FI",
            "https://app.example.com/callback",
            "state123",
            "2025-12-31T00:00:00Z",
            "personal",
        )
        .await
        .unwrap();

    assert_eq!(url, "https://bank.example.com/authorize?token=abc123");
}

#[tokio::test]
async fn auth_exchange_code() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/sessions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "session_id": "sess-new-001",
            "accounts": [
                {
                    "uid": "acct-new-001",
                    "name": "Savings",
                    "currency": "EUR",
                    "cash_account_type": "SVGS"
                }
            ]
        })))
        .mount(&server)
        .await;

    let config = test_config(&server.uri());
    let client = Client::new(config);
    let auth = EnableBankingAuth::new(client);

    let session = auth.exchange_code("auth-code-xyz").await.unwrap();
    assert_eq!(session.session_id, "sess-new-001");
    assert_eq!(session.accounts.len(), 1);
    assert_eq!(session.accounts[0].uid, "acct-new-001");
}

#[tokio::test]
async fn auth_revoke_session() {
    let server = MockServer::start().await;

    Mock::given(method("DELETE"))
        .and(path("/sessions/sess-to-revoke"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let config = test_config(&server.uri());
    let client = Client::new(config);
    let auth = EnableBankingAuth::new(client);

    auth.revoke_session("sess-to-revoke").await.unwrap();
}

// ── List accounts ─────────────────────────────────────────────────

#[tokio::test]
async fn list_accounts_from_session() {
    let provider = provider_with("http://unused");
    let accounts = provider.list_accounts().await.unwrap();

    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].provider_account_id, "acct-001");
    assert_eq!(accounts[0].name, "My Checking");
    assert_eq!(accounts[0].account_type, "checking");
    assert_eq!(accounts[0].currency, "EUR");
    assert_eq!(accounts[0].institution, "Test Bank");
}
