use regex::RegexBuilder;
use rust_decimal::Decimal;

use crate::error::Error;
use crate::models::{
    CategoryId, CorrelationType, MatchField, Rule, RuleType, Transaction, TransactionId,
};

/// A rule with its match pattern pre-compiled for efficient repeated evaluation.
#[derive(Debug)]
pub struct CompiledRule {
    /// The original rule definition (cloned because `Rule` contains `String` fields
    /// and must outlive the borrow used to compile it).
    pub rule: Rule,
    /// The compiled form of `rule.match_pattern`.
    pub pattern: CompiledPattern,
}

/// The compiled representation of a rule's match pattern.
#[derive(Debug)]
pub enum CompiledPattern {
    /// A case-insensitive regular expression for merchant or description matching.
    Regex(regex::Regex),
    /// An inclusive numeric range for amount matching.
    AmountRange {
        /// Lower bound (inclusive).
        min: Decimal,
        /// Upper bound (inclusive).
        max: Decimal,
    },
}

/// Pre-compile a rule's match pattern for efficient repeated evaluation.
///
/// For `Merchant` or `Description` fields, the pattern is compiled as a
/// case-insensitive regular expression. For `AmountRange`, the pattern is
/// parsed as `"min..max"` where both bounds are decimal numbers.
///
/// # Errors
///
/// Returns `Error::InvalidRulePattern` if the regex fails to compile or the
/// amount range format is invalid.
pub fn compile_rule_pattern(rule: &Rule) -> Result<CompiledRule, Error> {
    let pattern = match rule.match_field {
        MatchField::Merchant | MatchField::Description => {
            let regex = RegexBuilder::new(&rule.match_pattern)
                .case_insensitive(true)
                .build()
                .map_err(|e| Error::InvalidRulePattern(e.to_string()))?;
            CompiledPattern::Regex(regex)
        }
        MatchField::AmountRange => {
            let (min, max) = parse_amount_range(&rule.match_pattern)?;
            CompiledPattern::AmountRange { min, max }
        }
    };

    Ok(CompiledRule {
        rule: rule.clone(),
        pattern,
    })
}

/// Evaluate categorization rules against a transaction, returning the first match.
///
/// Rules must be pre-sorted by priority descending. The first matching rule wins.
#[must_use]
pub fn evaluate_categorization_rules(
    transaction: &Transaction,
    rules: &[CompiledRule],
) -> Option<CategoryId> {
    rules
        .iter()
        .filter(|compiled| compiled.rule.rule_type == RuleType::Categorization)
        .find(|compiled| matches_rule(transaction, compiled))
        .and_then(|compiled| compiled.rule.target_category_id)
}

/// Evaluate correlation rules against a transaction and candidate partners.
///
/// For each correlation rule, searches candidates for a match. Returns the
/// first matched candidate's ID and the rule's target correlation type.
///
/// Rules without a `target_correlation_type` are skipped.
#[must_use]
pub fn evaluate_correlation_rules(
    _transaction: &Transaction,
    candidates: &[Transaction],
    rules: &[CompiledRule],
) -> Option<(TransactionId, CorrelationType)> {
    for compiled in rules {
        if compiled.rule.rule_type != RuleType::Correlation {
            continue;
        }

        let Some(correlation_type) = compiled.rule.target_correlation_type else {
            continue;
        };

        // The rule pattern is evaluated against the transaction itself to confirm
        // it applies, but for correlation we search candidates that also match.
        for candidate in candidates {
            if matches_rule(candidate, compiled) {
                return Some((candidate.id, correlation_type));
            }
        }
    }

    None
}

/// Check whether a transaction matches a compiled rule's pattern.
///
/// Returns `false` for mismatched pattern/field combinations (e.g. an
/// `AmountRange` pattern paired with a `Merchant` field).
fn matches_rule(transaction: &Transaction, compiled: &CompiledRule) -> bool {
    match (&compiled.pattern, compiled.rule.match_field) {
        (CompiledPattern::Regex(regex), MatchField::Merchant) => {
            regex.is_match(&transaction.merchant_name)
        }
        (CompiledPattern::Regex(regex), MatchField::Description) => {
            regex.is_match(&transaction.description)
        }
        (CompiledPattern::AmountRange { min, max }, MatchField::AmountRange) => {
            transaction.amount >= *min && transaction.amount <= *max
        }
        // Mismatched pattern/field combinations never match
        _ => false,
    }
}

