use budget_core::models::MatchField;

use crate::error::ProviderError;
use crate::llm::{
    CategorizeInput, CategorizeResult, CorrelationResult, CorrelationType, LlmProvider,
    ProposedRule, RuleContext, TransactionSummary,
};

/// A mock LLM provider that uses simple keyword matching for categorization
/// and basic heuristics for correlation.
pub struct MockLlmProvider;

impl MockLlmProvider {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for MockLlmProvider {
    fn default() -> Self {
        Self::new()
    }
}

/// Keyword-to-category mapping with associated confidence.
struct CategoryRule {
    keywords: &'static [&'static str],
    category: &'static str,
    confidence: f64,
}

const CATEGORY_RULES: &[CategoryRule] = &[
    CategoryRule {
        keywords: &[
            "WHOLE FOODS",
            "TRADER JOE",
            "COSTCO",
            "GROCERY",
            "GROCERIES",
        ],
        category: "Food:Groceries",
        confidence: 0.92,
    },
    CategoryRule {
        keywords: &[
            "CHIPOTLE",
            "CHEESECAKE FACTORY",
            "RESTAURANT",
            "DINER",
            "CAFE",
        ],
        category: "Food:Restaurants",
        confidence: 0.90,
    },
    CategoryRule {
        keywords: &["NETFLIX", "SPOTIFY", "HULU", "DISNEY+", "SUBSCRIPTION"],
        category: "Entertainment:Subscriptions",
        confidence: 0.95,
    },
    CategoryRule {
        keywords: &["AMAZON"],
        category: "Shopping",
        confidence: 0.70,
    },
    CategoryRule {
        keywords: &["TARGET", "WALMART", "CLOTHING"],
        category: "Shopping:Clothing",
        confidence: 0.75,
    },
    CategoryRule {
        keywords: &["SHELL", "EXXON", "CHEVRON", "BP", "GAS"],
        category: "Transportation:Gas",
        confidence: 0.93,
    },
    CategoryRule {
        keywords: &["INSURANCE"],
        category: "Insurance",
        confidence: 0.88,
    },
    CategoryRule {
        keywords: &["WATER", "ELECTRIC", "UTILITY", "PG&E"],
        category: "Housing:Utilities",
        confidence: 0.90,
    },
    CategoryRule {
        keywords: &["RENT", "APARTMENT", "MORTGAGE"],
        category: "Housing:Rent",
        confidence: 0.95,
    },
    CategoryRule {
        keywords: &["PAYROLL", "SALARY", "DIRECT DEPOSIT"],
        category: "Income:Salary",
        confidence: 0.97,
    },
    CategoryRule {
        keywords: &["ATM"],
        category: "Cash",
        confidence: 0.85,
    },
    CategoryRule {
        keywords: &["VENMO", "ZELLE", "PAYPAL"],
        category: "Transfers:P2P",
        confidence: 0.60,
    },
];

fn match_category(merchant_upper: &str) -> Option<(&'static str, f64)> {
    for rule in CATEGORY_RULES {
        if rule.keywords.iter().any(|kw| merchant_upper.contains(kw)) {
            return Some((rule.category, rule.confidence));
        }
    }
    None
}

impl LlmProvider for MockLlmProvider {
    async fn categorize(
        &self,
        input: &CategorizeInput<'_>,
    ) -> Result<CategorizeResult, ProviderError> {
        let merchant_upper = input.merchant_name.to_uppercase();

        let (category, confidence) =
            match_category(&merchant_upper).unwrap_or(("Uncategorized", 0.15));

        Ok(CategorizeResult {
            category_name: category.to_owned(),
            confidence,
            justification: format!("Matched merchant \"{merchant_upper}\" to {category}"),
            proposed_category: None,
            title: Some(input.merchant_name.to_owned()),
        })
    }

