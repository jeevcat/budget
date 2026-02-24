use serde::{Deserialize, Serialize};

pub mod budget;
pub mod db;
pub mod error;
pub mod models;
pub mod rules;

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub database_url: String,
    pub llm_model: String,
    pub bank_provider: String,
    pub budget_currency: String,
    pub expected_salary_count: u32,
    pub server_port: u16,
    pub enable_banking_app_id: Option<String>,
    pub enable_banking_private_key_path: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            database_url: "sqlite:budget.db?mode=rwc".to_owned(),
            llm_model: "gemini-2.5-flash-lite".to_owned(),
            bank_provider: "mock".to_owned(),
            budget_currency: "USD".to_owned(),
            expected_salary_count: 1,
            server_port: 3000,
            enable_banking_app_id: None,
            enable_banking_private_key_path: None,
        }
    }
}

/// Load configuration from the default confy location.
///
/// # Errors
///
/// Returns an error if the config file cannot be read or parsed.
pub fn load_config() -> Result<Config, confy::ConfyError> {
    confy::load("budget", None)
}
