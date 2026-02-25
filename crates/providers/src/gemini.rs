//! Gemini LLM provider — calls Google's Generative Language API for
//! transaction categorization, correlation, and rule proposal.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::error::ProviderError;
use crate::llm::{
    CategorizeResult, CorrelationResult, CorrelationType, LlmProvider, MatchField, ProposedRule,
    TransactionSummary,
};

const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com";

/// A real LLM provider backed by the Gemini REST API.
pub struct GeminiProvider {
    http: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
}

impl GeminiProvider {
    #[must_use]
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            api_key,
            model,
            base_url: DEFAULT_BASE_URL.to_owned(),
        }
    }

    /// Override the base URL (for testing against a mock server).
    #[cfg(test)]
    fn with_base_url(mut self, base_url: String) -> Self {
        self.base_url = base_url;
        self
    }

    /// Send a prompt to Gemini and return the raw text response.
    async fn generate(&self, prompt: &str) -> Result<String, ProviderError> {
        let url = format!(
            "{}/v1beta/models/{}:generateContent",
            self.base_url, self.model
        );

        let body = GenerateRequest {
            contents: vec![Content {
                parts: vec![Part {
                    text: prompt.to_owned(),
                }],
            }],
            generation_config: GenerationConfig {
                response_mime_type: "application/json".to_owned(),
                temperature: 0.1,
            },
        };

        let response = self
            .http
            .post(&url)
            .query(&[("key", &self.api_key)])
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::ConnectionFailed(e.to_string()))?;

        let status = response.status();

        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(ProviderError::AuthenticationFailed(
                "invalid Gemini API key".to_owned(),
            ));
        }

        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(ProviderError::RateLimited);
        }

        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(ProviderError::ApiError {
                code: status.as_u16().to_string(),
                description: text,
            });
        }

        let parsed: GenerateResponse = response
            .json()
            .await
            .map_err(|e| ProviderError::Other(format!("failed to parse Gemini response: {e}")))?;

        parsed
            .candidates
            .into_iter()
            .next()
            .and_then(|c| c.content.parts.into_iter().next())
            .map(|p| p.text)
            .ok_or_else(|| ProviderError::Other("empty response from Gemini".to_owned()))
    }
}

impl LlmProvider for GeminiProvider {
    async fn categorize(
        &self,
        merchant_name: &str,
        amount: Decimal,
        description: Option<&str>,
        existing_categories: &[String],
    ) -> Result<CategorizeResult, ProviderError> {
        let desc_line = description
            .map(|d| format!("Description: {d}\n"))
            .unwrap_or_default();

        let categories_block = if existing_categories.is_empty() {
            "Use hierarchical categories with \":\" as separator. Common examples:\n\
             - Food:Groceries, Food:Restaurants, Food:Coffee\n\
             - Housing:Rent, Housing:Utilities, Housing:Insurance\n\
             - Transportation:Gas, Transportation:Public Transit, Transportation:Parking\n\
             - Entertainment:Subscriptions, Entertainment:Movies\n\
             - Shopping, Shopping:Clothing, Shopping:Electronics\n\
             - Health:Pharmacy, Health:Doctor\n\
             - Income:Salary, Income:Freelance\n\
             - Transfers:P2P\n\
             - Cash"
                .to_owned()
        } else {
            let list = existing_categories.join(", ");
            format!(
                "You MUST use one of these existing categories: {list}\n\
                 If none of these fit, you may propose a new hierarchical category using \":\" as separator, but prefer existing ones."
            )
        };

        let prompt = format!(
            r#"You are a transaction categorization engine for a personal budgeting tool.

Given a bank transaction, classify it into exactly one category.
{categories_block}

Respond with a JSON object containing:
- "category_name": string — the category (use ":" for hierarchy)
- "confidence": number — your confidence from 0.0 to 1.0

If you are unsure, use a low confidence score. Do not guess wildly.

Transaction:
Merchant: {merchant_name}
Amount: {amount}
{desc_line}
JSON response:"#
        );

        let text = self.generate(&prompt).await?;
        let result: CategorizeResponse = serde_json::from_str(&text).map_err(|e| {
            ProviderError::Other(format!(
                "failed to parse categorize response: {e} (raw: {text})"
            ))
        })?;

        Ok(CategorizeResult {
            category_name: result.category_name,
            confidence: result.confidence.clamp(0.0, 1.0),
        })
    }