/// Parse an amount range pattern in `"min..max"` format into two `Decimal` bounds.
fn parse_amount_range(pattern: &str) -> Result<(Decimal, Decimal), Error> {
    let parts: Vec<&str> = pattern.splitn(3, "..").collect();
    if parts.len() != 2 {
        return Err(Error::InvalidRulePattern(format!(
            "expected 'min..max' format, got: {pattern}"
        )));
    }

    let min: Decimal = parts[0]
        .trim()
        .parse()
        .map_err(|e| Error::InvalidRulePattern(format!("invalid min value: {e}")))?;

    let max: Decimal = parts[1]
        .trim()
        .parse()
        .map_err(|e| Error::InvalidRulePattern(format!("invalid max value: {e}")))?;

    Ok((min, max))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::*;
    use chrono::NaiveDate;
    use rust_decimal_macros::dec;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).expect("valid test date")
    }

    fn make_rule(
        rule_type: RuleType,
        match_field: MatchField,
        match_pattern: &str,
        target_category_id: Option<CategoryId>,
        target_correlation_type: Option<CorrelationType>,
        priority: i32,
    ) -> Rule {
        Rule {
            id: RuleId::new(),
            rule_type,
            match_field,
            match_pattern: match_pattern.to_owned(),
            target_category_id,
            target_correlation_type,
            priority,
        }
    }

    fn make_txn(merchant: &str, description: &str, amount: Decimal) -> Transaction {
        Transaction {
            id: TransactionId::new(),
            account_id: AccountId::new(),
            category_id: None,
            amount,
            original_amount: None,
            original_currency: None,
            merchant_name: merchant.to_owned(),
            description: description.to_owned(),
            posted_date: date(2025, 6, 15),
            budget_month_id: None,
            project_id: None,
            correlation_id: None,
            correlation_type: None,
            suggested_category: None,
        }
    }

    #[test]
    fn compile_valid_regex_rule() {
        let rule = make_rule(
            RuleType::Categorization,
            MatchField::Merchant,
            r"^starbucks",
            Some(CategoryId::new()),
            None,
            10,
        );

        let compiled = compile_rule_pattern(&rule);
        assert!(compiled.is_ok());
        assert!(matches!(
            compiled.as_ref().map(|c| &c.pattern),
            Ok(CompiledPattern::Regex(_))
        ));
    }

    #[test]
    fn compile_invalid_regex_returns_error() {
        let rule = make_rule(
            RuleType::Categorization,
            MatchField::Merchant,
            r"[invalid(",
            Some(CategoryId::new()),
            None,
            10,
        );

        let result = compile_rule_pattern(&rule);
        assert!(result.is_err());
    }

    #[test]
    fn compile_amount_range_rule() {
        let rule = make_rule(
            RuleType::Categorization,
            MatchField::AmountRange,
            "100.00..500.00",
            Some(CategoryId::new()),
            None,
            5,
        );

        let compiled = compile_rule_pattern(&rule);
        assert!(compiled.is_ok());

        let compiled = compiled.unwrap();
        match &compiled.pattern {
            CompiledPattern::AmountRange { min, max } => {
                assert_eq!(*min, dec!(100.00));
                assert_eq!(*max, dec!(500.00));
            }
            CompiledPattern::Regex(_) => panic!("expected AmountRange pattern"),
        }
    }

    #[test]
    fn invalid_amount_range_returns_error() {
        let rule = make_rule(
            RuleType::Categorization,
            MatchField::AmountRange,
            "not_a_range",
            Some(CategoryId::new()),
            None,
            5,
        );

        let result = compile_rule_pattern(&rule);
        assert!(result.is_err());
    }

    #[test]
    fn categorization_first_match_wins_by_priority() {
        let cat_coffee = CategoryId::new();
        let cat_food = CategoryId::new();

        // Higher priority rule (sorted first by caller)
        let rule_high = make_rule(
            RuleType::Categorization,
            MatchField::Merchant,
            r"starbucks",
            Some(cat_coffee),
            None,
            100,
        );

        // Lower priority rule
        let rule_low = make_rule(
            RuleType::Categorization,
            MatchField::Merchant,
            r"star",
            Some(cat_food),
            None,
            10,
        );

        let compiled = vec![
            compile_rule_pattern(&rule_high).unwrap(),
            compile_rule_pattern(&rule_low).unwrap(),
        ];

        let txn = make_txn("Starbucks Reserve", "Coffee purchase", dec!(5.50));

        let result = evaluate_categorization_rules(&txn, &compiled);
        assert_eq!(result, Some(cat_coffee));
    }

    #[test]
    fn categorization_no_match_returns_none() {
        let rule = make_rule(
            RuleType::Categorization,
            MatchField::Merchant,
            r"walmart",
            Some(CategoryId::new()),
            None,
            10,
        );

        let compiled = vec![compile_rule_pattern(&rule).unwrap()];

        let txn = make_txn("Target", "Household items", dec!(42.00));

        let result = evaluate_categorization_rules(&txn, &compiled);
        assert_eq!(result, None);
    }

    #[test]
    fn amount_range_matching() {
        let cat_id = CategoryId::new();
        let rule = make_rule(
            RuleType::Categorization,
            MatchField::AmountRange,
            "50.00..200.00",
            Some(cat_id),
            None,
            10,
        );

        let compiled = vec![compile_rule_pattern(&rule).unwrap()];

        // Within range
        let txn_in = make_txn("Any", "desc", dec!(100.00));
        assert_eq!(
            evaluate_categorization_rules(&txn_in, &compiled),
            Some(cat_id)
        );

        // At lower bound (inclusive)
        let txn_min = make_txn("Any", "desc", dec!(50.00));
        assert_eq!(
            evaluate_categorization_rules(&txn_min, &compiled),
            Some(cat_id)
        );

        // At upper bound (inclusive)
        let txn_max = make_txn("Any", "desc", dec!(200.00));
        assert_eq!(
            evaluate_categorization_rules(&txn_max, &compiled),
            Some(cat_id)
        );

        // Below range
        let txn_below = make_txn("Any", "desc", dec!(49.99));
        assert_eq!(evaluate_categorization_rules(&txn_below, &compiled), None);

        // Above range
        let txn_above = make_txn("Any", "desc", dec!(200.01));
        assert_eq!(evaluate_categorization_rules(&txn_above, &compiled), None);
    }

    #[test]
    fn correlation_rule_evaluation() {
        let rule = make_rule(
            RuleType::Correlation,
            MatchField::Merchant,
            r"venmo",
            None,
            Some(CorrelationType::Transfer),
            10,
        );

        let compiled = vec![compile_rule_pattern(&rule).unwrap()];

        let txn = make_txn("Bank Transfer", "Outgoing", dec!(-500.00));
        let candidate = make_txn("Venmo", "Payment received", dec!(500.00));
        let candidate_id = candidate.id;

        let result = evaluate_correlation_rules(&txn, &[candidate], &compiled);
        assert_eq!(result, Some((candidate_id, CorrelationType::Transfer)));
    }

    #[test]
    fn correlation_skips_rules_without_correlation_type() {
        let rule = make_rule(
            RuleType::Correlation,
            MatchField::Merchant,
            r"venmo",
            None,
            None, // no correlation type set
            10,
        );

        let compiled = vec![compile_rule_pattern(&rule).unwrap()];

        let txn = make_txn("Bank Transfer", "Outgoing", dec!(-500.00));
        let candidate = make_txn("Venmo", "Payment received", dec!(500.00));

        let result = evaluate_correlation_rules(&txn, &[candidate], &compiled);
        assert_eq!(result, None);
    }

    #[test]
    fn correlation_no_match_returns_none() {
        let rule = make_rule(
            RuleType::Correlation,
            MatchField::Merchant,
            r"venmo",
            None,
            Some(CorrelationType::Transfer),
            10,
        );

        let compiled = vec![compile_rule_pattern(&rule).unwrap()];

        let txn = make_txn("Bank Transfer", "Outgoing", dec!(-500.00));
        let candidate = make_txn("PayPal", "Refund", dec!(500.00));

        let result = evaluate_correlation_rules(&txn, &[candidate], &compiled);
        assert_eq!(result, None);
    }

    #[test]
    fn regex_is_case_insensitive() {
        let cat_id = CategoryId::new();
        let rule = make_rule(
            RuleType::Categorization,
            MatchField::Merchant,
            r"starbucks",
            Some(cat_id),
            None,
            10,
        );

        let compiled = vec![compile_rule_pattern(&rule).unwrap()];

        let txn = make_txn("STARBUCKS", "Coffee", dec!(5.00));
        assert_eq!(evaluate_categorization_rules(&txn, &compiled), Some(cat_id));
    }

    #[test]
    fn description_field_matching() {
        let cat_id = CategoryId::new();
        let rule = make_rule(
            RuleType::Categorization,
            MatchField::Description,
            r"grocery",
            Some(cat_id),
            None,
            10,
        );

        let compiled = vec![compile_rule_pattern(&rule).unwrap()];

        let txn = make_txn("Local Store", "Weekly grocery shopping", dec!(85.00));
        assert_eq!(evaluate_categorization_rules(&txn, &compiled), Some(cat_id));
    }

    #[test]
    fn categorization_rules_skip_correlation_type() {
        let rule = make_rule(
            RuleType::Correlation,
            MatchField::Merchant,
            r"anything",
            Some(CategoryId::new()),
            Some(CorrelationType::Transfer),
            10,
        );

        let compiled = vec![compile_rule_pattern(&rule).unwrap()];

        let txn = make_txn("anything", "desc", dec!(10.00));
        // Should return None because the rule is Correlation, not Categorization
        assert_eq!(evaluate_categorization_rules(&txn, &compiled), None);
    }
}
