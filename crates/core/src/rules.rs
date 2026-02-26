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
    /// A case-insensitive regular expression for text field matching.
    Regex(regex::Regex),
    /// A numeric range for amount matching, with optional open-ended bounds.
    AmountRange {
        min: Option<Decimal>,
        max: Option<Decimal>,
        min_inclusive: bool,
        max_inclusive: bool,
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
        MatchField::Merchant
        | MatchField::Description
        | MatchField::CounterpartyName
        | MatchField::CounterpartyIban
        | MatchField::CounterpartyBic
        | MatchField::BankTransactionCode => {
            let regex = RegexBuilder::new(&rule.match_pattern)
                .case_insensitive(true)
                .build()
                .map_err(|e| Error::InvalidRulePattern(e.to_string()))?;
            CompiledPattern::Regex(regex)
        }
        MatchField::AmountRange => parse_amount_range(&rule.match_pattern)?,
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
        (CompiledPattern::Regex(regex), MatchField::CounterpartyName) => transaction
            .counterparty_name
            .as_ref()
            .is_some_and(|v| regex.is_match(v)),
        (CompiledPattern::Regex(regex), MatchField::CounterpartyIban) => transaction
            .counterparty_iban
            .as_ref()
            .is_some_and(|v| regex.is_match(v)),
        (CompiledPattern::Regex(regex), MatchField::CounterpartyBic) => transaction
            .counterparty_bic
            .as_ref()
            .is_some_and(|v| regex.is_match(v)),
        (CompiledPattern::Regex(regex), MatchField::BankTransactionCode) => transaction
            .bank_transaction_code
            .as_ref()
            .is_some_and(|v| regex.is_match(v)),
        (
            CompiledPattern::AmountRange {
                min,
                max,
                min_inclusive,
                max_inclusive,
            },
            MatchField::AmountRange,
        ) => {
            let amount = transaction.amount;
            let above_min = match min {
                Some(m) => {
                    if *min_inclusive {
                        amount >= *m
                    } else {
                        amount > *m
                    }
                }
                None => true,
            };
            let below_max = match max {
                Some(m) => {
                    if *max_inclusive {
                        amount <= *m
                    } else {
                        amount < *m
                    }
                }
                None => true,
            };
            above_min && below_max
        }
        // Mismatched pattern/field combinations never match
        _ => false,
    }
}

