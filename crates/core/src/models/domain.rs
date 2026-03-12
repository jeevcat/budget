use std::fmt;

use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use super::enums::{
    AccountType, BudgetMode, BudgetType, CategoryMethod, ConnectionStatus, CorrelationType,
    MatchField, PaceIndicator, RuleType,
};
use super::id::{AccountId, BudgetMonthId, CategoryId, ConnectionId, RuleId, TransactionId};
use super::newtypes::{
    Bic, CurrencyCode, DomainCode, ExchangeRateType, Iban, MerchantCategoryCode, Priority,
    ReferenceNumberSchema, SubFamilyCode,
};
use crate::error::Error;

// ---------------------------------------------------------------------------
// BudgetConfig — replaces 5 independent Option fields on Category
// ---------------------------------------------------------------------------

/// Budget configuration for a category.
///
/// Encodes the budget mode and its mode-specific parameters as a single enum,
/// making impossible states unrepresentable (e.g. a Monthly category cannot
/// carry project dates, and a Project always has a start date and amount).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BudgetConfig {
    /// No budget set for this category.
    None,
    /// Resets each budget month.
    Monthly {
        amount: Decimal,
        budget_type: BudgetType,
    },
    /// Cumulative across the calendar year.
    Annual {
        amount: Decimal,
        budget_type: BudgetType,
    },
    /// Time-bounded project.
    Project {
        amount: Decimal,
        start_date: NaiveDate,
        end_date: Option<NaiveDate>,
    },
    /// Income category (salary deposits). Excluded from budget math.
    Salary,
}

impl BudgetConfig {
    /// The budget mode, or `None` for unbudgeted categories.
    #[must_use]
    pub fn mode(&self) -> Option<BudgetMode> {
        match self {
            Self::None => Option::None,
            Self::Monthly { .. } => Some(BudgetMode::Monthly),
            Self::Annual { .. } => Some(BudgetMode::Annual),
            Self::Project { .. } => Some(BudgetMode::Project),
            Self::Salary => Some(BudgetMode::Salary),
        }
    }

    /// The budget amount, or `None` for unbudgeted categories.
    #[must_use]
    pub fn amount(&self) -> Option<Decimal> {
        match self {
            Self::None | Self::Salary => Option::None,
            Self::Monthly { amount, .. }
            | Self::Annual { amount, .. }
            | Self::Project { amount, .. } => Some(*amount),
        }
    }

    /// The budget type (Fixed/Variable), or `None` for unbudgeted/project categories.
    #[must_use]
    pub fn budget_type(&self) -> Option<BudgetType> {
        match self {
            Self::Monthly { budget_type, .. } | Self::Annual { budget_type, .. } => {
                Some(*budget_type)
            }
            Self::None | Self::Salary | Self::Project { .. } => Option::None,
        }
    }

    /// Construct from the flat DB/API representation.
    ///
    /// Unrecognised combinations (e.g. Project without a start date) fall back
    /// to `BudgetConfig::None`.
    #[must_use]
    pub fn from_parts(
        mode: Option<BudgetMode>,
        budget_type: Option<BudgetType>,
        amount: Option<Decimal>,
        start_date: Option<NaiveDate>,
        end_date: Option<NaiveDate>,
    ) -> Self {
        match mode {
            Some(BudgetMode::Monthly) => Self::Monthly {
                amount: amount.unwrap_or(Decimal::ZERO),
                budget_type: budget_type.unwrap_or(BudgetType::Variable),
            },
            Some(BudgetMode::Annual) => Self::Annual {
                amount: amount.unwrap_or(Decimal::ZERO),
                budget_type: budget_type.unwrap_or(BudgetType::Variable),
            },
            Some(BudgetMode::Project) => match start_date {
                Some(start) => Self::Project {
                    amount: amount.unwrap_or(Decimal::ZERO),
                    start_date: start,
                    end_date,
                },
                // Project without a start date is invalid; fall back
                Option::None => Self::None,
            },
            Some(BudgetMode::Salary) => Self::Salary,
            Option::None => Self::None,
        }
    }
}

/// Flat serialization: emit the same JSON shape the frontend/mobile expect.
impl Serialize for BudgetConfig {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;

