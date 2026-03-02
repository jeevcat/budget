use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountType {
    Checking,
    Savings,
    CreditCard,
    Investment,
    Loan,
    Other,
}

impl fmt::Display for AccountType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Checking => write!(f, "checking"),
            Self::Savings => write!(f, "savings"),
            Self::CreditCard => write!(f, "credit_card"),
            Self::Investment => write!(f, "investment"),
            Self::Loan => write!(f, "loan"),
            Self::Other => write!(f, "other"),
        }
    }
}

impl std::str::FromStr for AccountType {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "checking" => Ok(Self::Checking),
            "savings" => Ok(Self::Savings),
            "credit_card" => Ok(Self::CreditCard),
            "investment" => Ok(Self::Investment),
            "loan" => Ok(Self::Loan),
            "other" => Ok(Self::Other),
            _ => Err(Error::InvalidAccountType(s.to_owned())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleType {
    Categorization,
    Correlation,
}

impl fmt::Display for RuleType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Categorization => write!(f, "categorization"),
            Self::Correlation => write!(f, "correlation"),
        }
    }
}

impl std::str::FromStr for RuleType {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "categorization" => Ok(Self::Categorization),
            "correlation" => Ok(Self::Correlation),
            _ => Err(Error::InvalidRuleType(s.to_owned())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchField {
    Merchant,
    Description,
    AmountRange,
    CounterpartyName,
    CounterpartyIban,
    CounterpartyBic,
    BankTransactionCode,
}

impl fmt::Display for MatchField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Merchant => write!(f, "merchant"),
            Self::Description => write!(f, "description"),
            Self::AmountRange => write!(f, "amount_range"),
            Self::CounterpartyName => write!(f, "counterparty_name"),
            Self::CounterpartyIban => write!(f, "counterparty_iban"),
            Self::CounterpartyBic => write!(f, "counterparty_bic"),
            Self::BankTransactionCode => write!(f, "bank_transaction_code"),
        }
    }
}

impl std::str::FromStr for MatchField {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "merchant" => Ok(Self::Merchant),
            "description" => Ok(Self::Description),
            "amount_range" => Ok(Self::AmountRange),
            "counterparty_name" => Ok(Self::CounterpartyName),
            "counterparty_iban" => Ok(Self::CounterpartyIban),
            "counterparty_bic" => Ok(Self::CounterpartyBic),
            "bank_transaction_code" => Ok(Self::BankTransactionCode),
            _ => Err(Error::InvalidMatchField(s.to_owned())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetMode {
    Monthly,
    Annual,
    Project,
}

impl fmt::Display for BudgetMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Monthly => write!(f, "monthly"),
            Self::Annual => write!(f, "annual"),
            Self::Project => write!(f, "project"),
        }
    }
}

impl std::str::FromStr for BudgetMode {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "monthly" => Ok(Self::Monthly),
            "annual" => Ok(Self::Annual),
            "project" => Ok(Self::Project),
            _ => Err(Error::InvalidBudgetMode(s.to_owned())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CorrelationType {
    Transfer,
    Reimbursement,
}

impl fmt::Display for CorrelationType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transfer => write!(f, "transfer"),
            Self::Reimbursement => write!(f, "reimbursement"),
        }
    }
}

impl std::str::FromStr for CorrelationType {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "transfer" => Ok(Self::Transfer),
            "reimbursement" => Ok(Self::Reimbursement),
            _ => Err(Error::InvalidCorrelationType(s.to_owned())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionStatus {
    Active,
    Expired,
    Revoked,
}

impl fmt::Display for ConnectionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Active => write!(f, "active"),
            Self::Expired => write!(f, "expired"),
            Self::Revoked => write!(f, "revoked"),
        }
    }
}

