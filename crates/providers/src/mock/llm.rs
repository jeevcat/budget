use rust_decimal::Decimal;

use crate::error::ProviderError;
use crate::llm::{
    CategorizeResult, CorrelationResult, CorrelationType, LlmProvider, ProposedRule,
    TransactionSummary,
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
        merchant_name: &str,
        _amount: Decimal,
        _description: Option<&str>,
        _existing_categories: &[String],
    ) -> Result<CategorizeResult, ProviderError> {
        let merchant_upper = merchant_name.to_uppercase();

        let (category, confidence) =
            match_category(&merchant_upper).unwrap_or(("Uncategorized", 0.15));

        Ok(CategorizeResult {
            category_name: category.to_owned(),
            confidence,
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

    async fn propose_rule(
        &self,
        merchant_name: &str,
        user_category: &str,
    ) -> Result<ProposedRule, ProviderError> {
        Ok(ProposedRule {
            match_field: crate::llm::MatchField::Merchant,
            match_pattern: merchant_name.to_owned(),
            explanation: format!(
                "Transactions from \"{merchant_name}\" are typically \"{user_category}\""
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;
    use rust_decimal_macros::dec;

    use super::*;

    #[tokio::test]
    async fn categorize_grocery_merchant() {
        let provider = MockLlmProvider::new();
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
        assert!(result.confidence > 0.8);
    }

    #[tokio::test]
    async fn categorize_restaurant() {
        let provider = MockLlmProvider::new();
        let result = provider
            .categorize("CHIPOTLE MEXICAN GRILL", dec!(42.50), None, &[])
            .await
            .unwrap();
        assert_eq!(result.category_name, "Food:Restaurants");
        assert!(result.confidence > 0.8);
    }

    #[tokio::test]
    async fn categorize_subscription() {
        let provider = MockLlmProvider::new();
        let result = provider
            .categorize(
                "NETFLIX.COM",
                dec!(15.99),
                Some("Monthly subscription"),
                &[],
            )
            .await
            .unwrap();
        assert_eq!(result.category_name, "Entertainment:Subscriptions");
        assert!(result.confidence > 0.9);
    }

    #[tokio::test]
    async fn categorize_unknown_merchant_returns_low_confidence() {
        let provider = MockLlmProvider::new();
        let result = provider
            .categorize("OBSCURE SHOP XYZ", dec!(25.00), None, &[])
            .await
            .unwrap();
        assert_eq!(result.category_name, "Uncategorized");
        assert!(result.confidence < 0.5);
    }

    #[tokio::test]
    async fn categorize_is_case_insensitive() {
        let provider = MockLlmProvider::new();
        let result = provider
            .categorize("trader joes", dec!(58.12), None, &[])
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
            description: Some("Credit card payment".to_owned()),
            posted_date: NaiveDate::from_ymd_opt(2025, 1, 20).unwrap(),
        };
        let txn_b = TransactionSummary {
            merchant_name: "PAYMENT RECEIVED".to_owned(),
            amount: dec!(1500.00),
            description: Some("Thank you for your payment".to_owned()),
            posted_date: NaiveDate::from_ymd_opt(2025, 1, 20).unwrap(),
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
            description: None,
            posted_date: NaiveDate::from_ymd_opt(2025, 1, 22).unwrap(),
        };
        let txn_b = TransactionSummary {
            merchant_name: "TARGET".to_owned(),
            amount: dec!(-65.00),
            description: None,
            posted_date: NaiveDate::from_ymd_opt(2025, 1, 21).unwrap(),
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
            description: None,
            posted_date: NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
        };
        let txn_b = TransactionSummary {
            merchant_name: "DEPOSIT".to_owned(),
            amount: dec!(500.00),
            description: None,
            posted_date: NaiveDate::from_ymd_opt(2025, 1, 15).unwrap(),
        };

        let result = provider.propose_correlation(&txn_a, &txn_b).await.unwrap();
        assert_eq!(result.correlation_type, Some(CorrelationType::Transfer));
        assert!(result.confidence < 0.9);
        assert!(result.confidence > 0.5);
    }

    #[tokio::test]
    async fn propose_rule_returns_merchant_match() {
        let provider = MockLlmProvider::new();
        let rule = provider
            .propose_rule("WHOLE FOODS MARKET", "Food:Groceries")
            .await
            .unwrap();
        assert_eq!(rule.match_field, crate::llm::MatchField::Merchant);
        assert_eq!(rule.match_pattern, "WHOLE FOODS MARKET");
        assert!(rule.explanation.contains("WHOLE FOODS MARKET"));
        assert!(rule.explanation.contains("Food:Groceries"));
    }
}
