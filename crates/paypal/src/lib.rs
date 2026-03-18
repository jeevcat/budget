pub mod client;
pub mod error;
mod live_tests;
pub mod matching;
pub mod types;

pub use client::PayPalClient;
pub use error::PayPalError;
pub use matching::find_matches;
pub use types::{BankCandidate, MatchResult, PayPalItem, PayPalTransaction};