impl std::str::FromStr for ConnectionStatus {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "active" => Ok(Self::Active),
            "expired" => Ok(Self::Expired),
            "revoked" => Ok(Self::Revoked),
            _ => Err(Error::InvalidConnectionStatus(s.to_owned())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CategoryMethod {
    Manual,
    Rule,
    Llm,
}

impl fmt::Display for CategoryMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Manual => write!(f, "manual"),
            Self::Rule => write!(f, "rule"),
            Self::Llm => write!(f, "llm"),
        }
    }
}

impl std::str::FromStr for CategoryMethod {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "manual" => Ok(Self::Manual),
            "rule" => Ok(Self::Rule),
            "llm" => Ok(Self::Llm),
            _ => Err(Error::InvalidCategoryMethod(s.to_owned())),
        }
    }
}

/// Whether a budgeted category represents a fixed or variable expense.
///
/// Fixed expenses (rent, mortgage) hit the budget predictably in a lump sum.
/// Variable expenses (groceries, dining) are spread over the period.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetType {
    /// Expense that hits the budget predictably (rent, mortgage, subscriptions)
    Fixed,
    /// Expense you want to spread and minimize (groceries, dining)
    Variable,
}

impl fmt::Display for BudgetType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fixed => write!(f, "fixed"),
            Self::Variable => write!(f, "variable"),
        }
    }
}

impl std::str::FromStr for BudgetType {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "fixed" => Ok(Self::Fixed),
            "variable" => Ok(Self::Variable),
            _ => Err(Error::InvalidBudgetType(s.to_owned())),
        }
    }
}

/// Budget status pace indicator
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaceIndicator {
    /// Fixed category: payment hasn't arrived yet
    Pending,
    UnderBudget,
    OnTarget,
    OnTrack,
    OverBudget,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_method_display_roundtrip() {
        for (variant, expected) in [
            (CategoryMethod::Manual, "manual"),
            (CategoryMethod::Rule, "rule"),
            (CategoryMethod::Llm, "llm"),
        ] {
            assert_eq!(variant.to_string(), expected);
            assert_eq!(expected.parse::<CategoryMethod>().unwrap(), variant);
        }
    }

    #[test]
    fn category_method_from_str_invalid() {
        assert!("unknown".parse::<CategoryMethod>().is_err());
    }

    #[test]
    fn category_method_serde_roundtrip() {
        for variant in [
            CategoryMethod::Manual,
            CategoryMethod::Rule,
            CategoryMethod::Llm,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let deserialized: CategoryMethod = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized, variant);
        }
    }

    #[test]
    fn category_method_serde_snake_case() {
        assert_eq!(
            serde_json::to_string(&CategoryMethod::Manual).unwrap(),
            "\"manual\""
        );
        assert_eq!(
            serde_json::to_string(&CategoryMethod::Rule).unwrap(),
            "\"rule\""
        );
        assert_eq!(
            serde_json::to_string(&CategoryMethod::Llm).unwrap(),
            "\"llm\""
        );
    }

    #[test]
    fn budget_type_display_roundtrip() {
        for (variant, expected) in [
            (BudgetType::Fixed, "fixed"),
            (BudgetType::Variable, "variable"),
        ] {
            assert_eq!(variant.to_string(), expected);
            assert_eq!(expected.parse::<BudgetType>().unwrap(), variant);
        }
    }

    #[test]
    fn budget_type_from_str_invalid() {
        assert!("unknown".parse::<BudgetType>().is_err());
    }

    #[test]
    fn budget_type_serde_roundtrip() {
        for variant in [BudgetType::Fixed, BudgetType::Variable] {
            let json = serde_json::to_string(&variant).unwrap();
            let deserialized: BudgetType = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized, variant);
        }
    }

    #[test]
    fn pace_indicator_pending_serde() {
        assert_eq!(
            serde_json::to_string(&PaceIndicator::Pending).unwrap(),
            "\"pending\""
        );
        let parsed: PaceIndicator = serde_json::from_str("\"pending\"").unwrap();
        assert_eq!(parsed, PaceIndicator::Pending);
    }
}
