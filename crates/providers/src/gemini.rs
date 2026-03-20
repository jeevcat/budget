//! Gemini LLM provider — calls Google's Generative Language API for
//! transaction categorization, correlation, and rule proposal.

use serde::{Deserialize, Serialize};

use budget_core::models::MatchField;

use crate::error::ProviderError;
use crate::llm::{
    CategorizeInput, CategorizeResult, CorrelationResult, CorrelationType, LlmProvider,
    ProposedRule, RuleContext, TransactionSummary,
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

        tracing::debug!(prompt_len = prompt.len(), "gemini request");

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

        let text = parsed
            .candidates
            .into_iter()
            .next()
            .and_then(|c| c.content.parts.into_iter().next())
            .map(|p| p.text)
            .ok_or_else(|| ProviderError::Other("empty response from Gemini".to_owned()))?;

        tracing::debug!(response = %text, "gemini response");

        Ok(text)
    }
}

impl LlmProvider for GeminiProvider {
    async fn categorize(
        &self,
        input: &CategorizeInput<'_>,
    ) -> Result<CategorizeResult, ProviderError> {
        let merchant_name = input.merchant_name;
        let amount = input.amount;

        let desc_line = if input.remittance_information.is_empty() {
            String::new()
        } else {
            let joined = input.remittance_information.join(" / ");
            format!("Description: {joined}\n")
        };

        let btc_line = input
            .bank_transaction_code
            .map(|b| format!("Bank classification: {b}\n"))
            .unwrap_or_default();

        let cp_name_line = input
            .counterparty_name
            .map(|n| format!("Counterparty: {n}\n"))
            .unwrap_or_default();

        let cp_iban_line = input
            .counterparty_iban
            .map(|i| format!("Counterparty IBAN: {i}\n"))
            .unwrap_or_default();

        let cp_bic_line = input
            .counterparty_bic
            .map(|b| format!("Counterparty BIC: {b}\n"))
            .unwrap_or_default();

        let enrichment_line = if input.enrichment_item_titles.is_empty() {
            String::new()
        } else {
            let items = input.enrichment_item_titles.join(", ");
            format!("Known item details: {items}\n")
        };

        let categories_block = if input.existing_categories.is_empty() {
            "No categories exist yet. Propose a category using \"Parent:Child\" format with \":\" as separator \
             (e.g. \"Food:Groceries\", \"Transportation:Fuel\")."
                .to_owned()
        } else {
            let list = input.existing_categories.join(", ");
            format!(
                "You MUST choose exactly one of these existing categories for \"category_name\": {list}\n\
                 Do NOT invent new categories for \"category_name\". If none fit perfectly, pick the closest match and use a low confidence score.\n\
                 If you genuinely believe a new category is needed, set \"proposed_category\" to your suggestion using \"Parent:Child\" format — but \"category_name\" must still be one of the existing categories above."
            )
        };

        let prompt = format!(
            r#"You are a transaction categorization engine for a personal budgeting tool.
Transaction descriptions and merchant names are very likely in German.

Given a bank transaction, classify it into exactly one category.
Negative amounts are expenses/debits. Positive amounts are income/credits. Choose expense categories for negative amounts and income categories for positive amounts.

{categories_block}

Respond with a JSON object containing:
- "category_name": string — the category (MUST be from the existing list if one was provided)
- "confidence": number — your confidence from 0.0 to 1.0
- "justification": string — one brief sentence explaining why this category fits
- "proposed_category": string or null — only set this if none of the existing categories fit well; suggest a new "Parent:Child" category name
- "title": string — a concise 2-4 word human-friendly name for this transaction. Strip payment processor prefixes (VISA, SUMUP, BBMSL*), store numbers, location suffixes, and junk. Use the actual business name or purpose (e.g. "Tim Ho Wan" not "BBMSL*Tim Ho Wan Dim Su HK", "MTR Top-up" not "VISA MTR-KIOSK PAYMENT04100", "Rent" for a rent payment to a person). When "Known item details" are present (e.g. from Amazon or PayPal enrichment), prefer the actual product name(s) as the title if they are short enough (e.g. "Pixel 9 Pro" not "Amazon", "Mass Gainer" not "PayPal").

If you are unsure, use a low confidence score. Do not guess wildly.

Transaction:
Merchant: {merchant_name}
Amount: {amount}
{desc_line}{btc_line}{cp_name_line}{cp_iban_line}{cp_bic_line}{enrichment_line}
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
            justification: result.justification,
            proposed_category: result.proposed_category.filter(|s| !s.is_empty()),
            title: result.title.filter(|s| !s.is_empty()),
        })
    }

    async fn propose_correlation(
        &self,
        txn_a: &TransactionSummary,
        txn_b: &TransactionSummary,
    ) -> Result<CorrelationResult, ProviderError> {
        let desc_a = if txn_a.remittance_information.is_empty() {
            "(none)".to_owned()
        } else {
            txn_a.remittance_information.join(" / ")
        };
        let desc_b = if txn_b.remittance_information.is_empty() {
            "(none)".to_owned()
        } else {
            txn_b.remittance_information.join(" / ")
        };
        let cat_a = txn_a.category.as_deref().unwrap_or("(none)");
        let cat_b = txn_b.category.as_deref().unwrap_or("(none)");

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
Category: {}

Transaction B:
Merchant: {}
Amount: {}
Description: {}
Date: {}
Category: {}

JSON response:"#,
            txn_a.merchant_name,
            txn_a.amount,
            desc_a,
            txn_a.posted_date,
            cat_a,
            txn_b.merchant_name,
            txn_b.amount,
            desc_b,
            txn_b.posted_date,
            cat_b,
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

    async fn propose_rules(
        &self,
        context: &RuleContext,
    ) -> Result<Vec<ProposedRule>, ProviderError> {
        let prompt = build_rule_prompt(context);

        let text = self.generate(&prompt).await?;
        let results: Vec<RuleProposalResponse> = serde_json::from_str(&text).map_err(|e| {
            ProviderError::Other(format!(
                "failed to parse rule proposals response: {e} (raw: {text})"
            ))
        })?;

        Ok(results
            .into_iter()
            .map(|r| {
                let match_field = match r.match_field.as_str() {
                    "Description" => MatchField::Description,
                    "CounterpartyName" => MatchField::CounterpartyName,
                    "CounterpartyIban" => MatchField::CounterpartyIban,
                    "CounterpartyBic" => MatchField::CounterpartyBic,
                    "BankTransactionCode" => MatchField::BankTransactionCode,
                    "EnrichmentItemTitle" => MatchField::EnrichmentItemTitle,
                    _ => MatchField::Merchant,
                };
                ProposedRule {
                    match_field,
                    match_pattern: r.match_pattern,
                    explanation: r.explanation,
                }
            })
            .collect())
    }
}

