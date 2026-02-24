use chrono::{NaiveDate, Utc};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};

use crate::error::ProviderError;

use super::types::{
    ApiErrorResponse, AuthorizationRequest, AuthorizationResponse, BalanceResponse,
    SessionCreateRequest, SessionResponse, TransactionResponse,
};
#[cfg(test)]
use super::types::{AspspEntry, AspspsResponse};

/// Configuration for connecting to the Enable Banking API.
#[derive(Debug, Clone)]
pub struct EnableBankingConfig {
    pub app_id: String,
    pub private_key_pem: Vec<u8>,
    pub base_url: String,
}

impl EnableBankingConfig {
    #[must_use]
    pub fn new(app_id: String, private_key_pem: Vec<u8>) -> Self {
        Self {
            app_id,
            private_key_pem,
            base_url: "https://api.enablebanking.com".to_owned(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct JwtClaims {
    iss: String,
    aud: String,
    iat: i64,
    exp: i64,
}

/// Low-level HTTP client for the Enable Banking API.
fn truncate(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        s
    } else {
        &s[..s.floor_char_boundary(max_len)]
    }
}

pub struct Client {
    http: reqwest::Client,
    config: EnableBankingConfig,
}

impl Client {
    #[must_use]
    pub fn new(config: EnableBankingConfig) -> Self {
        Self {
            http: reqwest::Client::new(),
            config,
        }
    }

    fn sign_jwt(&self) -> Result<String, ProviderError> {
        let now = Utc::now().timestamp();
        let claims = JwtClaims {
            iss: "enablebanking.com".to_owned(),
            aud: "api.enablebanking.com".to_owned(),
            iat: now,
            exp: now + 3600,
        };

        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(self.config.app_id.clone());

        let key = EncodingKey::from_rsa_pem(&self.config.private_key_pem).map_err(|e| {
            ProviderError::AuthenticationFailed(format!("invalid private key: {e}"))
        })?;

        jsonwebtoken::encode(&header, &claims, &key)
            .map_err(|e| ProviderError::AuthenticationFailed(format!("JWT signing failed: {e}")))
    }

    async fn handle_error_response(
        &self,
        response: reqwest::Response,
    ) -> Result<reqwest::Response, ProviderError> {
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }

        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(ProviderError::SessionExpired);
        }

        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(ProviderError::RateLimited);
        }

        let raw_body = response.text().await.unwrap_or_default();
        let body =
            serde_json::from_str::<ApiErrorResponse>(&raw_body).unwrap_or(ApiErrorResponse {
                code: None,
                description: None,
            });

        Err(ProviderError::ApiError {
            code: body
                .code_string()
                .unwrap_or_else(|| status.as_u16().to_string()),
            description: body
                .description
                .unwrap_or_else(|| truncate(&raw_body, 200).to_owned()),
        })
    }

    // ── Authorization flow ────────────────────────────────────────

    pub(crate) async fn start_authorization(
        &self,
        request: &AuthorizationRequest,
    ) -> Result<AuthorizationResponse, ProviderError> {
        let token = self.sign_jwt()?;
        let url = format!("{}/auth", self.config.base_url);

        let response = self
            .http
            .post(&url)
            .bearer_auth(&token)
            .json(request)
            .send()
            .await
            .map_err(|e| ProviderError::ConnectionFailed(e.to_string()))?;

        let response = self.handle_error_response(response).await?;
        response
            .json()
            .await
            .map_err(|e| ProviderError::Other(format!("failed to parse auth response: {e}")))
    }

    pub(crate) async fn create_session(
        &self,
        code: &str,
    ) -> Result<SessionResponse, ProviderError> {
        let token = self.sign_jwt()?;
        let url = format!("{}/sessions", self.config.base_url);

        let response = self
            .http
            .post(&url)
            .bearer_auth(&token)
            .json(&SessionCreateRequest {
                code: code.to_owned(),
            })
            .send()
            .await
            .map_err(|e| ProviderError::ConnectionFailed(e.to_string()))?;

        let response = self.handle_error_response(response).await?;
        response
            .json()
            .await
            .map_err(|e| ProviderError::Other(format!("failed to parse session response: {e}")))
    }