    async fn propose_correlation(
        &self,
        txn_a: &TransactionSummary,
        txn_b: &TransactionSummary,
    ) -> Result<CorrelationResult, ProviderError> {
        // Equal and opposite amounts suggest a transfer
        let amounts_cancel = txn_a.amount == -txn_b.amount;
        // Same-day or close dates strengthen the signal
        let days_apart = (txn_a.posted_date - txn_b.posted_date).num_days().abs();
        let close_in_time = days_apart <= 3;

        if amounts_cancel && close_in_time {
            Ok(CorrelationResult {
                correlation_type: Some(CorrelationType::Transfer),
                confidence: 0.95,
            })
        } else if amounts_cancel {
            Ok(CorrelationResult {
                correlation_type: Some(CorrelationType::Transfer),
                confidence: 0.70,
            })
        } else {
            Ok(CorrelationResult {
                correlation_type: None,
                confidence: 0.0,
            })
        }
    }

    async fn propose_rules(
        &self,
        context: &RuleContext,
    ) -> Result<Vec<ProposedRule>, ProviderError> {
        let merchant = &context.merchant_name;
        // Tight: exact match
        let exact = format!("^{merchant}$");
        // Medium: simplified prefix
        let simplified = merchant
            .split_whitespace()
            .next()
            .unwrap_or(merchant)
            .to_owned();
        // Broad: case-insensitive keyword
        let broad = merchant
            .split_whitespace()
            .next()
            .unwrap_or(merchant)
            .to_lowercase();

        Ok(vec![
            ProposedRule {
                match_field: MatchField::Merchant,
                match_pattern: exact,
                explanation: format!("Exact match for \"{merchant}\""),
            },
            ProposedRule {
                match_field: MatchField::Merchant,
                match_pattern: simplified,
                explanation: "Matches merchants starting with the same name".to_owned(),
            },
            ProposedRule {
                match_field: MatchField::Merchant,
                match_pattern: broad,
                explanation: "Broad match for similar merchants".to_owned(),
            },
        ])
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::llm::{CategorizeInput, RuleContext};

    fn cat_input(merchant_name: &str) -> CategorizeInput<'_> {
        CategorizeInput {
            merchant_name,
            amount: dec!(0),
            remittance_information: &[],
            existing_categories: &[],
            bank_transaction_code: None,
            counterparty_name: None,
            counterparty_iban: None,
            counterparty_bic: None,
            enrichment_item_titles: &[],
        }
    }

    #[tokio::test]
    async fn categorize_grocery_merchant() {
        let provider = MockLlmProvider::new();
        let result = provider
            .categorize(&cat_input("WHOLE FOODS MARKET"))
            .await
            .unwrap();
        assert_eq!(result.category_name, "Food:Groceries");
        assert!(result.confidence > 0.8);
    }

    #[tokio::test]
    async fn categorize_restaurant() {
        let provider = MockLlmProvider::new();
        let result = provider
            .categorize(&cat_input("CHIPOTLE MEXICAN GRILL"))
            .await
            .unwrap();
        assert_eq!(result.category_name, "Food:Restaurants");
        assert!(result.confidence > 0.8);
    }

    #[tokio::test]
    async fn categorize_subscription() {
        let provider = MockLlmProvider::new();
        let result = provider
            .categorize(&cat_input("NETFLIX.COM"))
            .await
            .unwrap();
        assert_eq!(result.category_name, "Entertainment:Subscriptions");
        assert!(result.confidence > 0.9);
    }

    #[tokio::test]
    async fn categorize_unknown_merchant_returns_low_confidence() {
        let provider = MockLlmProvider::new();
        let result = provider
            .categorize(&cat_input("OBSCURE SHOP XYZ"))
            .await
            .unwrap();
        assert_eq!(result.category_name, "Uncategorized");
        assert!(result.confidence < 0.5);
    }

    #[tokio::test]
    async fn categorize_is_case_insensitive() {
        let provider = MockLlmProvider::new();
        let result = provider
            .categorize(&cat_input("trader joes"))
            .await
            .unwrap();
        assert_eq!(result.category_name, "Food:Groceries");
    }

