use std::collections::HashSet;

use crate::types::{AmazonTransaction, BankCandidate, MatchConfidence, MatchResult};

/// Maximum date offset (in days) for a transaction match.
const DATE_WINDOW_DAYS: i64 = 3;

/// Match Amazon transactions against bank candidates.
///
/// A match requires:
/// - Exact amount match (absolute values equal)
/// - Date within ±3 days
/// - Bank merchant name contains "AMZN" or "Amazon" (case-insensitive)
///
/// For duplicate amounts on the same day, the closest date is preferred.
/// Each bank transaction and Amazon transaction can only be matched once.
#[must_use]
pub fn find_matches(
    amazon_txns: &[AmazonTransaction],
    bank_candidates: &[BankCandidate],
) -> Vec<MatchResult> {
    let mut results = Vec::new();
    let mut used_bank_ids = HashSet::new();
    let mut used_dedup_keys = HashSet::new();

    // Build a list of (amazon_idx, bank_idx, date_distance) sorted by distance
    let mut candidates: Vec<(usize, usize, i64)> = Vec::new();

    for (ai, atxn) in amazon_txns.iter().enumerate() {
        let amazon_abs = atxn.amount.abs();

        for (bi, bank) in bank_candidates.iter().enumerate() {
            let bank_abs = bank.amount.abs();

            if amazon_abs != bank_abs {
                continue;
            }

            if !is_amazon_merchant(&bank.merchant_name) {
                continue;
            }

            let date_diff = (atxn.date - bank.posted_date).num_days().abs();
            if date_diff > DATE_WINDOW_DAYS {
                continue;
            }

            candidates.push((ai, bi, date_diff));
        }
    }

    // Sort by date distance (closest first) for greedy matching
    candidates.sort_by_key(|&(_, _, dist)| dist);

    for (ai, bi, date_diff) in candidates {
        let atxn = &amazon_txns[ai];
        let bank = &bank_candidates[bi];

        if used_dedup_keys.contains(&atxn.dedup_key) || used_bank_ids.contains(&bank.id) {
            continue;
        }

        let confidence = if date_diff == 0 {
            MatchConfidence::Exact
        } else {
            MatchConfidence::Approximate
        };

        used_dedup_keys.insert(atxn.dedup_key.clone());
        used_bank_ids.insert(bank.id);

        results.push(MatchResult {
            amazon_dedup_key: atxn.dedup_key.clone(),
            bank_transaction_id: bank.id,
            confidence,
        });
    }

    results
}

