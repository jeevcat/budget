use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use super::enums::{
    AccountType, BudgetMode, BudgetType, CategoryMethod, ConnectionStatus, CorrelationType,
    MatchField, PaceIndicator, RuleType,
};
use super::id::{AccountId, BudgetMonthId, CategoryId, ConnectionId, RuleId, TransactionId};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: AccountId,
    pub provider_account_id: String,
    pub name: String,
    pub nickname: Option<String>,
    pub institution: String,
    pub account_type: AccountType,
    pub currency: String,
    pub connection_id: Option<ConnectionId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connection {
    pub id: ConnectionId,
    pub provider: String,
    pub provider_session_id: String,
    pub institution_name: String,
    pub valid_until: DateTime<Utc>,
    pub status: ConnectionStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Category {
    pub id: CategoryId,
    pub name: String,
    pub parent_id: Option<CategoryId>,
    pub budget_mode: Option<BudgetMode>,
    pub budget_type: Option<BudgetType>,
    pub budget_amount: Option<Decimal>,
    pub project_start_date: Option<NaiveDate>,
    pub project_end_date: Option<NaiveDate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub id: TransactionId,
    pub account_id: AccountId,
    pub category_id: Option<CategoryId>,
    pub amount: Decimal,
    pub original_amount: Option<Decimal>,
    pub original_currency: Option<String>,
    pub merchant_name: String,
    /// Array of free-text payment detail lines from the bank.
    /// May contain "Key: Value" pairs, reference numbers, or plain text.
    /// Source: Enable Banking `remittance_information`
    pub remittance_information: Vec<String>,
    pub posted_date: NaiveDate,
    pub correlation_id: Option<TransactionId>,
    pub correlation_type: Option<CorrelationType>,
    pub category_method: Option<CategoryMethod>,
    pub suggested_category: Option<String>,
    pub counterparty_name: Option<String>,
    pub counterparty_iban: Option<String>,
    pub counterparty_bic: Option<String>,
    /// Human-readable bank transaction label (e.g. "Gehalt/Rente"). Bank-specific, not standardized.
    /// Source: Enable Banking `bank_transaction_code.description`
    pub bank_transaction_code: Option<String>,
    pub llm_justification: Option<String>,
    pub skip_correlation: bool,
    /// ISO 18245 MCC code (e.g. "5411" = grocery). Only present for card transactions.
    /// Source: Enable Banking `merchant_category_code`
    pub merchant_category_code: Option<String>,
    /// ISO 20022 domain code (e.g. "PMNT" for payments).
    /// Source: Enable Banking `bank_transaction_code.code`
    pub bank_transaction_code_code: Option<String>,
    /// ISO 20022 sub-family code (e.g. "ICDT-STDO").
    /// Source: Enable Banking `bank_transaction_code.sub_code`
    pub bank_transaction_code_sub_code: Option<String>,
    /// Actual FX rate applied (e.g. "1.0856"), stored as string to preserve bank precision.
    /// Source: Enable Banking `exchange_rate.exchange_rate`
    pub exchange_rate: Option<String>,
    /// ISO 4217 currency code in which the exchange rate is expressed.
    /// Source: Enable Banking `exchange_rate.unit_currency`
    pub exchange_rate_unit_currency: Option<String>,
    /// FX rate type: AGRD (agreed/contract), SALE, or SPOT.
    /// Source: Enable Banking `exchange_rate.rate_type`
    pub exchange_rate_type: Option<String>,
    /// FX contract reference when `rate_type` is AGRD (agreed).
    /// Source: Enable Banking `exchange_rate.contract_identification`
    pub exchange_rate_contract_id: Option<String>,
    /// Structured payment reference (e.g. "RF07850352502356628678117").
    /// Source: Enable Banking `reference_number`
    pub reference_number: Option<String>,
    /// Scheme of the reference number: BERF, FIRF, INTL, NORF, SDDM, SEBG.
    /// Source: Enable Banking `reference_number_schema`
    pub reference_number_schema: Option<String>,
    /// Internal note made by PSU (Payment Service User), distinct from remittance info.
    /// Source: Enable Banking `note`
    pub note: Option<String>,
    /// Account balance after this transaction (amount component).
    /// Source: Enable Banking `balance_after_transaction.amount`
    pub balance_after_transaction: Option<Decimal>,
    /// Currency of the balance after transaction (usually same as account currency).
    /// Source: Enable Banking `balance_after_transaction.currency`
    pub balance_after_transaction_currency: Option<String>,
    /// Non-IBAN creditor account IDs: JSONB array of `{identification, scheme_name, issuer}`.
    /// Source: Enable Banking `creditor_account_additional_identification`
    pub creditor_account_additional_id: Option<serde_json::Value>,
    /// Non-IBAN debtor account IDs: JSONB array of `{identification, scheme_name, issuer}`.
    /// Source: Enable Banking `debtor_account_additional_identification`
    pub debtor_account_additional_id: Option<serde_json::Value>,
}

impl Default for Transaction {
    fn default() -> Self {
        Self {
            id: TransactionId::new(),
            account_id: AccountId::new(),
            category_id: None,
            amount: Decimal::ZERO,
            original_amount: None,
            original_currency: None,
            merchant_name: String::new(),
            remittance_information: Vec::new(),
            posted_date: NaiveDate::from_ymd_opt(2025, 1, 1).expect("valid date"),
            correlation_id: None,
            correlation_type: None,
            category_method: None,
            suggested_category: None,
            counterparty_name: None,
            counterparty_iban: None,
            counterparty_bic: None,
            bank_transaction_code: None,
            llm_justification: None,
            skip_correlation: false,
            merchant_category_code: None,
            bank_transaction_code_code: None,
            bank_transaction_code_sub_code: None,
            exchange_rate: None,
            exchange_rate_unit_currency: None,
            exchange_rate_type: None,
            exchange_rate_contract_id: None,
            reference_number: None,
            reference_number_schema: None,
            note: None,
            balance_after_transaction: None,
            balance_after_transaction_currency: None,
            creditor_account_additional_id: None,
            debtor_account_additional_id: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleCondition {
    pub field: MatchField,
    pub pattern: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub id: RuleId,
    pub rule_type: RuleType,
    pub conditions: Vec<RuleCondition>,
    pub target_category_id: Option<CategoryId>,
    pub target_correlation_type: Option<CorrelationType>,
    pub priority: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetMonth {
    pub id: BudgetMonthId,
    pub start_date: NaiveDate,
    pub end_date: Option<NaiveDate>,
    pub salary_transactions_detected: i32,
}

impl Category {
    /// Build the fully-qualified display name for this category using the
    /// `"Parent:Child"` convention. If this category has a parent, the result
    /// is `"ParentName:ChildName"` (using the child's stored `name` — stripping
    /// any redundant parent prefix if the stored name already contains one).
    /// Root categories return their `name` as-is.
    ///
    /// `parent_name` should be the `name` field of the parent [`Category`] when
    /// `self.parent_id` is `Some`. Pass `None` for root categories.
    #[must_use]
    pub fn qualified_name(&self, parent_name: Option<&str>) -> String {
        match parent_name {
            Some(p) => {
                let leaf = self.leaf_name(Some(p));
                format!("{p}:{leaf}")
            }
            None => self.name.clone(),
        }
    }

    /// Extract just the leaf (child) portion of this category's name.
    ///
    /// If the stored name already contains the parent prefix (e.g.
    /// `"Food:Groceries"` with parent `"Food"`), the prefix is stripped.
    /// Otherwise the name is returned as-is.
    #[must_use]
    pub fn leaf_name(&self, parent_name: Option<&str>) -> String {
        if let Some(p) = parent_name {
            let prefix = format!("{p}:");
            if self.name.starts_with(&prefix) {
                return self.name[prefix.len()..].to_owned();
            }
        }
        self.name.clone()
    }
}

/// Build a lookup from category ID to qualified name for a set of categories.
///
/// This handles both naming conventions: categories whose `name` already
/// contains the parent prefix (`"Food:Groceries"`) and those that store only
/// the leaf name (`"Groceries"` with `parent_id` pointing to `"Food"`).
#[must_use]
pub fn build_qualified_name_map(
    categories: &[Category],
) -> std::collections::HashMap<CategoryId, String> {
    let by_id: std::collections::HashMap<CategoryId, &Category> =
        categories.iter().map(|c| (c.id, c)).collect();
    categories
        .iter()
        .map(|c| {
            let parent_name = c
                .parent_id
                .and_then(|pid| by_id.get(&pid))
                .map(|p| p.name.as_str());
            (c.id, c.qualified_name(parent_name))
        })
        .collect()
}

/// Parse a colon-separated qualified category name into (parent, child) parts.
///
/// Returns `Some((parent, child))` if the name contains at least one colon
/// (e.g. `"Food:Groceries"` → `Some(("Food", "Groceries"))`).
/// Returns `None` for root-level names without a colon.
///
/// Multi-level names split at the *first* colon only:
/// `"A:B:C"` → `Some(("A", "B:C"))`.
#[must_use]
pub fn parse_qualified_name(name: &str) -> Option<(&str, &str)> {
    name.split_once(':')
}

/// Spending breakdown for a direct child of a project category.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectChildSpending {
    pub category_id: CategoryId,
    pub category_name: String,
    pub spent: Decimal,
}

/// Result of computing budget status for a category in a budget month
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetStatus {
    pub category_id: CategoryId,
    pub category_name: String,
    pub budget_amount: Decimal,
    pub spent: Decimal,
    pub remaining: Decimal,
    /// Monthly = days left, Annual = months left, Project = days left (-1 if open-ended)
    pub time_left: i64,
    pub pace: PaceIndicator,
    /// Signed deviation from pro-rata expected spend (`spent - expected`).
    /// Positive = over pace, negative = under pace.
    pub pace_delta: Decimal,
    pub budget_mode: BudgetMode,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cat(id: u128, name: &str, parent_id: Option<u128>) -> Category {
        Category {
            id: CategoryId::from_uuid(uuid::Uuid::from_u128(id)),
            name: name.to_owned(),
            parent_id: parent_id.map(|p| CategoryId::from_uuid(uuid::Uuid::from_u128(p))),
            budget_mode: None,
            budget_type: None,
            budget_amount: None,
            project_start_date: None,
            project_end_date: None,
        }
    }

    // -----------------------------------------------------------------------
    // parse_qualified_name
    // -----------------------------------------------------------------------

    #[test]
    fn parse_qualified_name_with_colon() {
        assert_eq!(
            parse_qualified_name("Food:Groceries"),
            Some(("Food", "Groceries"))
        );
    }

    #[test]
    fn parse_qualified_name_without_colon() {
        assert_eq!(parse_qualified_name("Cash"), None);
    }

    #[test]
    fn parse_qualified_name_empty_string() {
        assert_eq!(parse_qualified_name(""), None);
    }

    #[test]
    fn parse_qualified_name_multi_colon_splits_at_first() {
        assert_eq!(parse_qualified_name("A:B:C"), Some(("A", "B:C")));
    }

    #[test]
    fn parse_qualified_name_colon_at_start() {
        assert_eq!(parse_qualified_name(":Child"), Some(("", "Child")));
    }

    #[test]
    fn parse_qualified_name_colon_at_end() {
        assert_eq!(parse_qualified_name("Parent:"), Some(("Parent", "")));
    }

    // -----------------------------------------------------------------------
    // Category::leaf_name
    // -----------------------------------------------------------------------

    #[test]
    fn leaf_name_strips_parent_prefix() {
        let c = cat(1, "Food:Groceries", Some(2));
        assert_eq!(c.leaf_name(Some("Food")), "Groceries");
    }

    #[test]
    fn leaf_name_returns_raw_when_no_prefix() {
        let c = cat(1, "Groceries", Some(2));
        assert_eq!(c.leaf_name(Some("Food")), "Groceries");
    }

    #[test]
    fn leaf_name_with_no_parent() {
        let c = cat(1, "Cash", None);
        assert_eq!(c.leaf_name(None), "Cash");
    }

    #[test]
    fn leaf_name_parent_prefix_is_case_sensitive() {
        let c = cat(1, "food:Groceries", Some(2));
        // "Food" prefix doesn't match "food:..." → returns raw name
        assert_eq!(c.leaf_name(Some("Food")), "food:Groceries");
    }

    // -----------------------------------------------------------------------
    // Category::qualified_name
    // -----------------------------------------------------------------------

    #[test]
    fn qualified_name_root_category() {
        let c = cat(1, "Cash", None);
        assert_eq!(c.qualified_name(None), "Cash");
    }

    #[test]
    fn qualified_name_child_with_leaf_name() {
        // Child stored as just "Groceries" under parent "Food"
        let c = cat(1, "Groceries", Some(2));
        assert_eq!(c.qualified_name(Some("Food")), "Food:Groceries");
    }

    #[test]
    fn qualified_name_child_already_has_prefix() {
        // Child stored as "Food:Groceries" under parent "Food"
        let c = cat(1, "Food:Groceries", Some(2));
        // Should still produce "Food:Groceries", not "Food:Food:Groceries"
        assert_eq!(c.qualified_name(Some("Food")), "Food:Groceries");
    }

    // -----------------------------------------------------------------------
    // build_qualified_name_map
    // -----------------------------------------------------------------------

    #[test]
    fn qualified_map_root_only() {
        let categories = vec![cat(1, "Cash", None), cat(2, "Income", None)];
        let map = build_qualified_name_map(&categories);
        assert_eq!(map.len(), 2);
        assert_eq!(
            map[&CategoryId::from_uuid(uuid::Uuid::from_u128(1))],
            "Cash"
        );
        assert_eq!(
            map[&CategoryId::from_uuid(uuid::Uuid::from_u128(2))],
            "Income"
        );
    }

    #[test]
    fn qualified_map_parent_child_leaf_name() {
        // Child stored as simple "Groceries"
        let categories = vec![cat(1, "Food", None), cat(2, "Groceries", Some(1))];
        let map = build_qualified_name_map(&categories);
        assert_eq!(
            map[&CategoryId::from_uuid(uuid::Uuid::from_u128(1))],
            "Food"
        );
        assert_eq!(
            map[&CategoryId::from_uuid(uuid::Uuid::from_u128(2))],
            "Food:Groceries"
        );
    }

    #[test]
    fn qualified_map_parent_child_colon_name() {
        // Child stored as "Food:Groceries" (legacy colon convention)
        let categories = vec![cat(1, "Food", None), cat(2, "Food:Groceries", Some(1))];
        let map = build_qualified_name_map(&categories);
        assert_eq!(
            map[&CategoryId::from_uuid(uuid::Uuid::from_u128(2))],
            "Food:Groceries"
        );
    }

    #[test]
    fn qualified_map_orphan_child_keeps_raw_name() {
        // Child has parent_id but parent not in the list (e.g. deleted)
        let categories = vec![cat(2, "Groceries", Some(99))];
        let map = build_qualified_name_map(&categories);
        // No parent found → falls back to raw name
        assert_eq!(
            map[&CategoryId::from_uuid(uuid::Uuid::from_u128(2))],
            "Groceries"
        );
    }

    #[test]
    fn qualified_map_multiple_children() {
        let categories = vec![
            cat(1, "Food", None),
            cat(2, "Groceries", Some(1)),
            cat(3, "Restaurants", Some(1)),
            cat(4, "Cash", None),
        ];
        let map = build_qualified_name_map(&categories);
        assert_eq!(map.len(), 4);
        assert_eq!(
            map[&CategoryId::from_uuid(uuid::Uuid::from_u128(2))],
            "Food:Groceries"
        );
        assert_eq!(
            map[&CategoryId::from_uuid(uuid::Uuid::from_u128(3))],
            "Food:Restaurants"
        );
        assert_eq!(
            map[&CategoryId::from_uuid(uuid::Uuid::from_u128(4))],
            "Cash"
        );
    }

    #[test]
    fn qualified_map_mixed_naming_conventions() {
        // Mix of old (colon-stored) and new (leaf-stored) conventions
        let categories = vec![
            cat(1, "Food", None),
            cat(2, "Food:Groceries", Some(1)), // old convention
            cat(3, "Restaurants", Some(1)),    // new convention
        ];
        let map = build_qualified_name_map(&categories);
        assert_eq!(
            map[&CategoryId::from_uuid(uuid::Uuid::from_u128(2))],
            "Food:Groceries"
        );
        assert_eq!(
            map[&CategoryId::from_uuid(uuid::Uuid::from_u128(3))],
            "Food:Restaurants"
        );
    }
}
