use std::collections::HashSet;

use regex::Regex;

use crate::types::{BankCandidate, MatchResult, PayPalTransaction};

/// Maximum date offset (in days) for a transaction match.
const DATE_WINDOW_DAYS: i64 = 3;

/// Match `PayPal` transactions against bank candidates.
///
/// Two-tier matching:
/// 1. **Reference match**: extract a reference number from the bank's
///    `remittance_information` and match against the `PayPal` transaction ID.
/// 2. **Fallback**: exact amount match + date within ±3 days + merchant
///    name contains "paypal".
///
/// For duplicate amounts on the same day, the closest date is preferred.
/// Each bank transaction and `PayPal` transaction can only be matched once.
#[must_use]
pub fn find_matches(
    paypal_txns: &[PayPalTransaction],
    bank_candidates: &[BankCandidate],
) -> Vec<MatchResult> {
    let mut results = Vec::new();
    let mut used_bank_ids = HashSet::new();
    let mut used_paypal_ids: HashSet<&str> = HashSet::new();

    // Build a lookup from PayPal transaction ID → index
    let paypal_by_id: std::collections::HashMap<&str, usize> = paypal_txns
        .iter()
        .enumerate()
        .map(|(i, t)| (t.transaction_id.as_str(), i))
        .collect();

    // Pass 1: reference-based matching (highest confidence)
    for bank in bank_candidates {
        if used_bank_ids.contains(&bank.id) {
            continue;
        }
        if let Some(ref_id) = extract_paypal_reference(&bank.remittance_information)
            && let Some(&pi) = paypal_by_id.get(ref_id.as_str())
        {
            let ptxn = &paypal_txns[pi];
            if !used_paypal_ids.contains(ptxn.transaction_id.as_str()) {
                used_bank_ids.insert(bank.id);
                used_paypal_ids.insert(&ptxn.transaction_id);
                results.push(MatchResult {
                    paypal_transaction_id: ptxn.transaction_id.clone(),
                    bank_transaction_id: bank.id,
                });
            }
        }
    }

    // Pass 2: amount + date + merchant fallback
    let mut candidates: Vec<(usize, usize, i64)> = Vec::new();

    for (pi, ptxn) in paypal_txns.iter().enumerate() {
        if used_paypal_ids.contains(ptxn.transaction_id.as_str()) {
            continue;
        }

        for (bi, bank) in bank_candidates.iter().enumerate() {
            if used_bank_ids.contains(&bank.id) {
                continue;
            }

            // Exact amount match (same sign) — PayPal outgoing payments are
            // negative, matching bank debits. Refunds (positive in PayPal)
            // should only match bank credits (positive).
            if ptxn.amount != bank.amount {
                continue;
            }

            if !is_paypal_merchant(&bank.merchant_name) {
                continue;
            }

            let date_diff = (ptxn.transaction_date - bank.posted_date).num_days().abs();
            if date_diff > DATE_WINDOW_DAYS {
                continue;
            }

            candidates.push((pi, bi, date_diff));
        }
    }

    // Sort by date distance (closest first) for greedy matching
    candidates.sort_by_key(|&(_, _, dist)| dist);

    for (pi, bi, _) in candidates {
        let ptxn = &paypal_txns[pi];
        let bank = &bank_candidates[bi];

        if used_paypal_ids.contains(ptxn.transaction_id.as_str())
            || used_bank_ids.contains(&bank.id)
        {
            continue;
        }

        used_paypal_ids.insert(&ptxn.transaction_id);
        used_bank_ids.insert(bank.id);

        results.push(MatchResult {
            paypal_transaction_id: ptxn.transaction_id.clone(),
            bank_transaction_id: bank.id,
        });
    }

    results
}

fn is_paypal_merchant(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.contains("paypal")
}