/// Parse an amount pattern into a `CompiledPattern::AmountRange`.
///
/// Supported formats:
/// - `"min..max"` — inclusive range (backward compatible)
/// - `">100"` — strictly greater than
/// - `">=100"` — greater than or equal
/// - `"<100"` — strictly less than
/// - `"<=100"` — less than or equal
fn parse_amount_range(pattern: &str) -> Result<CompiledPattern, Error> {
    let trimmed = pattern.trim();

    // Try comparison operators first (>=, <=, >, <)
    if let Some(val) = trimmed.strip_prefix(">=") {
        let v: Decimal = val
            .trim()
            .parse()
            .map_err(|e| Error::InvalidRulePattern(format!("invalid amount value: {e}")))?;
        return Ok(CompiledPattern::AmountRange {
            min: Some(v),
            max: None,
            min_inclusive: true,
            max_inclusive: true,
        });
    }
    if let Some(val) = trimmed.strip_prefix("<=") {
        let v: Decimal = val
            .trim()
            .parse()
            .map_err(|e| Error::InvalidRulePattern(format!("invalid amount value: {e}")))?;
        return Ok(CompiledPattern::AmountRange {
            min: None,
            max: Some(v),
            min_inclusive: true,
            max_inclusive: true,
        });
    }
    if let Some(val) = trimmed.strip_prefix('>') {
        let v: Decimal = val
            .trim()
            .parse()
            .map_err(|e| Error::InvalidRulePattern(format!("invalid amount value: {e}")))?;
        return Ok(CompiledPattern::AmountRange {
            min: Some(v),
            max: None,
            min_inclusive: false,
            max_inclusive: true,
        });
    }
    if let Some(val) = trimmed.strip_prefix('<') {
        let v: Decimal = val
            .trim()
            .parse()
            .map_err(|e| Error::InvalidRulePattern(format!("invalid amount value: {e}")))?;
        return Ok(CompiledPattern::AmountRange {
            min: None,
            max: Some(v),
            min_inclusive: true,
            max_inclusive: false,
        });
    }

    // Inclusive range: "min..max"
    let parts: Vec<&str> = trimmed.splitn(3, "..").collect();
    if parts.len() != 2 {
        return Err(Error::InvalidRulePattern(format!(
            "expected 'min..max', '>N', '>=N', '<N', or '<=N', got: {pattern}"
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

    Ok(CompiledPattern::AmountRange {
        min: Some(min),
        max: Some(max),
        min_inclusive: true,
        max_inclusive: true,
    })
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
            correlation_id: None,
            correlation_type: None,
            category_method: None,
            suggested_category: None,
            counterparty_name: None,
            counterparty_iban: None,
            counterparty_bic: None,
            bank_transaction_code: None,
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
            CompiledPattern::AmountRange {
                min,
                max,
                min_inclusive,
                max_inclusive,
            } => {
                assert_eq!(*min, Some(dec!(100.00)));
                assert_eq!(*max, Some(dec!(500.00)));
                assert!(*min_inclusive);
                assert!(*max_inclusive);
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

    #[test]
    fn counterparty_name_matching() {
        let cat_id = CategoryId::new();
        let rule = make_rule(
            RuleType::Categorization,
            MatchField::CounterpartyName,
            r"landlord",
            Some(cat_id),
            None,
            10,
        );
        let compiled = vec![compile_rule_pattern(&rule).unwrap()];

        let mut txn = make_txn("Any", "desc", dec!(1000.00));
        txn.counterparty_name = Some("My Landlord Inc".to_owned());
        assert_eq!(evaluate_categorization_rules(&txn, &compiled), Some(cat_id));
    }

    #[test]
    fn counterparty_name_none_never_matches() {
        let rule = make_rule(
            RuleType::Categorization,
            MatchField::CounterpartyName,
            r"landlord",
            Some(CategoryId::new()),
            None,
            10,
        );
        let compiled = vec![compile_rule_pattern(&rule).unwrap()];

        let txn = make_txn("Any", "desc", dec!(1000.00));
        assert_eq!(evaluate_categorization_rules(&txn, &compiled), None);
    }

    #[test]
    fn counterparty_iban_matching() {
        let cat_id = CategoryId::new();
        let rule = make_rule(
            RuleType::Categorization,
            MatchField::CounterpartyIban,
            r"^FI\d+",
            Some(cat_id),
            None,
            10,
        );
        let compiled = vec![compile_rule_pattern(&rule).unwrap()];

        let mut txn = make_txn("Any", "desc", dec!(50.00));
        txn.counterparty_iban = Some("FI1234567890".to_owned());
        assert_eq!(evaluate_categorization_rules(&txn, &compiled), Some(cat_id));

        // None never matches
        let txn_none = make_txn("Any", "desc", dec!(50.00));
        assert_eq!(evaluate_categorization_rules(&txn_none, &compiled), None);
    }

    #[test]
    fn counterparty_bic_matching() {
        let cat_id = CategoryId::new();
        let rule = make_rule(
            RuleType::Categorization,
            MatchField::CounterpartyBic,
            r"NDEAFIHH",
            Some(cat_id),
            None,
            10,
        );
        let compiled = vec![compile_rule_pattern(&rule).unwrap()];

        let mut txn = make_txn("Any", "desc", dec!(50.00));
        txn.counterparty_bic = Some("NDEAFIHH".to_owned());
        assert_eq!(evaluate_categorization_rules(&txn, &compiled), Some(cat_id));
    }

    #[test]
    fn bank_transaction_code_matching() {
        let cat_id = CategoryId::new();
        let rule = make_rule(
            RuleType::Categorization,
            MatchField::BankTransactionCode,
            r"PMNT-ICDT-STDO",
            Some(cat_id),
            None,
            10,
        );
        let compiled = vec![compile_rule_pattern(&rule).unwrap()];

        let mut txn = make_txn("Any", "desc", dec!(50.00));
        txn.bank_transaction_code = Some("PMNT-ICDT-STDO".to_owned());
        assert_eq!(evaluate_categorization_rules(&txn, &compiled), Some(cat_id));

        // None never matches
        let txn_none = make_txn("Any", "desc", dec!(50.00));
        assert_eq!(evaluate_categorization_rules(&txn_none, &compiled), None);
    }

    #[test]
    fn amount_greater_than() {
        let cat_id = CategoryId::new();
        let rule = make_rule(
            RuleType::Categorization,
            MatchField::AmountRange,
            ">100",
            Some(cat_id),
            None,
            10,
        );
        let compiled = vec![compile_rule_pattern(&rule).unwrap()];

        // Exactly 100 should NOT match (strict >)
        let txn_eq = make_txn("Any", "desc", dec!(100));
        assert_eq!(evaluate_categorization_rules(&txn_eq, &compiled), None);

        // Above 100 should match
        let txn_above = make_txn("Any", "desc", dec!(100.01));
        assert_eq!(
            evaluate_categorization_rules(&txn_above, &compiled),
            Some(cat_id)
        );
    }

    #[test]
    fn amount_greater_than_or_equal() {
        let cat_id = CategoryId::new();
        let rule = make_rule(
            RuleType::Categorization,
            MatchField::AmountRange,
            ">=100",
            Some(cat_id),
            None,
            10,
        );
        let compiled = vec![compile_rule_pattern(&rule).unwrap()];

        let txn_eq = make_txn("Any", "desc", dec!(100));
        assert_eq!(
            evaluate_categorization_rules(&txn_eq, &compiled),
            Some(cat_id)
        );

        let txn_below = make_txn("Any", "desc", dec!(99.99));
        assert_eq!(evaluate_categorization_rules(&txn_below, &compiled), None);
    }

    #[test]
    fn amount_less_than() {
        let cat_id = CategoryId::new();
        let rule = make_rule(
            RuleType::Categorization,
            MatchField::AmountRange,
            "<50",
            Some(cat_id),
            None,
            10,
        );
        let compiled = vec![compile_rule_pattern(&rule).unwrap()];

        // Exactly 50 should NOT match (strict <)
        let txn_eq = make_txn("Any", "desc", dec!(50));
        assert_eq!(evaluate_categorization_rules(&txn_eq, &compiled), None);

        // Below 50 should match
        let txn_below = make_txn("Any", "desc", dec!(49.99));
        assert_eq!(
            evaluate_categorization_rules(&txn_below, &compiled),
            Some(cat_id)
        );
    }

    #[test]
    fn amount_less_than_or_equal() {
        let cat_id = CategoryId::new();
        let rule = make_rule(
            RuleType::Categorization,
            MatchField::AmountRange,
            "<=50",
            Some(cat_id),
            None,
            10,
        );
        let compiled = vec![compile_rule_pattern(&rule).unwrap()];

        let txn_eq = make_txn("Any", "desc", dec!(50));
        assert_eq!(
            evaluate_categorization_rules(&txn_eq, &compiled),
            Some(cat_id)
        );

        let txn_above = make_txn("Any", "desc", dec!(50.01));
        assert_eq!(evaluate_categorization_rules(&txn_above, &compiled), None);
    }
}