        let mut map = serializer.serialize_map(Option::None)?;
        map.serialize_entry("budget_mode", &self.mode())?;
        map.serialize_entry("budget_type", &self.budget_type())?;
        map.serialize_entry("budget_amount", &self.amount())?;
        if let Self::Project {
            start_date,
            end_date,
            ..
        } = self
        {
            map.serialize_entry("project_start_date", &Some(start_date))?;
            map.serialize_entry("project_end_date", end_date)?;
        } else {
            map.serialize_entry("project_start_date", &Option::<NaiveDate>::None)?;
            map.serialize_entry("project_end_date", &Option::<NaiveDate>::None)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for BudgetConfig {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Flat {
            budget_mode: Option<BudgetMode>,
            budget_type: Option<BudgetType>,
            budget_amount: Option<Decimal>,
            project_start_date: Option<NaiveDate>,
            project_end_date: Option<NaiveDate>,
        }
        let f = Flat::deserialize(deserializer)?;
        Ok(Self::from_parts(
            f.budget_mode,
            f.budget_type,
            f.budget_amount,
            f.project_start_date,
            f.project_end_date,
        ))
    }
}

// ---------------------------------------------------------------------------
// Correlation — replaces two independent Option fields on Transaction
// ---------------------------------------------------------------------------

/// A correlation link to another transaction (transfer or reimbursement).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Correlation {
    pub partner_id: TransactionId,
    pub correlation_type: CorrelationType,
}

/// Maximum length for a category name (UTF-8 bytes).
const MAX_CATEGORY_NAME_LEN: usize = 100;

/// A validated category name.
///
/// Invariants enforced at construction:
/// - Non-empty after trimming
/// - No leading or trailing whitespace (callers must trim first, or the
///   constructor rejects it so the error is visible rather than silently lost)
/// - No colons — hierarchy is expressed via `parent_id`, not embedded in names
/// - At most [`MAX_CATEGORY_NAME_LEN`] UTF-8 bytes
/// - No control characters (U+0000–U+001F, U+007F–U+009F)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct CategoryName(String);

impl CategoryName {
    /// Create a new `CategoryName`, validating all invariants.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidCategoryName`] if the name is empty, has
    /// leading/trailing whitespace, contains colons or control characters,
    /// or exceeds [`MAX_CATEGORY_NAME_LEN`] bytes.
    pub fn new(name: impl Into<String>) -> Result<Self, Error> {
        let name = name.into();

        if name.is_empty() {
            return Err(Error::InvalidCategoryName("name is empty".into()));
        }

        if name != name.trim() {
            return Err(Error::InvalidCategoryName(
                "name has leading or trailing whitespace".into(),
            ));
        }

        if name.contains(':') {
            return Err(Error::InvalidCategoryName(
                "name contains a colon — use parent_id for hierarchy".into(),
            ));
        }

        if name.len() > MAX_CATEGORY_NAME_LEN {
            return Err(Error::InvalidCategoryName(format!(
                "name exceeds {MAX_CATEGORY_NAME_LEN} bytes"
            )));
        }

        if name.chars().any(char::is_control) {
            return Err(Error::InvalidCategoryName(
                "name contains control characters".into(),
            ));
        }

        Ok(Self(name))
    }
}

impl fmt::Display for CategoryName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl PartialEq<&str> for CategoryName {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl AsRef<str> for CategoryName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for CategoryName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        CategoryName::new(s).map_err(serde::de::Error::custom)
    }
}

impl std::str::FromStr for CategoryName {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        CategoryName::new(s)
    }
}

// sqlx support — delegates to the inner String (TEXT column)

impl sqlx::Type<sqlx::Postgres> for CategoryName {
    fn type_info() -> sqlx::postgres::PgTypeInfo {
        <String as sqlx::Type<sqlx::Postgres>>::type_info()
    }

    fn compatible(ty: &sqlx::postgres::PgTypeInfo) -> bool {
        <String as sqlx::Type<sqlx::Postgres>>::compatible(ty)
    }
}

impl sqlx::Encode<'_, sqlx::Postgres> for CategoryName {
    fn encode_by_ref(
        &self,
        buf: &mut sqlx::postgres::PgArgumentBuffer,
    ) -> Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
        <String as sqlx::Encode<'_, sqlx::Postgres>>::encode_by_ref(&self.0, buf)
    }
}