    async fn propose_correlation(
        &self,
        txn_a: &TransactionSummary,
        txn_b: &TransactionSummary,
    ) -> Result<CorrelationResult, ProviderError> {
        let desc_a = txn_a.description.as_deref().unwrap_or("(none)");
        let desc_b = txn_b.description.as_deref().unwrap_or("(none)");

        let prompt = format!(
            r#"You analyze pairs of financial transactions to determine if they represent the same money movement across accounts.

Types of correlations:
- "Transfer": money moving between the user's own accounts (e.g., credit card payment from checking, savings transfer). These net to zero.
- "Reimbursement": an incoming deposit that offsets a prior expense (e.g., insurance reimbursement, expense report payout).

If the transactions are unrelated, correlation_type should be null.

Respond with a JSON object containing:
- "correlation_type": "Transfer", "Reimbursement", or null
- "confidence": number from 0.0 to 1.0

Transaction A:
Merchant: {}
Amount: {}
Description: {}
Date: {}

Transaction B:
Merchant: {}
Amount: {}
Description: {}
Date: {}

JSON response:"#,
            txn_a.merchant_name,
            txn_a.amount,
            desc_a,
            txn_a.posted_date,
            txn_b.merchant_name,
            txn_b.amount,
            desc_b,
            txn_b.posted_date,
        );

        let text = self.generate(&prompt).await?;
        let result: CorrelationResponse = serde_json::from_str(&text).map_err(|e| {
            ProviderError::Other(format!(
                "failed to parse correlation response: {e} (raw: {text})"
            ))
        })?;

        let correlation_type = match result.correlation_type.as_deref() {
            Some("Transfer") => Some(CorrelationType::Transfer),
            Some("Reimbursement") => Some(CorrelationType::Reimbursement),
            _ => None,
        };

        Ok(CorrelationResult {
            correlation_type,
            confidence: result.confidence.clamp(0.0, 1.0),
        })
    }

    async fn propose_rule(
        &self,
        merchant_name: &str,
        user_category: &str,
    ) -> Result<ProposedRule, ProviderError> {
        let prompt = format!(
            r#"You propose deterministic categorization rules for a personal budgeting tool.

The user has categorized a transaction and you need to propose a rule that would automatically categorize similar transactions in the future.

Rules can match on:
- "Merchant" — match against the merchant/payee name
- "Description" — match against the transaction description

The match_pattern should be a regex pattern that would catch this merchant and similar variations. Keep it simple and precise — avoid overly broad patterns.

Respond with a JSON object containing:
- "match_field": "Merchant" or "Description"
- "match_pattern": string — a regex pattern
- "explanation": string — brief explanation of the rule

Merchant: {merchant_name}
Category: {user_category}

JSON response:"#
        );

        let text = self.generate(&prompt).await?;
        let result: RuleProposalResponse = serde_json::from_str(&text).map_err(|e| {
            ProviderError::Other(format!(
                "failed to parse rule proposal response: {e} (raw: {text})"
            ))
        })?;

        let match_field = match result.match_field.as_str() {
            "Description" => MatchField::Description,
            _ => MatchField::Merchant,
        };

        Ok(ProposedRule {
            match_field,
            match_pattern: result.match_pattern,
            explanation: result.explanation,
        })
    }
}

// ---------------------------------------------------------------------------
// Gemini API request/response types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct GenerateRequest {
    contents: Vec<Content>,
    #[serde(rename = "generationConfig")]
    generation_config: GenerationConfig,
}

#[derive(Serialize)]
struct Content {
    parts: Vec<Part>,
}

#[derive(Serialize, Deserialize)]
struct Part {
    text: String,
}

#[derive(Serialize)]
struct GenerationConfig {
    #[serde(rename = "responseMimeType")]
    response_mime_type: String,
    temperature: f64,
}

#[derive(Deserialize)]
struct GenerateResponse {
    candidates: Vec<Candidate>,
}

#[derive(Deserialize)]
struct Candidate {
    content: CandidateContent,
}

#[derive(Deserialize)]
struct CandidateContent {
    parts: Vec<Part>,
}

// Response schemas parsed from Gemini's JSON output

#[derive(Deserialize)]
struct CategorizeResponse {
    category_name: String,
    confidence: f64,
}

#[derive(Deserialize)]
struct CorrelationResponse {
    correlation_type: Option<String>,
    confidence: f64,
}

#[derive(Deserialize)]
struct RuleProposalResponse {
    match_field: String,
    match_pattern: String,
    explanation: String,
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;
    use rust_decimal_macros::dec;
    use wiremock::matchers::{method, path_regex};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    fn gemini_json_response(json_text: &str) -> serde_json::Value {
        serde_json::json!({
            "candidates": [{
                "content": {
                    "parts": [{"text": json_text}]
                }
            }]
        })
    }

    async fn setup() -> (MockServer, GeminiProvider) {
        let server = MockServer::start().await;
        let provider = GeminiProvider::new("test-key".to_owned(), "test-model".to_owned())
            .with_base_url(server.uri());
        (server, provider)
    }