/// Extract a `PayPal` reference number from bank remittance information.
///
/// Bank remittance lines contain patterns like:
/// `"remittanceinformation:1046498634842/PP.8173.PP/. , Ihr Einkauf bei"`
///
/// We extract the numeric ID before `/PP`.
fn extract_paypal_reference(remittance: &[String]) -> Option<String> {
    // Lazy-init regex matching digits followed by /PP
    let re = Regex::new(r"(\d{10,})/PP").expect("valid regex");

    for line in remittance {
        if let Some(caps) = re.captures(line) {
            return Some(caps[1].to_owned());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    fn paypal_txn(id: &str, date: NaiveDate, amount: rust_decimal::Decimal) -> PayPalTransaction {
        PayPalTransaction {
            transaction_id: id.to_owned(),
            transaction_date: date,
            amount,
            currency: "EUR".into(),
            merchant_name: Some("Some Shop".into()),
            event_code: Some("T0006".into()),
            status: "S".into(),
            items: vec![],
            payer_email: None,
            payer_name: None,
        }
    }

    fn bank(
        date: NaiveDate,
        amount: rust_decimal::Decimal,
        merchant: &str,
        remittance: &[&str],
    ) -> BankCandidate {
        BankCandidate {
            id: Uuid::new_v4(),
            amount,
            posted_date: date,
            merchant_name: merchant.into(),
            remittance_information: remittance.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    #[test]
    fn reference_based_match() {
        let d = NaiveDate::from_ymd_opt(2024, 6, 1).unwrap();
        let ptxn = paypal_txn("1046498634842", d, dec!(-32.00));
        let btxn = bank(
            d,
            dec!(-32.00),
            "PayPal Europe S.a.r.l.",
            &[
                "mandatereference:43NJ2252D9V2E,creditorid:LU96,remittanceinformation:1046498634842/PP.8173.PP/. , Ihr Einkauf bei",
            ],
        );

        let matches = find_matches(&[ptxn], &[btxn]);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].paypal_transaction_id, "1046498634842");
    }

    #[test]
    fn amount_and_date_fallback() {
        let d = NaiveDate::from_ymd_opt(2024, 6, 1).unwrap();
        let ptxn = paypal_txn("TXN001", d, dec!(-15.00));
        let btxn = bank(d, dec!(-15.00), "PayPal Europe S.a.r.l.", &[]);

        let matches = find_matches(&[ptxn], &[btxn]);
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn date_offset_within_window() {
        let paypal_date = NaiveDate::from_ymd_opt(2024, 6, 1).unwrap();
        let bank_date = NaiveDate::from_ymd_opt(2024, 6, 3).unwrap();
        let ptxn = paypal_txn("TXN001", paypal_date, dec!(-15.00));
        let btxn = bank(bank_date, dec!(-15.00), "PayPal Europe", &[]);

        let matches = find_matches(&[ptxn], &[btxn]);
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn date_offset_outside_window() {
        let paypal_date = NaiveDate::from_ymd_opt(2024, 6, 1).unwrap();
        let bank_date = NaiveDate::from_ymd_opt(2024, 6, 5).unwrap();
        let ptxn = paypal_txn("TXN001", paypal_date, dec!(-15.00));
        let btxn = bank(bank_date, dec!(-15.00), "PayPal Europe", &[]);

        let matches = find_matches(&[ptxn], &[btxn]);
        assert!(matches.is_empty());
    }

    #[test]
    fn amount_mismatch() {
        let d = NaiveDate::from_ymd_opt(2024, 6, 1).unwrap();
        let ptxn = paypal_txn("TXN001", d, dec!(-15.00));
        let btxn = bank(d, dec!(-15.01), "PayPal Europe", &[]);

        let matches = find_matches(&[ptxn], &[btxn]);
        assert!(matches.is_empty());
    }

    #[test]
    fn non_paypal_merchant_no_match() {
        let d = NaiveDate::from_ymd_opt(2024, 6, 1).unwrap();
        let ptxn = paypal_txn("TXN001", d, dec!(-15.00));
        let btxn = bank(d, dec!(-15.00), "REWE", &[]);

        let matches = find_matches(&[ptxn], &[btxn]);
        assert!(matches.is_empty());
    }

    #[test]
    fn no_double_matching() {
        let d = NaiveDate::from_ymd_opt(2024, 6, 1).unwrap();
        let p1 = paypal_txn("TXN001", d, dec!(-15.00));
        let p2 = paypal_txn("TXN002", d, dec!(-15.00));
        let btxn = bank(d, dec!(-15.00), "PayPal Europe", &[]);

        let matches = find_matches(&[p1, p2], &[btxn]);
        assert_eq!(matches.len(), 1, "bank txn should only match once");
    }

    #[test]
    fn duplicate_amounts_closest_date_wins() {
        let paypal_date = NaiveDate::from_ymd_opt(2024, 6, 1).unwrap();
        let close = NaiveDate::from_ymd_opt(2024, 6, 2).unwrap();
        let far = NaiveDate::from_ymd_opt(2024, 6, 4).unwrap();

        let ptxn = paypal_txn("TXN001", paypal_date, dec!(-20.00));
        let b_close = bank(close, dec!(-20.00), "PayPal Europe", &[]);
        let b_far = bank(far, dec!(-20.00), "PayPal Europe", &[]);
        let close_id = b_close.id;

        let matches = find_matches(&[ptxn], &[b_close, b_far]);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].bank_transaction_id, close_id);
    }

    #[test]
    fn empty_inputs() {
        let d = NaiveDate::from_ymd_opt(2024, 6, 1).unwrap();
        assert!(find_matches(&[], &[]).is_empty());
        assert!(find_matches(&[paypal_txn("X", d, dec!(-1))], &[]).is_empty());
        assert!(find_matches(&[], &[bank(d, dec!(-1), "PayPal", &[])]).is_empty());
    }

    #[test]
    fn reference_takes_priority_over_amount_match() {
        let d = NaiveDate::from_ymd_opt(2024, 6, 1).unwrap();
        // Both PayPal txns have same amount, but only one matches by reference
        let p1 = paypal_txn("1046498634842", d, dec!(-32.00));
        let p2 = paypal_txn("TXN_OTHER", d, dec!(-32.00));
        let b1 = bank(
            d,
            dec!(-32.00),
            "PayPal Europe",
            &["remittanceinformation:1046498634842/PP.8173.PP"],
        );
        let b2 = bank(d, dec!(-32.00), "PayPal Europe", &[]);

        let matches = find_matches(&[p1, p2], &[b1, b2]);
        assert_eq!(matches.len(), 2);

        // The reference match should pair p1 with b1
        let ref_match = matches
            .iter()
            .find(|m| m.paypal_transaction_id == "1046498634842")
            .unwrap();
        // b1 was created first, and reference match happens in pass 1
        // before amount-based matching
        assert_ne!(ref_match.bank_transaction_id, uuid::Uuid::nil());
    }

    #[test]
    fn extract_reference_from_remittance() {
        let lines = vec![
            "mandatereference:43NJ2252D9V2E,creditorid:LU96ZZZ0000000000000000058,remittanceinformation:1046498634842/PP.8173.PP/. , Ihr Einkauf bei".to_owned(),
        ];
        assert_eq!(
            extract_paypal_reference(&lines),
            Some("1046498634842".to_owned())
        );
    }

    #[test]
    fn extract_reference_no_match() {
        let lines = vec!["some random remittance info".to_owned()];
        assert_eq!(extract_paypal_reference(&lines), None);
    }

    #[test]
    fn refund_does_not_match_payment() {
        let d = NaiveDate::from_ymd_opt(2024, 6, 1).unwrap();
        // PayPal refund is positive, bank payment is negative — should NOT match
        let ptxn = paypal_txn("TXN_REFUND", d, dec!(535.50));
        let btxn = bank(d, dec!(-535.50), "PayPal Europe", &[]);

        let matches = find_matches(&[ptxn], &[btxn]);
        assert!(
            matches.is_empty(),
            "refund (+) should not match payment (-)"
        );
    }

    #[test]
    fn sign_must_match_exactly() {
        let d = NaiveDate::from_ymd_opt(2024, 6, 1).unwrap();
        // Both negative — should match
        let p1 = paypal_txn("TXN001", d, dec!(-15.00));
        let b1 = bank(d, dec!(-15.00), "PayPal Europe", &[]);
        assert_eq!(find_matches(&[p1], &[b1]).len(), 1);

        // Both positive (bank credit + PayPal refund) — should match
        let p2 = paypal_txn("TXN002", d, dec!(15.00));
        let b2 = bank(d, dec!(15.00), "PayPal Europe", &[]);
        assert_eq!(find_matches(&[p2], &[b2]).len(), 1);

        // Opposite signs — should NOT match
        let p3 = paypal_txn("TXN003", d, dec!(15.00));
        let b3 = bank(d, dec!(-15.00), "PayPal Europe", &[]);
        assert!(find_matches(&[p3], &[b3]).is_empty());
    }
}
