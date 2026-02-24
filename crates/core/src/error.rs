use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("no salary category provided")]
    NoSalaryCategory,

    #[error("budget period not found for category {0}")]
    BudgetPeriodNotFound(String),

    #[error("invalid period type: {0}")]
    InvalidPeriodType(String),

    #[error("invalid rule type: {0}")]
    InvalidRuleType(String),

    #[error("invalid match field: {0}")]
    InvalidMatchField(String),

    #[error("invalid correlation type: {0}")]
    InvalidCorrelationType(String),

    #[error("invalid account type: {0}")]
    InvalidAccountType(String),

    #[error("invalid rule pattern: {0}")]
    InvalidRulePattern(String),
}