/// Build the LLM prompt for rule proposals from a transaction context.
fn build_rule_prompt(context: &RuleContext) -> String {
    let siblings_block = if context.sibling_merchants.is_empty() {
        "No other merchants in this category.".to_owned()
    } else {
        context
            .sibling_merchants
            .iter()
            .map(|m| format!("- {m}"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let existing_rules_block = if context.existing_rule_patterns.is_empty() {
        "No existing rules for this category.".to_owned()
    } else {
        context
            .existing_rule_patterns
            .iter()
            .map(|p| format!("- {p}"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let cp_name_line = context
        .counterparty_name
        .as_deref()
        .map(|n| format!("Counterparty: {n}\n"))
        .unwrap_or_default();
    let cp_iban_line = context
        .counterparty_iban
        .as_deref()
        .map(|i| format!("Counterparty IBAN: {i}\n"))
        .unwrap_or_default();
    let cp_bic_line = context
        .counterparty_bic
        .as_deref()
        .map(|b| format!("Counterparty BIC: {b}\n"))
        .unwrap_or_default();
    let btc_line = context
        .bank_transaction_code
        .as_deref()
        .map(|b| format!("Bank classification: {b}\n"))
        .unwrap_or_default();
    let enrichment_line = if context.enrichment_item_titles.is_empty() {
        String::new()
    } else {
        let items = context.enrichment_item_titles.join(", ");
        format!("Known item details: {items}\n")
    };

    format!(
        r#"You propose categorization rules for a personal budgeting tool. A rule has a field to match on and a regex pattern. When a transaction matches the pattern, it is automatically assigned the category.

Given this transaction and its category, propose up to 3 rules that would correctly categorize this and similar future transactions.

Available fields:
- "Merchant" — regex against the merchant/payee name
- "Description" — regex against the transaction description
- "CounterpartyName" — regex against the counterparty name
- "CounterpartyIban" — regex against the counterparty IBAN
- "CounterpartyBic" — regex against the counterparty BIC
- "BankTransactionCode" — regex against the bank transaction code
- "EnrichmentItemTitle" — regex against enrichment item titles (from Amazon orders, PayPal items, etc.)

Guidelines:
- Only propose a rule if the pattern meaningfully identifies this payee or transaction type. Never use broad patterns like ".*", ".+", or patterns that match most transactions.
- Only use a field if the transaction has useful data for it. If a field is absent or its value is too generic to be discriminating, skip it.
- Only use data that is literally present in the transaction fields. Never invent name variants, abbreviations, or alternate spellings that do not appear in the transaction.
- Prefer the field that most precisely identifies the payee. An exact IBAN is better than a merchant name substring, but a broad IBAN pattern like "^DE.*" is useless.
- Consider the sibling merchants — if a pattern would also match them, that's good. If it would match unrelated merchants, it's too broad.
- For Description rules, match on meaningful words (business names, product types like "hotel", "insurance", etc.). Do NOT match on banking metadata such as "remittanceinformation:", "KAUFUMSATZ", "Lastschrift", reference numbers, or other bank-internal fields — these are brittle and do not generalize.
- It is fine to propose fewer than 3 rules if you cannot find 3 precise ones.

Respond with a JSON array of objects, each containing:
- "match_field": one of "Merchant", "Description", "CounterpartyName", "CounterpartyIban", "CounterpartyBic", "BankTransactionCode", "EnrichmentItemTitle"
- "match_pattern": string — regex pattern (case-insensitive matching is applied automatically, do not include (?i) flags)
- "explanation": string — brief explanation of what the rule matches

Transaction:
Merchant: {merchant}
Description: {remittance_info}
Amount: {amount}
Date: {date}
{cp_name_line}{cp_iban_line}{cp_bic_line}{btc_line}{enrichment_line}Category: {category}

Other merchants in this category:
{siblings_block}

Existing rules for this category (avoid duplicating these):
{existing_rules_block}

JSON response:"#,
        merchant = context.merchant_name,
        remittance_info = if context.remittance_information.is_empty() {
            "(none)".to_owned()
        } else {
            context.remittance_information.join(" / ")
        },
        amount = context.amount,
        date = context.posted_date,
        category = context.category_name,
    )
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
    #[serde(default)]
    justification: String,
    #[serde(default)]
    proposed_category: Option<String>,
    #[serde(default)]
    title: Option<String>,
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
    use crate::llm::RuleContext;

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
            .categorize(&CategorizeInput {
                merchant_name: "WHOLE FOODS MARKET",
                amount: dec!(72.34),
                remittance_information: &["Weekly groceries".to_owned()],
                existing_categories: &[],
                bank_transaction_code: None,
                counterparty_name: None,
                counterparty_iban: None,
                counterparty_bic: None,
                enrichment_item_titles: &[],
            })
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
            .categorize(&CategorizeInput {
                merchant_name: "AMAZON",
                amount: dec!(25.00),
                remittance_information: &[],
                existing_categories: &[],
                bank_transaction_code: None,
                counterparty_name: None,
                counterparty_iban: None,
                counterparty_bic: None,
                enrichment_item_titles: &[],
            })
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
            remittance_information: vec!["Credit card payment".to_owned()],
            posted_date: NaiveDate::from_ymd_opt(2025, 1, 20).expect("valid date"),
            category: None,
        };
        let txn_b = TransactionSummary {
            merchant_name: "PAYMENT RECEIVED".to_owned(),
            amount: dec!(1500.00),
            remittance_information: vec!["Thank you".to_owned()],
            posted_date: NaiveDate::from_ymd_opt(2025, 1, 20).expect("valid date"),
            category: None,
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
            remittance_information: vec![],
            posted_date: NaiveDate::from_ymd_opt(2025, 1, 22).expect("valid date"),
            category: None,
        };
        let txn_b = TransactionSummary {
            merchant_name: "TARGET".to_owned(),
            amount: dec!(-65.00),
            remittance_information: vec![],
            posted_date: NaiveDate::from_ymd_opt(2025, 1, 21).expect("valid date"),
            category: None,
        };

        let result = provider.propose_correlation(&txn_a, &txn_b).await.unwrap();
        assert!(result.correlation_type.is_none());
    }

    #[tokio::test]
    async fn propose_rules_parses_response() {
        let (server, provider) = setup().await;

        Mock::given(method("POST"))
            .and(path_regex(r"/v1beta/models/.+:generateContent"))
            .respond_with(ResponseTemplate::new(200).set_body_json(gemini_json_response(
                r#"[{"match_field": "Merchant", "match_pattern": "^WHOLE FOODS MARKET$", "explanation": "Exact match"},{"match_field": "Merchant", "match_pattern": "whole\\s*foods", "explanation": "Matches Whole Foods variations"},{"match_field": "Merchant", "match_pattern": "foods|grocery", "explanation": "Broad grocery match"}]"#,
            )))
            .mount(&server)
            .await;

        let context = RuleContext {
            merchant_name: "WHOLE FOODS MARKET".to_owned(),
            remittance_information: vec!["Groceries".to_owned()],
            amount: dec!(72.34),
            posted_date: NaiveDate::from_ymd_opt(2025, 1, 15).expect("valid date"),
            category_name: "Food:Groceries".to_owned(),
            sibling_merchants: vec!["TRADER JOE'S".to_owned()],
            existing_rule_patterns: vec![],
            counterparty_name: None,
            counterparty_iban: None,
            counterparty_bic: None,
            bank_transaction_code: None,
            enrichment_item_titles: vec![],
        };

        let results = provider.propose_rules(&context).await.unwrap();
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].match_field, MatchField::Merchant);
        assert!(results[0].match_pattern.contains("WHOLE FOODS"));
        assert!(!results[2].explanation.is_empty());
    }

    #[tokio::test]
    async fn auth_error_returns_authentication_failed() {
        let (server, provider) = setup().await;

        Mock::given(method("POST"))
            .and(path_regex(r"/v1beta/models/.+:generateContent"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;

        let result = provider
            .categorize(&CategorizeInput {
                merchant_name: "TEST",
                amount: dec!(10.00),
                remittance_information: &[],
                existing_categories: &[],
                bank_transaction_code: None,
                counterparty_name: None,
                counterparty_iban: None,
                counterparty_bic: None,
                enrichment_item_titles: &[],
            })
            .await;
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

        let result = provider
            .categorize(&CategorizeInput {
                merchant_name: "TEST",
                amount: dec!(10.00),
                remittance_information: &[],
                existing_categories: &[],
                bank_transaction_code: None,
                counterparty_name: None,
                counterparty_iban: None,
                counterparty_bic: None,
                enrichment_item_titles: &[],
            })
            .await;
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

        let result = provider
            .categorize(&CategorizeInput {
                merchant_name: "TEST",
                amount: dec!(10.00),
                remittance_information: &[],
                existing_categories: &[],
                bank_transaction_code: None,
                counterparty_name: None,
                counterparty_iban: None,
                counterparty_bic: None,
                enrichment_item_titles: &[],
            })
            .await;
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

        let result = provider
            .categorize(&CategorizeInput {
                merchant_name: "TEST",
                amount: dec!(10.00),
                remittance_information: &[],
                existing_categories: &[],
                bank_transaction_code: None,
                counterparty_name: None,
                counterparty_iban: None,
                counterparty_bic: None,
                enrichment_item_titles: &[],
            })
            .await;
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

        let result = provider
            .categorize(&CategorizeInput {
                merchant_name: "TEST",
                amount: dec!(10.00),
                remittance_information: &[],
                existing_categories: &[],
                bank_transaction_code: None,
                counterparty_name: None,
                counterparty_iban: None,
                counterparty_bic: None,
                enrichment_item_titles: &[],
            })
            .await;
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
    use crate::llm::RuleContext;

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
            .categorize(&CategorizeInput {
                merchant_name: "WHOLE FOODS MARKET #10234",
                amount: dec!(87.43),
                remittance_information: &["Groceries".to_owned()],
                existing_categories: &[],
                bank_transaction_code: None,
                counterparty_name: None,
                counterparty_iban: None,
                counterparty_bic: None,
                enrichment_item_titles: &[],
            })
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
            .categorize(&CategorizeInput {
                merchant_name: "NETFLIX.COM",
                amount: dec!(15.99),
                remittance_information: &["Monthly subscription".to_owned()],
                existing_categories: &[],
                bank_transaction_code: None,
                counterparty_name: None,
                counterparty_iban: None,
                counterparty_bic: None,
                enrichment_item_titles: &[],
            })
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
            remittance_information: vec!["Automatic payment to credit card".to_owned()],
            posted_date: NaiveDate::from_ymd_opt(2025, 3, 15).expect("valid date"),
            category: None,
        };
        let txn_b = TransactionSummary {
            merchant_name: "PAYMENT THANK YOU".to_owned(),
            amount: dec!(1500.00),
            remittance_information: vec!["Payment received".to_owned()],
            posted_date: NaiveDate::from_ymd_opt(2025, 3, 15).expect("valid date"),
            category: None,
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
    async fn live_propose_rules() {
        let provider = require_provider();
        let context = RuleContext {
            merchant_name: "TRADER JOE'S #123".to_owned(),
            remittance_information: vec!["Grocery purchase".to_owned()],
            amount: dec!(58.12),
            posted_date: NaiveDate::from_ymd_opt(2025, 3, 15).expect("valid date"),
            category_name: "Food:Groceries".to_owned(),
            sibling_merchants: vec!["WHOLE FOODS MARKET".to_owned()],
            existing_rule_patterns: vec![],
            counterparty_name: None,
            counterparty_iban: None,
            counterparty_bic: None,
            bank_transaction_code: None,
            enrichment_item_titles: vec![],
        };
        let results = provider.propose_rules(&context).await.unwrap();

        for r in &results {
            println!(
                "rule: {:?} match on '{}' — {}",
                r.match_field, r.match_pattern, r.explanation
            );
        }
        assert_eq!(results.len(), 3);
        assert!(!results[0].match_pattern.is_empty());
    }
}