fn is_amazon_merchant(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.contains("amzn") || lower.contains("amazon")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    fn amazon_txn(date: NaiveDate, amount: rust_decimal::Decimal) -> AmazonTransaction {
        AmazonTransaction {
            date,
            amount,
            currency: "EUR".into(),
            statement_descriptor: "AMZN Mktp DE".into(),
            status: crate::types::AmazonTransactionStatus::Charged,
            payment_method: "Visa ••••1000".into(),
            order_ids: vec!["304-0000000-0000000".into()],
            dedup_key: crate::parser::dedup_key(date, amount, "AMZN Mktp DE"),
        }
    }

    fn bank(date: NaiveDate, amount: rust_decimal::Decimal, merchant: &str) -> BankCandidate {
        BankCandidate {
            id: Uuid::new_v4(),
            amount,
            posted_date: date,
            merchant_name: merchant.into(),
        }
    }

    #[test]
    fn exact_amount_and_date() {
        let d = NaiveDate::from_ymd_opt(2023, 10, 7).unwrap();
        let matches = find_matches(
            &[amazon_txn(d, dec!(-42.91))],
            &[bank(d, dec!(-42.91), "AMZN Mktp DE")],
        );
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].confidence, MatchConfidence::Exact);
    }

    #[test]
    fn date_offset_within_window() {
        let amazon_date = NaiveDate::from_ymd_opt(2023, 10, 7).unwrap();
        let bank_date = NaiveDate::from_ymd_opt(2023, 10, 9).unwrap();
        let matches = find_matches(
            &[amazon_txn(amazon_date, dec!(-42.91))],
            &[bank(bank_date, dec!(-42.91), "AMZN Mktp DE")],
        );
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].confidence, MatchConfidence::Approximate);
    }

    #[test]
    fn date_offset_outside_window() {
        let amazon_date = NaiveDate::from_ymd_opt(2023, 10, 7).unwrap();
        let bank_date = NaiveDate::from_ymd_opt(2023, 10, 11).unwrap();
        let matches = find_matches(
            &[amazon_txn(amazon_date, dec!(-42.91))],
            &[bank(bank_date, dec!(-42.91), "AMZN Mktp DE")],
        );
        assert!(matches.is_empty());
    }

    #[test]
    fn amount_mismatch() {
        let d = NaiveDate::from_ymd_opt(2023, 10, 7).unwrap();
        let matches = find_matches(
            &[amazon_txn(d, dec!(-42.91))],
            &[bank(d, dec!(-42.92), "AMZN Mktp DE")],
        );
        assert!(matches.is_empty());
    }

    #[test]
    fn non_amazon_merchant_no_match() {
        let d = NaiveDate::from_ymd_opt(2023, 10, 7).unwrap();
        let matches = find_matches(
            &[amazon_txn(d, dec!(-42.91))],
            &[bank(d, dec!(-42.91), "REWE Supermarkt")],
        );
        assert!(matches.is_empty());
    }

    #[test]
    fn duplicate_amounts_closest_date_wins() {
        let amazon_date = NaiveDate::from_ymd_opt(2023, 10, 7).unwrap();
        let close_date = NaiveDate::from_ymd_opt(2023, 10, 8).unwrap();
        let far_date = NaiveDate::from_ymd_opt(2023, 10, 10).unwrap();

        let b_close = bank(close_date, dec!(-16.49), "AMZN Mktp DE");
        let b_far = bank(far_date, dec!(-16.49), "AMZN Mktp DE");
        let close_id = b_close.id;

        let matches = find_matches(&[amazon_txn(amazon_date, dec!(-16.49))], &[b_close, b_far]);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].bank_transaction_id, close_id);
    }

    #[test]
    fn refund_matching() {
        let d = NaiveDate::from_ymd_opt(2023, 11, 15).unwrap();
        let mut atxn = amazon_txn(d, dec!(80.99));
        atxn.status = crate::types::AmazonTransactionStatus::Refunded;
        atxn.dedup_key = crate::parser::dedup_key(d, dec!(80.99), "AMZN Mktp DE");

        let matches = find_matches(&[atxn], &[bank(d, dec!(80.99), "AMZN Mktp DE")]);
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn multiple_amazon_txns_matched_independently() {
        let d1 = NaiveDate::from_ymd_opt(2023, 10, 7).unwrap();
        let d2 = NaiveDate::from_ymd_opt(2023, 10, 14).unwrap();

        let b1 = bank(d1, dec!(-42.91), "AMZN Mktp DE");
        let b2 = bank(d2, dec!(-16.49), "Amazon.de");
        let b1_id = b1.id;
        let b2_id = b2.id;

        let matches = find_matches(
            &[amazon_txn(d1, dec!(-42.91)), amazon_txn(d2, dec!(-16.49))],
            &[b1, b2],
        );
        assert_eq!(matches.len(), 2);
        let ids: HashSet<_> = matches.iter().map(|m| m.bank_transaction_id).collect();
        assert!(ids.contains(&b1_id));
        assert!(ids.contains(&b2_id));
    }

    #[test]
    fn empty_inputs() {
        assert!(find_matches(&[], &[]).is_empty());
        let d = NaiveDate::from_ymd_opt(2023, 10, 7).unwrap();
        assert!(find_matches(&[amazon_txn(d, dec!(-42.91))], &[]).is_empty());
        assert!(find_matches(&[], &[bank(d, dec!(-42.91), "AMZN")]).is_empty());
    }

    #[test]
    fn no_double_matching() {
        let d = NaiveDate::from_ymd_opt(2023, 10, 7).unwrap();
        // Two Amazon txns with same amount, one bank txn
        let matches = find_matches(
            &[amazon_txn(d, dec!(-42.91)), {
                let mut t = amazon_txn(d, dec!(-42.91));
                t.dedup_key = "different-key".into();
                t
            }],
            &[bank(d, dec!(-42.91), "AMZN Mktp DE")],
        );
        assert_eq!(matches.len(), 1, "bank txn should only match once");
    }
}
