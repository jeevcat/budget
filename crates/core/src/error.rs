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

    #[error("invalid category name: {0}")]
    InvalidCategoryName(String),

    #[error("invalid currency code: {0}")]
    InvalidCurrencyCode(String),

    #[error("invalid IBAN: {0}")]
    InvalidIban(String),

    #[error("invalid BIC: {0}")]
    InvalidBic(String),

    #[error("invalid merchant category code: {0}")]
    InvalidMerchantCategoryCode(String),

    #[error("invalid exchange rate type: {0}")]
    InvalidExchangeRateType(String),

    #[error("invalid priority: {0} (must be 0–1000)")]
    InvalidPriority(i32),

    #[error("invalid valid_days: {0} (must be 1–365)")]
    InvalidValidDays(u32),
}
