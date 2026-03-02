use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("no salary category provided")]
    NoSalaryCategory,

    #[error("invalid budget mode: {0}")]
    InvalidBudgetMode(String),

    #[error("invalid budget type: {0}")]
    InvalidBudgetType(String),

    #[error("invalid rule type: {0}")]
    InvalidRuleType(String),

    #[error("invalid match field: {0}")]
    InvalidMatchField(String),

    #[error("invalid correlation type: {0}")]
    InvalidCorrelationType(String),

    #[error("invalid account type: {0}")]
    InvalidAccountType(String),

    #[error("invalid connection status: {0}")]
    InvalidConnectionStatus(String),

    #[error("invalid category method: {0}")]
    InvalidCategoryMethod(String),

    #[error("invalid rule pattern: {0}")]
    InvalidRulePattern(String),
}
