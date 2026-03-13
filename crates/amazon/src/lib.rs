pub mod client;
pub mod cookies;
pub mod error;
pub mod matching;
pub mod parser;
pub mod types;

pub use client::AmazonClient;
pub use cookies::CookieStore;
pub use error::AmazonError;
pub use matching::find_matches;
pub use types::{
    AmazonCookie, AmazonItem, AmazonOrder, AmazonTransaction, AmazonTransactionStatus,
    BankCandidate, MatchConfidence, TransactionsPageData,
};
