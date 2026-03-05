//! Core domain library: configuration, database access, models, and business rules.
//!
//! This crate has no knowledge of HTTP, job queues, or external providers.
//! It defines the shared vocabulary (models, IDs, enums) and persistence layer
//! used by all other crates.

use serde::{Deserialize, Serialize};

pub mod budget;
pub mod db;
pub mod error;
pub mod models;
pub mod rules;

use models::CurrencyCode;

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub database_url: String,
    pub llm_model: String,
    pub gemini_api_key: Option<String>,
    pub bank_provider: String,
    pub budget_currency: CurrencyCode,
    pub expected_salary_count: u32,
    pub secret_key: String,
    pub server_port: u16,
    pub enable_banking_app_id: Option<String>,
    pub enable_banking_private_key_path: Option<String>,
    /// Sandbox credentials for live tests (overrides the main fields in tests).
    pub enable_banking_sandbox_app_id: Option<String>,
    pub enable_banking_sandbox_private_key_path: Option<String>,
    pub host: Option<String>,
    pub log_path: Option<String>,
    pub frontend_dir: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            database_url: "postgresql://budget@localhost:5432/budget".to_owned(),
            llm_model: "gemini-2.5-flash-lite".to_owned(),
            gemini_api_key: None,
            bank_provider: "mock".to_owned(),
            budget_currency: CurrencyCode::new("USD").expect("valid default currency"),
            expected_salary_count: 1,
            secret_key: String::new(),
            server_port: 3000,
            enable_banking_app_id: None,
            enable_banking_private_key_path: None,
            enable_banking_sandbox_app_id: None,
            enable_banking_sandbox_private_key_path: None,
            host: None,
            log_path: None,
            frontend_dir: None,
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

/// Return the path confy resolves for the configuration file.
///
/// # Errors
///
/// Returns an error if the config directory cannot be determined.
pub fn config_path() -> Result<std::path::PathBuf, confy::ConfyError> {
    confy::get_configuration_file_path("budget", None)
}
