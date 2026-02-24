use super::client::Client;
use super::types::{AccessRequest, AspspRequest, AuthorizationRequest, SessionResponse};
use crate::error::ProviderError;

/// Handles the Enable Banking OAuth-like authorization flow.
///
/// This is separate from `BankProvider` — it manages the redirect dance
/// (start auth → user redirects → exchange code) before a provider session exists.
pub struct EnableBankingAuth {
    client: Client,
}

impl EnableBankingAuth {
    #[must_use]
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    /// Start the authorization flow. Returns a URL to redirect the user to.
    ///
    /// # Errors
    ///
    /// Returns `ProviderError` if the API request fails.
    pub async fn start_authorization(
        &self,
        aspsp_name: &str,
        aspsp_country: &str,
        redirect_url: &str,
        state: &str,
        valid_until: &str,
    ) -> Result<String, ProviderError> {
        let request = AuthorizationRequest {
            access: AccessRequest {
                valid_until: valid_until.to_owned(),
            },
            aspsp: AspspRequest {
                name: aspsp_name.to_owned(),
                country: aspsp_country.to_owned(),
            },
            state: state.to_owned(),
            redirect_url: redirect_url.to_owned(),
            psu_type: None,
        };

        let response = self.client.start_authorization(&request).await?;
        Ok(response.url)
    }

    /// Exchange the authorization code for a session with account list.
    ///
    /// # Errors
    ///
    /// Returns `ProviderError` if the API request fails.
    pub async fn exchange_code(&self, code: &str) -> Result<SessionResponse, ProviderError> {
        self.client.create_session(code).await
    }

    /// Revoke an existing session.
    ///
    /// # Errors
    ///
    /// Returns `ProviderError` if the API request fails.
    pub async fn revoke_session(&self, session_id: &str) -> Result<(), ProviderError> {
        self.client.delete_session(session_id).await
    }
}
