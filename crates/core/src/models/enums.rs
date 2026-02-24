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
}

impl fmt::Display for MatchField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Merchant => write!(f, "merchant"),
            Self::Description => write!(f, "description"),
            Self::AmountRange => write!(f, "amount_range"),
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
            _ => Err(Error::InvalidMatchField(s.to_owned())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PeriodType {
    Monthly,
    Annual,
}

impl fmt::Display for PeriodType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Monthly => write!(f, "monthly"),
            Self::Annual => write!(f, "annual"),
        }
    }
}

impl std::str::FromStr for PeriodType {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "monthly" => Ok(Self::Monthly),
            "annual" => Ok(Self::Annual),
            _ => Err(Error::InvalidPeriodType(s.to_owned())),
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

/// Budget status pace indicator
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaceIndicator {
    UnderBudget,
    OnTrack,
    OverBudget,
}