impl<'r> sqlx::Decode<'r, sqlx::Postgres> for CategoryName {
    /// Decode from Postgres TEXT without validation — the DB may contain legacy
    /// colon-names that will be cleaned up by migration.
    fn decode(value: sqlx::postgres::PgValueRef<'r>) -> Result<Self, sqlx::error::BoxDynError> {
        let s = <String as sqlx::Decode<'r, sqlx::Postgres>>::decode(value)?;
        Ok(Self(s))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: AccountId,
    pub provider_account_id: String,
    pub name: String,
    pub nickname: Option<String>,
    pub institution: String,
    pub account_type: AccountType,
    pub currency: CurrencyCode,
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
    pub name: CategoryName,
    pub parent_id: Option<CategoryId>,
    /// Budget configuration (mode + mode-specific parameters).
    /// Serializes as flat fields for API compatibility.
    #[serde(flatten)]
    pub budget: BudgetConfig,
}

// ---------------------------------------------------------------------------
// Categorization — replaces two independent Option fields on Transaction
// ---------------------------------------------------------------------------

/// How a transaction is categorized: uncategorized, or by manual/rule/LLM method.
///
/// Collapses `category_id: Option<CategoryId>` + `category_method: Option<CategoryMethod>`
/// into a single sum type, making invalid states (e.g. method without ID) unrepresentable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Categorization {
    Uncategorized,
    Manual(CategoryId),
    Rule(CategoryId),
    Llm(CategoryId),
}

impl Categorization {
    /// The assigned category ID, if categorized.
    #[must_use]
    pub fn category_id(&self) -> Option<CategoryId> {
        match self {
            Self::Uncategorized => None,
            Self::Manual(id) | Self::Rule(id) | Self::Llm(id) => Some(*id),
        }
    }

    /// The categorization method, if categorized.
    #[must_use]
    pub fn method(&self) -> Option<CategoryMethod> {
        match self {
            Self::Uncategorized => None,
            Self::Manual(_) => Some(CategoryMethod::Manual),
            Self::Rule(_) => Some(CategoryMethod::Rule),
            Self::Llm(_) => Some(CategoryMethod::Llm),
        }
    }

    /// Whether the transaction has a category assigned.
    #[must_use]
    pub fn is_categorized(&self) -> bool {
        !matches!(self, Self::Uncategorized)
    }

    /// Construct from the flat DB/API representation.
    ///
    /// Falls back to `Uncategorized` when `category_id` is `None`.
    #[must_use]
    pub fn from_parts(category_id: Option<CategoryId>, method: Option<CategoryMethod>) -> Self {
        match (category_id, method) {
            (Some(id), Some(CategoryMethod::Rule)) => Self::Rule(id),
            (Some(id), Some(CategoryMethod::Llm)) => Self::Llm(id),
            // ID with explicit Manual or missing method defaults to Manual
            (Some(id), Some(CategoryMethod::Manual) | None) => Self::Manual(id),
            (None, _) => Self::Uncategorized,
        }
    }
}

/// Flat serde for `Categorization` — serializes/deserializes as two
/// top-level fields `category_id` and `category_method` for API compat.
mod categorization_serde {
    use super::{Categorization, CategoryId, CategoryMethod};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize)]
    struct Flat {
        category_id: Option<CategoryId>,
        category_method: Option<CategoryMethod>,
    }

    #[derive(Deserialize)]
    struct FlatDe {
        category_id: Option<CategoryId>,
        category_method: Option<CategoryMethod>,
    }

    pub fn serialize<S: Serializer>(
        value: &Categorization,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let flat = Flat {
            category_id: value.category_id(),
            category_method: value.method(),
        };
        flat.serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Categorization, D::Error> {
        let flat = FlatDe::deserialize(deserializer)?;
        Ok(Categorization::from_parts(
            flat.category_id,
            flat.category_method,
        ))
    }
}

