//! External service integrations: bank data providers and LLM providers.
//!
//! Defines the `BankProvider` and `LlmProvider` traits with concrete implementations
//! (Enable Banking, Gemini) and test mocks. No knowledge of job queues or HTTP — the
//! `budget-jobs` crate wraps these in type-erased clients for worker injection.

pub mod bank;
pub mod enable_banking;
pub mod error;
pub mod gemini;
pub mod llm;
pub mod mock;

pub use bank::{Account, AccountBalance, AccountId, BankProvider, Transaction};
pub use enable_banking::{
    AspspEntry, Client as EnableBankingClient, EnableBankingAuth, EnableBankingConfig,
    EnableBankingProvider, SessionAccount, SessionResponse,
};
pub use error::ProviderError;
pub use gemini::GeminiProvider;
pub use llm::{
    CategorizeInput, CategorizeResult, CorrelationResult, CorrelationType, LlmProvider,
    ProposedRule, RuleContext, TransactionSummary,
};
pub use mock::{MockBankProvider, MockLlmProvider};
