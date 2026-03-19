//! Core domain library: configuration, models, and business rules.
//!
//! This crate has no knowledge of HTTP, job queues, or external providers.
//! It defines the shared vocabulary (models, IDs, enums) used by all other
//! crates. Database queries live in `budget-db`.

use std::num::NonZeroU32;

use serde::{Deserialize, Serialize};

pub mod anomalies;
pub mod budget;
pub mod error;
pub mod models;
pub mod projection;
pub mod rules;
pub mod seasonality;

use models::{CurrencyCode, DatabaseUrl, Host, SecretKey};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub database_url: DatabaseUrl,
    pub llm_model: String,
    pub gemini_api_key: Option<String>,
    pub budget_currency: CurrencyCode,
    pub expected_salary_count: NonZeroU32,
    pub secret_key: SecretKey,
    pub server_port: u16,
    pub enable_banking_app_id: Option<String>,
    pub enable_banking_private_key_path: Option<String>,
    /// Sandbox credentials for live tests (overrides the main fields in tests).
    pub enable_banking_sandbox_app_id: Option<String>,
    pub enable_banking_sandbox_private_key_path: Option<String>,
    pub host: Option<Host>,
    pub log_path: Option<String>,
    pub frontend_dir: Option<String>,
    pub amazon_cookies_dir: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            database_url: DatabaseUrl::new("postgresql://budget@localhost:5432/budget")
                .expect("valid default database URL"),
            llm_model: "gemini-3.1-flash-lite-preview".to_owned(),
            gemini_api_key: None,
            budget_currency: CurrencyCode::new("USD").expect("valid default currency"),
            expected_salary_count: NonZeroU32::new(1).expect("1 is non-zero"),
            secret_key: SecretKey::empty(),
            server_port: 3000,
            enable_banking_app_id: None,
            enable_banking_private_key_path: None,
            enable_banking_sandbox_app_id: None,
            enable_banking_sandbox_private_key_path: None,
            host: None,
            log_path: None,
            frontend_dir: None,
            amazon_cookies_dir: None,
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