    #[tokio::test]
    async fn correlate_matching_transfer() {
        let provider = MockLlmProvider::new();
        let txn_a = TransactionSummary {
            merchant_name: "CHASE CREDIT CRD AUTOPAY".to_owned(),
            amount: dec!(-1500.00),
            remittance_information: vec!["Credit card payment".to_owned()],
            posted_date: NaiveDate::from_ymd_opt(2025, 1, 20).unwrap(),
            category: None,
        };
        let txn_b = TransactionSummary {
            merchant_name: "PAYMENT RECEIVED".to_owned(),
            amount: dec!(1500.00),
            remittance_information: vec!["Thank you for your payment".to_owned()],
            posted_date: NaiveDate::from_ymd_opt(2025, 1, 20).unwrap(),
            category: None,
        };

        let result = provider.propose_correlation(&txn_a, &txn_b).await.unwrap();
        assert_eq!(result.correlation_type, Some(CorrelationType::Transfer));
        assert!(result.confidence > 0.9);
    }

    #[tokio::test]
    async fn correlate_non_matching_amounts_returns_none() {
        let provider = MockLlmProvider::new();
        let txn_a = TransactionSummary {
            merchant_name: "AMAZON.COM".to_owned(),
            amount: dec!(-45.99),
            remittance_information: vec![],
            posted_date: NaiveDate::from_ymd_opt(2025, 1, 22).unwrap(),
            category: None,
        };
        let txn_b = TransactionSummary {
            merchant_name: "TARGET".to_owned(),
            amount: dec!(-65.00),
            remittance_information: vec![],
            posted_date: NaiveDate::from_ymd_opt(2025, 1, 21).unwrap(),
            category: None,
        };

        let result = provider.propose_correlation(&txn_a, &txn_b).await.unwrap();
        assert!(result.correlation_type.is_none());
    }

    #[tokio::test]
    async fn correlate_matching_amounts_far_apart_has_lower_confidence() {
        let provider = MockLlmProvider::new();
        let txn_a = TransactionSummary {
            merchant_name: "TRANSFER".to_owned(),
            amount: dec!(-500.00),
            remittance_information: vec![],
            posted_date: NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
            category: None,
        };
        let txn_b = TransactionSummary {
            merchant_name: "DEPOSIT".to_owned(),
            amount: dec!(500.00),
            remittance_information: vec![],
            posted_date: NaiveDate::from_ymd_opt(2025, 1, 15).unwrap(),
            category: None,
        };

        let result = provider.propose_correlation(&txn_a, &txn_b).await.unwrap();
        assert_eq!(result.correlation_type, Some(CorrelationType::Transfer));
        assert!(result.confidence < 0.9);
        assert!(result.confidence > 0.5);
    }

    #[tokio::test]
    async fn propose_rules_returns_three_patterns() {
        let provider = MockLlmProvider::new();
        let context = RuleContext {
            merchant_name: "WHOLE FOODS MARKET".to_owned(),
            remittance_information: vec!["Groceries".to_owned()],
            amount: dec!(72.34),
            posted_date: NaiveDate::from_ymd_opt(2025, 1, 15).unwrap(),
            category_name: "Food:Groceries".to_owned(),
            sibling_merchants: vec![],
            existing_rule_patterns: vec![],
            counterparty_name: None,
            counterparty_iban: None,
            counterparty_bic: None,
            bank_transaction_code: None,
            enrichment_item_titles: vec![],
        };
        let rules = provider.propose_rules(&context).await.unwrap();
        assert_eq!(rules.len(), 3);
        assert_eq!(rules[0].match_field, MatchField::Merchant);
        assert_eq!(rules[0].match_pattern, "^WHOLE FOODS MARKET$");
        assert_eq!(rules[1].match_pattern, "WHOLE");
        assert_eq!(rules[2].match_pattern, "whole");
    }
}