    pub(crate) async fn delete_session(&self, session_id: &str) -> Result<(), ProviderError> {
        let token = self.sign_jwt()?;
        let url = format!("{}/sessions/{session_id}", self.config.base_url);

        let response = self
            .http
            .delete(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| ProviderError::ConnectionFailed(e.to_string()))?;

        self.handle_error_response(response).await?;
        Ok(())
    }

    // ── Data fetching ─────────────────────────────────────────────

    #[cfg(test)]
    pub(crate) async fn get_aspsps(
        &self,
        country: Option<&str>,
    ) -> Result<Vec<AspspEntry>, ProviderError> {
        let token = self.sign_jwt()?;
        let url = format!("{}/aspsps", self.config.base_url);

        let mut req = self.http.get(&url).bearer_auth(&token);

        if let Some(c) = country {
            req = req.query(&[("country", c)]);
        }

        let response = req
            .send()
            .await
            .map_err(|e| ProviderError::ConnectionFailed(e.to_string()))?;

        let response = self.handle_error_response(response).await?;
        let wrapper: AspspsResponse = response
            .json()
            .await
            .map_err(|e| ProviderError::Other(format!("failed to parse ASPSPs: {e}")))?;
        Ok(wrapper.aspsps)
    }

    pub(crate) async fn get_balances(
        &self,
        account_id: &str,
    ) -> Result<BalanceResponse, ProviderError> {
        let token = self.sign_jwt()?;
        let url = format!("{}/accounts/{account_id}/balances", self.config.base_url);

        let response = self
            .http
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| ProviderError::ConnectionFailed(e.to_string()))?;

        let response = self.handle_error_response(response).await?;
        response
            .json()
            .await
            .map_err(|e| ProviderError::Other(format!("failed to parse balances: {e}")))
    }

    pub(crate) async fn get_transactions(
        &self,
        account_id: &str,
        date_from: NaiveDate,
        date_to: NaiveDate,
        continuation_key: Option<&str>,
    ) -> Result<TransactionResponse, ProviderError> {
        let token = self.sign_jwt()?;
        let url = format!(
            "{}/accounts/{account_id}/transactions",
            self.config.base_url
        );

        let mut req = self.http.get(&url).bearer_auth(&token).query(&[
            ("date_from", date_from.to_string()),
            ("date_to", date_to.to_string()),
        ]);

        if let Some(key) = continuation_key {
            req = req.query(&[("continuation_key", key)]);
        }

        let response = req
            .send()
            .await
            .map_err(|e| ProviderError::ConnectionFailed(e.to_string()))?;

        let response = self.handle_error_response(response).await?;
        response
            .json()
            .await
            .map_err(|e| ProviderError::Other(format!("failed to parse transactions: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_RSA_PEM: &[u8] = include_bytes!("test_fixtures/test_key.pem");

    #[test]
    fn sign_jwt_produces_valid_token() {
        let config = EnableBankingConfig {
            app_id: "test-app-123".to_owned(),
            private_key_pem: TEST_RSA_PEM.to_vec(),
            base_url: "https://api.enablebanking.com".to_owned(),
        };
        let client = Client::new(config);
        let token = client.sign_jwt().unwrap();

        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3);

        let header_bytes = jsonwebtoken::decode_header(&token).unwrap();
        assert_eq!(header_bytes.alg, Algorithm::RS256);
        assert_eq!(header_bytes.kid.as_deref(), Some("test-app-123"));
    }

    #[test]
    fn sign_jwt_rejects_invalid_pem() {
        let config = EnableBankingConfig {
            app_id: "test-app".to_owned(),
            private_key_pem: b"not a valid pem".to_vec(),
            base_url: "https://api.enablebanking.com".to_owned(),
        };
        let client = Client::new(config);
        let result = client.sign_jwt();
        assert!(result.is_err());
    }
}