    #[tokio::test]
    async fn categorize_parses_response() {
        let (server, provider) = setup().await;

        Mock::given(method("POST"))
            .and(path_regex(r"/v1beta/models/.+:generateContent"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(gemini_json_response(
                    r#"{"category_name": "Food:Groceries", "confidence": 0.92}"#,
                )),
            )
            .mount(&server)
            .await;

        let result = provider
            .categorize(
                "WHOLE FOODS MARKET",
                dec!(72.34),
                Some("Weekly groceries"),
                &[],
            )
            .await
            .unwrap();
        assert_eq!(result.category_name, "Food:Groceries");
        assert!((result.confidence - 0.92).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn categorize_clamps_confidence() {
        let (server, provider) = setup().await;

        Mock::given(method("POST"))
            .and(path_regex(r"/v1beta/models/.+:generateContent"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(gemini_json_response(
                    r#"{"category_name": "Shopping", "confidence": 1.5}"#,
                )),
            )
            .mount(&server)
            .await;

        let result = provider
            .categorize("AMAZON", dec!(25.00), None, &[])
            .await
            .unwrap();
        assert!((result.confidence - 1.0).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn propose_correlation_parses_transfer() {
        let (server, provider) = setup().await;

        Mock::given(method("POST"))
            .and(path_regex(r"/v1beta/models/.+:generateContent"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(gemini_json_response(
                    r#"{"correlation_type": "Transfer", "confidence": 0.95}"#,
                )),
            )
            .mount(&server)
            .await;

        let txn_a = TransactionSummary {
            merchant_name: "CHASE CREDIT CRD AUTOPAY".to_owned(),
            amount: dec!(-1500.00),
            description: Some("Credit card payment".to_owned()),
            posted_date: NaiveDate::from_ymd_opt(2025, 1, 20).expect("valid date"),
        };
        let txn_b = TransactionSummary {
            merchant_name: "PAYMENT RECEIVED".to_owned(),
            amount: dec!(1500.00),
            description: Some("Thank you".to_owned()),
            posted_date: NaiveDate::from_ymd_opt(2025, 1, 20).expect("valid date"),
        };

        let result = provider.propose_correlation(&txn_a, &txn_b).await.unwrap();
        assert_eq!(result.correlation_type, Some(CorrelationType::Transfer));
        assert!(result.confidence > 0.9);
    }

    #[tokio::test]
    async fn propose_correlation_parses_null_type() {
        let (server, provider) = setup().await;

        Mock::given(method("POST"))
            .and(path_regex(r"/v1beta/models/.+:generateContent"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(gemini_json_response(
                    r#"{"correlation_type": null, "confidence": 0.1}"#,
                )),
            )
            .mount(&server)
            .await;

        let txn_a = TransactionSummary {
            merchant_name: "AMAZON".to_owned(),
            amount: dec!(-45.99),
            description: None,
            posted_date: NaiveDate::from_ymd_opt(2025, 1, 22).expect("valid date"),
        };
        let txn_b = TransactionSummary {
            merchant_name: "TARGET".to_owned(),
            amount: dec!(-65.00),
            description: None,
            posted_date: NaiveDate::from_ymd_opt(2025, 1, 21).expect("valid date"),
        };

        let result = provider.propose_correlation(&txn_a, &txn_b).await.unwrap();
        assert!(result.correlation_type.is_none());
    }

    #[tokio::test]
    async fn propose_rule_parses_response() {
        let (server, provider) = setup().await;

        Mock::given(method("POST"))
            .and(path_regex(r"/v1beta/models/.+:generateContent"))
            .respond_with(ResponseTemplate::new(200).set_body_json(gemini_json_response(
                r#"{"match_field": "Merchant", "match_pattern": "(?i)whole\\s*foods", "explanation": "Matches Whole Foods variations"}"#,
            )))
            .mount(&server)
            .await;

        let result = provider
            .propose_rule("WHOLE FOODS MARKET", "Food:Groceries")
            .await
            .unwrap();
        assert_eq!(result.match_field, MatchField::Merchant);
        assert!(result.match_pattern.contains("whole"));
        assert!(!result.explanation.is_empty());
    }

    #[tokio::test]
    async fn auth_error_returns_authentication_failed() {
        let (server, provider) = setup().await;

        Mock::given(method("POST"))
            .and(path_regex(r"/v1beta/models/.+:generateContent"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;

        let result = provider.categorize("TEST", dec!(10.00), None, &[]).await;
        assert!(matches!(
            result,
            Err(ProviderError::AuthenticationFailed(_))
        ));
    }

    #[tokio::test]
    async fn rate_limit_returns_rate_limited() {
        let (server, provider) = setup().await;

        Mock::given(method("POST"))
            .and(path_regex(r"/v1beta/models/.+:generateContent"))
            .respond_with(ResponseTemplate::new(429))
            .mount(&server)
            .await;

        let result = provider.categorize("TEST", dec!(10.00), None, &[]).await;
        assert!(matches!(result, Err(ProviderError::RateLimited)));
    }

    #[tokio::test]
    async fn server_error_returns_api_error() {
        let (server, provider) = setup().await;

        Mock::given(method("POST"))
            .and(path_regex(r"/v1beta/models/.+:generateContent"))
            .respond_with(ResponseTemplate::new(500).set_body_string("internal error"))
            .mount(&server)
            .await;

        let result = provider.categorize("TEST", dec!(10.00), None, &[]).await;
        assert!(matches!(result, Err(ProviderError::ApiError { .. })));
    }

    #[tokio::test]
    async fn empty_candidates_returns_error() {
        let (server, provider) = setup().await;

        Mock::given(method("POST"))
            .and(path_regex(r"/v1beta/models/.+:generateContent"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"candidates": []})),
            )
            .mount(&server)
            .await;

        let result = provider.categorize("TEST", dec!(10.00), None, &[]).await;
        assert!(matches!(result, Err(ProviderError::Other(_))));
    }

    #[tokio::test]
    async fn malformed_json_in_response_returns_error() {
        let (server, provider) = setup().await;

        Mock::given(method("POST"))
            .and(path_regex(r"/v1beta/models/.+:generateContent"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(gemini_json_response(r"not valid json at all")),
            )
            .mount(&server)
            .await;

        let result = provider.categorize("TEST", dec!(10.00), None, &[]).await;
        assert!(matches!(result, Err(ProviderError::Other(_))));
    }
}

#[cfg(test)]
mod live_tests {
    //! Live tests that hit the real Gemini API.
    //!
    //! Run with: `cargo test -p budget-providers gemini::live_tests -- --ignored --nocapture`

    use chrono::NaiveDate;
    use rust_decimal_macros::dec;

    use super::*;

    fn require_provider() -> GeminiProvider {
        let config = budget_core::load_config().expect("failed to load budget config");
        let api_key = config
            .gemini_api_key
            .expect("gemini_api_key not set in config");
        GeminiProvider::new(api_key, config.llm_model)
    }

    #[tokio::test]
    #[ignore = "hits live Gemini API"]
    async fn live_categorize_grocery() {
        let provider = require_provider();
        let result = provider
            .categorize(
                "WHOLE FOODS MARKET #10234",
                dec!(87.43),
                Some("Groceries"),
                &[],
            )
            .await
            .unwrap();

        println!(
            "category: {} (confidence: {})",
            result.category_name, result.confidence
        );
        assert!(result.confidence > 0.5);
        assert!(
            result.category_name.to_lowercase().contains("grocer")
                || result.category_name.to_lowercase().contains("food"),
            "expected grocery-related category, got: {}",
            result.category_name
        );
    }

    #[tokio::test]
    #[ignore = "hits live Gemini API"]
    async fn live_categorize_subscription() {
        let provider = require_provider();
        let result = provider
            .categorize(
                "NETFLIX.COM",
                dec!(15.99),
                Some("Monthly subscription"),
                &[],
            )
            .await
            .unwrap();

        println!(
            "category: {} (confidence: {})",
            result.category_name, result.confidence
        );
        assert!(result.confidence > 0.5);
    }

    #[tokio::test]
    #[ignore = "hits live Gemini API"]
    async fn live_correlate_transfer_pair() {
        let provider = require_provider();
        let txn_a = TransactionSummary {
            merchant_name: "CHASE CREDIT CRD AUTOPAY".to_owned(),
            amount: dec!(-1500.00),
            description: Some("Automatic payment to credit card".to_owned()),
            posted_date: NaiveDate::from_ymd_opt(2025, 3, 15).expect("valid date"),
        };
        let txn_b = TransactionSummary {
            merchant_name: "PAYMENT THANK YOU".to_owned(),
            amount: dec!(1500.00),
            description: Some("Payment received".to_owned()),
            posted_date: NaiveDate::from_ymd_opt(2025, 3, 15).expect("valid date"),
        };

        let result = provider.propose_correlation(&txn_a, &txn_b).await.unwrap();

        println!(
            "correlation: {:?} (confidence: {})",
            result.correlation_type, result.confidence
        );
        assert_eq!(result.correlation_type, Some(CorrelationType::Transfer));
        assert!(result.confidence > 0.5);
    }

    #[tokio::test]
    #[ignore = "hits live Gemini API"]
    async fn live_propose_rule() {
        let provider = require_provider();
        let result = provider
            .propose_rule("TRADER JOE'S #123", "Food:Groceries")
            .await
            .unwrap();

        println!(
            "rule: {:?} match on '{}' — {}",
            result.match_field, result.match_pattern, result.explanation
        );
        assert!(!result.match_pattern.is_empty());
        assert!(!result.explanation.is_empty());
    }
}