/// Flat serde for `Option<Correlation>` — serializes/deserializes as two
/// top-level fields `correlation_id` and `correlation_type` for API compat.
mod correlation_serde {
    use super::{Correlation, CorrelationType, TransactionId};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize)]
    struct Flat {
        correlation_id: Option<TransactionId>,
        correlation_type: Option<CorrelationType>,
    }

    #[derive(Deserialize)]
    struct FlatDe {
        correlation_id: Option<TransactionId>,
        correlation_type: Option<CorrelationType>,
    }

    #[allow(clippy::ref_option)] // serde's serialize_with contract requires &Option<T>
    pub fn serialize<S: Serializer>(
        value: &Option<Correlation>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let flat = match value {
            Some(c) => Flat {
                correlation_id: Some(c.partner_id),
                correlation_type: Some(c.correlation_type),
            },
            None => Flat {
                correlation_id: None,
                correlation_type: None,
            },
        };
        flat.serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Correlation>, D::Error> {
        let flat = FlatDe::deserialize(deserializer)?;
        Ok(match (flat.correlation_id, flat.correlation_type) {
            (Some(partner_id), Some(correlation_type)) => Some(Correlation {
                partner_id,
                correlation_type,
            }),
            _ => None,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub id: TransactionId,
    pub account_id: AccountId,
    /// Categorization state: uncategorized, or categorized by manual/rule/LLM.
    /// Serialized as flat `category_id` + `category_method` fields.
    #[serde(flatten, with = "categorization_serde")]
    pub categorization: Categorization,
    pub amount: Decimal,
    pub original_amount: Option<Decimal>,
    pub original_currency: Option<CurrencyCode>,
    pub merchant_name: String,
    /// Array of free-text payment detail lines from the bank.
    /// May contain "Key: Value" pairs, reference numbers, or plain text.
    /// Source: Enable Banking `remittance_information`
    pub remittance_information: Vec<String>,
    pub posted_date: NaiveDate,
    /// Correlation link to a partner transaction (transfer or reimbursement).
    /// Serialized as flat `correlation_id` + `correlation_type` fields.
    #[serde(flatten, with = "correlation_serde")]
    pub correlation: Option<Correlation>,
    pub suggested_category: Option<String>,
    pub counterparty_name: Option<String>,
    pub counterparty_iban: Option<Iban>,
    pub counterparty_bic: Option<Bic>,
    /// Human-readable bank transaction label (e.g. "Gehalt/Rente"). Bank-specific, not standardized.
    /// Source: Enable Banking `bank_transaction_code.description`
    pub bank_transaction_code: Option<String>,
    pub llm_justification: Option<String>,
    pub skip_correlation: bool,
    /// ISO 18245 MCC code (e.g. "5411" = grocery). Only present for card transactions.
    /// Source: Enable Banking `merchant_category_code`
    pub merchant_category_code: Option<MerchantCategoryCode>,
    /// ISO 20022 domain code (e.g. "PMNT" for payments).
    /// Source: Enable Banking `bank_transaction_code.code`
    pub bank_transaction_code_code: Option<DomainCode>,
    /// ISO 20022 sub-family code (e.g. "ICDT-STDO").
    /// Source: Enable Banking `bank_transaction_code.sub_code`
    pub bank_transaction_code_sub_code: Option<SubFamilyCode>,
    /// Actual FX rate applied (e.g. "1.0856"), stored as string to preserve bank precision.
    /// Source: Enable Banking `exchange_rate.exchange_rate`
    pub exchange_rate: Option<String>,
    /// ISO 4217 currency code in which the exchange rate is expressed.
    /// Source: Enable Banking `exchange_rate.unit_currency`
    pub exchange_rate_unit_currency: Option<CurrencyCode>,
    /// FX rate type: AGRD (agreed/contract), SALE, or SPOT.
    /// Source: Enable Banking `exchange_rate.rate_type`
    pub exchange_rate_type: Option<ExchangeRateType>,
    /// FX contract reference when `rate_type` is AGRD (agreed).
    /// Source: Enable Banking `exchange_rate.contract_identification`
    pub exchange_rate_contract_id: Option<String>,
    /// Structured payment reference (e.g. "RF07850352502356628678117").
    /// Source: Enable Banking `reference_number`
    pub reference_number: Option<String>,
    /// Scheme of the reference number: BERF, FIRF, INTL, NORF, SDDM, SEBG.
    /// Source: Enable Banking `reference_number_schema`
    pub reference_number_schema: Option<ReferenceNumberSchema>,
    /// Internal note made by PSU (Payment Service User), distinct from remittance info.
    /// Source: Enable Banking `note`
    pub note: Option<String>,
    /// Account balance after this transaction (amount component).
    /// Source: Enable Banking `balance_after_transaction.amount`
    pub balance_after_transaction: Option<Decimal>,
    /// Currency of the balance after transaction (usually same as account currency).
    /// Source: Enable Banking `balance_after_transaction.currency`
    pub balance_after_transaction_currency: Option<CurrencyCode>,
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
            categorization: Categorization::Uncategorized,
            amount: Decimal::ZERO,
            original_amount: None,
            original_currency: None,
            merchant_name: String::new(),
            remittance_information: Vec::new(),
            posted_date: NaiveDate::from_ymd_opt(2025, 1, 1).expect("valid date"),
            correlation: None,
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
    pub priority: Priority,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetMonth {
    pub id: BudgetMonthId,
    pub start_date: NaiveDate,
    pub end_date: Option<NaiveDate>,
    pub salary_transactions_detected: u32,
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
            None => self.name.as_ref().to_owned(),
        }
    }

    /// Extract just the leaf (child) portion of this category's name.
    ///
    /// If the stored name already contains the parent prefix (e.g.
    /// `"Food:Groceries"` with parent `"Food"`), the prefix is stripped.
    /// Otherwise the name is returned as-is.
    #[must_use]
    pub fn leaf_name(&self, parent_name: Option<&str>) -> String {
        let name = self.name.as_ref();
        if let Some(p) = parent_name {
            let prefix = format!("{p}:");
            if name.starts_with(&prefix) {
                return name[prefix.len()..].to_owned();
            }
        }
        name.to_owned()
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
                .map(|p| p.name.as_ref());
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
    /// Time remaining in the budget period. `None` for open-ended projects.
    /// Unit: days for Monthly/Project, months for Annual.
    pub time_left: Option<i64>,
    pub pace: PaceIndicator,
    /// Signed deviation from pro-rata expected spend (`spent - expected`).
    /// Positive = over pace, negative = under pace.
    pub pace_delta: Decimal,
    pub budget_mode: BudgetMode,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a test category. Accepts any string including colon-names for
    /// testing legacy compat — bypasses `CategoryName` validation.
    fn cat(id: u128, name: &str, parent_id: Option<u128>) -> Category {
        Category {
            id: CategoryId::from_uuid(uuid::Uuid::from_u128(id)),
            name: CategoryName(name.to_owned()),
            parent_id: parent_id.map(|p| CategoryId::from_uuid(uuid::Uuid::from_u128(p))),
            budget: BudgetConfig::None,
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

    // -----------------------------------------------------------------------
    // CategoryName — construction
    // -----------------------------------------------------------------------

    #[test]
    fn category_name_valid_simple() {
        let name = CategoryName::new("Groceries").unwrap();
        assert_eq!(name.to_string(), "Groceries");
    }

    #[test]
    fn category_name_valid_with_spaces() {
        let name = CategoryName::new("Bank Fees").unwrap();
        assert_eq!(name.to_string(), "Bank Fees");
    }

    #[test]
    fn category_name_valid_single_char() {
        let name = CategoryName::new("X").unwrap();
        assert_eq!(name.to_string(), "X");
    }

    #[test]
    fn category_name_valid_at_max_length() {
        let s = "A".repeat(MAX_CATEGORY_NAME_LEN);
        let name = CategoryName::new(s.clone()).unwrap();
        assert_eq!(name.to_string(), s);
    }

    #[test]
    fn category_name_valid_unicode() {
        let name = CategoryName::new("Lebensmittel").unwrap();
        assert_eq!(name.to_string(), "Lebensmittel");
    }

    #[test]
    fn category_name_valid_unicode_emoji() {
        // Emoji are valid non-control characters
        let name = CategoryName::new("Food 🍕").unwrap();
        assert_eq!(name.to_string(), "Food 🍕");
    }

    #[test]
    fn category_name_valid_unicode_cjk() {
        let name = CategoryName::new("食料品").unwrap();
        assert_eq!(name.to_string(), "食料品");
    }

    #[test]
    fn category_name_valid_with_hyphens_and_ampersand() {
        let name = CategoryName::new("Health & Well-being").unwrap();
        assert_eq!(name.to_string(), "Health & Well-being");
    }

    #[test]
    fn category_name_valid_with_parentheses() {
        let name = CategoryName::new("Tax (2025)").unwrap();
        assert_eq!(name.to_string(), "Tax (2025)");
    }

    #[test]
    fn category_name_valid_with_numbers() {
        let name = CategoryName::new("Q4 2025 Budget").unwrap();
        assert_eq!(name.to_string(), "Q4 2025 Budget");
    }

    #[test]
    fn category_name_valid_with_slash() {
        let name = CategoryName::new("Food/Drink").unwrap();
        assert_eq!(name.to_string(), "Food/Drink");
    }

    // -----------------------------------------------------------------------
    // CategoryName — rejection
    // -----------------------------------------------------------------------

    #[test]
    fn category_name_rejects_empty() {
        let err = CategoryName::new("").unwrap_err();
        assert!(err.to_string().contains("empty"), "got: {err}");
    }

    #[test]
    fn category_name_rejects_colon() {
        let err = CategoryName::new("Food:Groceries").unwrap_err();
        assert!(err.to_string().contains("colon"), "got: {err}");
    }

    #[test]
    fn category_name_rejects_colon_at_start() {
        let err = CategoryName::new(":Groceries").unwrap_err();
        assert!(err.to_string().contains("colon"), "got: {err}");
    }

    #[test]
    fn category_name_rejects_colon_at_end() {
        let err = CategoryName::new("Food:").unwrap_err();
        assert!(err.to_string().contains("colon"), "got: {err}");
    }

    #[test]
    fn category_name_rejects_multiple_colons() {
        let err = CategoryName::new("A:B:C").unwrap_err();
        assert!(err.to_string().contains("colon"), "got: {err}");
    }

    #[test]
    fn category_name_rejects_just_a_colon() {
        let err = CategoryName::new(":").unwrap_err();
        assert!(err.to_string().contains("colon"), "got: {err}");
    }

    #[test]
    fn category_name_rejects_leading_space() {
        let err = CategoryName::new(" Groceries").unwrap_err();
        assert!(err.to_string().contains("whitespace"), "got: {err}");
    }

    #[test]
    fn category_name_rejects_trailing_space() {
        let err = CategoryName::new("Groceries ").unwrap_err();
        assert!(err.to_string().contains("whitespace"), "got: {err}");
    }

    #[test]
    fn category_name_rejects_leading_and_trailing_spaces() {
        let err = CategoryName::new("  Groceries  ").unwrap_err();
        assert!(err.to_string().contains("whitespace"), "got: {err}");
    }

    #[test]
    fn category_name_rejects_whitespace_only() {
        let err = CategoryName::new("   ").unwrap_err();
        assert!(err.to_string().contains("whitespace"), "got: {err}");
    }

    #[test]
    fn category_name_rejects_tab() {
        let err = CategoryName::new("\tGroceries").unwrap_err();
        // Tab is both leading whitespace and a control character
        assert!(
            err.to_string().contains("whitespace") || err.to_string().contains("control"),
            "got: {err}"
        );
    }

    #[test]
    fn category_name_rejects_newline() {
        let err = CategoryName::new("Foo\nBar").unwrap_err();
        assert!(err.to_string().contains("control"), "got: {err}");
    }

    #[test]
    fn category_name_rejects_carriage_return() {
        let err = CategoryName::new("Foo\rBar").unwrap_err();
        assert!(err.to_string().contains("control"), "got: {err}");
    }

    #[test]
    fn category_name_rejects_null_byte() {
        let err = CategoryName::new("Foo\0Bar").unwrap_err();
        assert!(err.to_string().contains("control"), "got: {err}");
    }

    #[test]
    fn category_name_rejects_bell() {
        let err = CategoryName::new("Foo\x07Bar").unwrap_err();
        assert!(err.to_string().contains("control"), "got: {err}");
    }

    #[test]
    fn category_name_rejects_escape() {
        let err = CategoryName::new("Foo\x1BBar").unwrap_err();
        assert!(err.to_string().contains("control"), "got: {err}");
    }

    #[test]
    fn category_name_rejects_delete_char() {
        let err = CategoryName::new("Foo\x7FBar").unwrap_err();
        assert!(err.to_string().contains("control"), "got: {err}");
    }

    #[test]
    fn category_name_rejects_c1_control_char() {
        // U+0080 is a C1 control character
        let err = CategoryName::new("Foo\u{0080}Bar").unwrap_err();
        assert!(err.to_string().contains("control"), "got: {err}");
    }

    #[test]
    fn category_name_rejects_exceeding_max_length() {
        let s = "A".repeat(MAX_CATEGORY_NAME_LEN + 1);
        let err = CategoryName::new(s).unwrap_err();
        assert!(err.to_string().contains("exceeds"), "got: {err}");
    }

    #[test]
    fn category_name_rejects_way_over_max_length() {
        let s = "B".repeat(10_000);
        let err = CategoryName::new(s).unwrap_err();
        assert!(err.to_string().contains("exceeds"), "got: {err}");
    }

    // -----------------------------------------------------------------------
    // CategoryName — trait impls
    // -----------------------------------------------------------------------

    #[test]
    fn category_name_display() {
        let name = CategoryName::new("Groceries").unwrap();
        assert_eq!(format!("{name}"), "Groceries");
    }

    #[test]
    fn category_name_as_ref_str() {
        let name = CategoryName::new("Groceries").unwrap();
        let s: &str = name.as_ref();
        assert_eq!(s, "Groceries");
    }

    #[test]
    fn category_name_partial_eq_str() {
        let name = CategoryName::new("Cash").unwrap();
        assert_eq!(name, "Cash");
    }

    #[test]
    fn category_name_partial_eq_str_negative() {
        let name = CategoryName::new("Cash").unwrap();
        assert_ne!(name, "Food");
    }

    #[test]
    fn category_name_eq_between_instances() {
        let a = CategoryName::new("Cash").unwrap();
        let b = CategoryName::new("Cash").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn category_name_ne_between_instances() {
        let a = CategoryName::new("Cash").unwrap();
        let b = CategoryName::new("Food").unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn category_name_clone() {
        let name = CategoryName::new("Rent").unwrap();
        let cloned = name.clone();
        assert_eq!(name, cloned);
    }

    #[test]
    fn category_name_debug() {
        let name = CategoryName::new("Rent").unwrap();
        let debug = format!("{name:?}");
        assert!(debug.contains("Rent"), "got: {debug}");
    }

    #[test]
    fn category_name_hash_consistent() {
        use std::collections::HashSet;
        let a = CategoryName::new("Cash").unwrap();
        let b = CategoryName::new("Cash").unwrap();
        let mut set = HashSet::new();
        set.insert(a);
        set.insert(b);
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn category_name_from_str() {
        let name: CategoryName = "Transport".parse().unwrap();
        assert_eq!(name, "Transport");
    }

    #[test]
    fn category_name_from_str_rejects_invalid() {
        let result: Result<CategoryName, _> = "A:B".parse();
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // CategoryName — serde roundtrip
    // -----------------------------------------------------------------------

    #[test]
    fn category_name_serde_roundtrip() {
        let name = CategoryName::new("Groceries").unwrap();
        let json = serde_json::to_string(&name).unwrap();
        assert_eq!(json, "\"Groceries\"");
        let back: CategoryName = serde_json::from_str(&json).unwrap();
        assert_eq!(back, name);
    }

    #[test]
    fn category_name_serde_serialize_transparent() {
        let name = CategoryName::new("Cash").unwrap();
        let val = serde_json::to_value(&name).unwrap();
        assert_eq!(val, serde_json::Value::String("Cash".into()));
    }

    #[test]
    fn category_name_serde_deserialize_rejects_colon() {
        let result: Result<CategoryName, _> = serde_json::from_str("\"A:B\"");
        assert!(result.is_err());
    }

    #[test]
    fn category_name_serde_deserialize_rejects_empty() {
        let result: Result<CategoryName, _> = serde_json::from_str("\"\"");
        assert!(result.is_err());
    }

    #[test]
    fn category_name_serde_deserialize_rejects_whitespace_only() {
        let result: Result<CategoryName, _> = serde_json::from_str("\"   \"");
        assert!(result.is_err());
    }

    #[test]
    fn category_name_serde_in_category_struct() {
        let c = Category {
            id: CategoryId::from_uuid(uuid::Uuid::from_u128(42)),
            name: CategoryName::new("Food").unwrap(),
            parent_id: None,
            budget: BudgetConfig::None,
        };
        let json = serde_json::to_string(&c).unwrap();
        assert!(json.contains("\"name\":\"Food\""));
        let back: Category = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "Food");
    }

    // -----------------------------------------------------------------------
    // CategoryName — edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn category_name_multibyte_at_max_boundary() {
        // 'é' is 2 UTF-8 bytes. Fill to exactly MAX_CATEGORY_NAME_LEN bytes.
        let count = MAX_CATEGORY_NAME_LEN / 2;
        let s = "é".repeat(count);
        assert_eq!(s.len(), MAX_CATEGORY_NAME_LEN);
        let name = CategoryName::new(s.clone()).unwrap();
        assert_eq!(name.to_string(), s);
    }

    #[test]
    fn category_name_multibyte_over_max_boundary() {
        // One more 'é' puts us over
        let count = (MAX_CATEGORY_NAME_LEN / 2) + 1;
        let s = "é".repeat(count);
        assert!(s.len() > MAX_CATEGORY_NAME_LEN);
        assert!(CategoryName::new(s).is_err());
    }

    #[test]
    fn category_name_inner_whitespace_preserved() {
        // Multiple internal spaces are fine
        let name = CategoryName::new("Bank  Fees").unwrap();
        assert_eq!(name.to_string(), "Bank  Fees");
    }

    #[test]
    fn category_name_just_numbers() {
        let name = CategoryName::new("12345").unwrap();
        assert_eq!(name.to_string(), "12345");
    }

    #[test]
    fn category_name_special_punctuation() {
        let name = CategoryName::new("Food & Drink - Misc.").unwrap();
        assert_eq!(name.to_string(), "Food & Drink - Misc.");
    }

    // -----------------------------------------------------------------------
    // Categorization — helpers
    // -----------------------------------------------------------------------

    #[test]
    fn categorization_uncategorized_helpers() {
        let c = Categorization::Uncategorized;
        assert!(c.category_id().is_none());
        assert!(c.method().is_none());
        assert!(!c.is_categorized());
    }

    #[test]
    fn categorization_manual_helpers() {
        let id = CategoryId::from_uuid(uuid::Uuid::from_u128(1));
        let c = Categorization::Manual(id);
        assert_eq!(c.category_id(), Some(id));
        assert_eq!(c.method(), Some(CategoryMethod::Manual));
        assert!(c.is_categorized());
    }

    #[test]
    fn categorization_rule_helpers() {
        let id = CategoryId::from_uuid(uuid::Uuid::from_u128(2));
        let c = Categorization::Rule(id);
        assert_eq!(c.category_id(), Some(id));
        assert_eq!(c.method(), Some(CategoryMethod::Rule));
    }

    #[test]
    fn categorization_llm_helpers() {
        let id = CategoryId::from_uuid(uuid::Uuid::from_u128(3));
        let c = Categorization::Llm(id);
        assert_eq!(c.category_id(), Some(id));
        assert_eq!(c.method(), Some(CategoryMethod::Llm));
    }

    #[test]
    fn categorization_from_parts_all_variants() {
        let id = CategoryId::from_uuid(uuid::Uuid::from_u128(1));

        assert_eq!(
            Categorization::from_parts(None, None),
            Categorization::Uncategorized
        );
        assert_eq!(
            Categorization::from_parts(None, Some(CategoryMethod::Manual)),
            Categorization::Uncategorized
        );
        assert_eq!(
            Categorization::from_parts(Some(id), Some(CategoryMethod::Manual)),
            Categorization::Manual(id)
        );
        assert_eq!(
            Categorization::from_parts(Some(id), Some(CategoryMethod::Rule)),
            Categorization::Rule(id)
        );
        assert_eq!(
            Categorization::from_parts(Some(id), Some(CategoryMethod::Llm)),
            Categorization::Llm(id)
        );
        assert_eq!(
            Categorization::from_parts(Some(id), None),
            Categorization::Manual(id)
        );
    }

    // -----------------------------------------------------------------------
    // Categorization — serde roundtrip
    // -----------------------------------------------------------------------

    #[test]
    fn categorization_serde_roundtrip_uncategorized() {
        let txn = Transaction::default();
        let json = serde_json::to_value(&txn).unwrap();
        assert_eq!(json["category_id"], serde_json::Value::Null);
        assert_eq!(json["category_method"], serde_json::Value::Null);
        let back: Transaction = serde_json::from_value(json).unwrap();
        assert_eq!(back.categorization, Categorization::Uncategorized);
    }

    #[test]
    fn categorization_serde_roundtrip_manual() {
        let id = CategoryId::from_uuid(uuid::Uuid::from_u128(42));
        let mut txn = Transaction::default();
        txn.categorization = Categorization::Manual(id);
        let json = serde_json::to_value(&txn).unwrap();
        assert_eq!(json["category_id"], id.as_uuid().to_string());
        assert_eq!(json["category_method"], "manual");
        let back: Transaction = serde_json::from_value(json).unwrap();
        assert_eq!(back.categorization, Categorization::Manual(id));
    }

    #[test]
    fn categorization_serde_roundtrip_rule() {
        let id = CategoryId::from_uuid(uuid::Uuid::from_u128(99));
        let mut txn = Transaction::default();
        txn.categorization = Categorization::Rule(id);
        let json = serde_json::to_value(&txn).unwrap();
        assert_eq!(json["category_method"], "rule");
        let back: Transaction = serde_json::from_value(json).unwrap();
        assert_eq!(back.categorization, Categorization::Rule(id));
    }

    #[test]
    fn categorization_serde_roundtrip_llm() {
        let id = CategoryId::from_uuid(uuid::Uuid::from_u128(7));
        let mut txn = Transaction::default();
        txn.categorization = Categorization::Llm(id);
        let json = serde_json::to_value(&txn).unwrap();
        assert_eq!(json["category_method"], "llm");
        let back: Transaction = serde_json::from_value(json).unwrap();
        assert_eq!(back.categorization, Categorization::Llm(id));
    }
}
