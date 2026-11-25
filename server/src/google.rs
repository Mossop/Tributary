//! Google OAuth: the consent redirect, exchanging codes and refreshing
//! access tokens.

use std::time::Duration;

use reqwest::Url;
use serde::Deserialize;

use crate::config::GoogleConfig;

/// Always requested so the account can be identified and named.
pub const IDENTITY_SCOPES: &[&str] = &["openid", "email"];

#[derive(Debug, thiserror::Error)]
pub enum GoogleError {
    /// The refresh token was revoked or expired; the account must be
    /// connected again.
    #[error("access was revoked")]
    Revoked,
    #[error("Google returned {0}")]
    Api(String),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
}

#[derive(Debug, Deserialize)]
pub struct Tokens {
    pub access_token: String,
    pub expires_in: u64,
    #[serde(default)]
    pub refresh_token: Option<String>,
    /// The granted scopes, space separated.
    #[serde(default)]
    pub scope: Option<String>,
}

impl Tokens {
    pub fn expires_in(&self) -> Duration {
        Duration::from_secs(self.expires_in)
    }
}

#[derive(Debug, Deserialize)]
pub struct UserInfo {
    pub sub: String,
    pub email: String,
}

#[derive(Deserialize)]
struct ErrorResponse {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

pub struct GoogleClient {
    pub config: GoogleConfig,
    http: reqwest::Client,
    redirect_uri: String,
}

impl GoogleClient {
    pub fn new(config: GoogleConfig, public_url: &str) -> Self {
        Self {
            redirect_uri: format!(
                "{}/api/oauth/google/callback",
                public_url.trim_end_matches('/')
            ),
            config,
            http: reqwest::Client::new(),
        }
    }

    /// An HTTP client for calling Google APIs.
    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    /// The consent page to send the user to.
    pub fn authorize_url(&self, state: &str, scopes: &[String], login_hint: Option<&str>) -> Url {
        let mut url = Url::parse(&self.config.auth_url).expect("invalid Google auth URL");
        {
            let mut query = url.query_pairs_mut();
            query
                .append_pair("client_id", &self.config.client_id)
                .append_pair("redirect_uri", &self.redirect_uri)
                .append_pair("response_type", "code")
                .append_pair("scope", &scopes.join(" "))
                .append_pair("access_type", "offline")
                .append_pair("prompt", "consent")
                .append_pair("include_granted_scopes", "true")
                .append_pair("state", state);
            if let Some(hint) = login_hint {
                query.append_pair("login_hint", hint);
            }
        }
        url
    }

    async fn token_request(&self, params: &[(&str, &str)]) -> Result<Tokens, GoogleError> {
        let response = self
            .http
            .post(&self.config.token_url)
            .form(params)
            .send()
            .await?;
        if response.status().is_success() {
            return Ok(response.json().await?);
        }
        let status = response.status();
        match response.json::<ErrorResponse>().await {
            Ok(error) if error.error == "invalid_grant" => Err(GoogleError::Revoked),
            Ok(error) => Err(GoogleError::Api(format!(
                "{status}: {}",
                error.error_description.unwrap_or(error.error)
            ))),
            Err(_) => Err(GoogleError::Api(status.to_string())),
        }
    }

    /// Exchanges the code from the consent redirect for tokens.
    pub async fn exchange_code(&self, code: &str) -> Result<Tokens, GoogleError> {
        self.token_request(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("client_id", &self.config.client_id),
            ("client_secret", &self.config.client_secret),
            ("redirect_uri", &self.redirect_uri),
        ])
        .await
    }

    /// Gets a new access token.
    pub async fn refresh(&self, refresh_token: &str) -> Result<Tokens, GoogleError> {
        self.token_request(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", &self.config.client_id),
            ("client_secret", &self.config.client_secret),
        ])
        .await
    }

    pub async fn user_info(&self, access_token: &str) -> Result<UserInfo, GoogleError> {
        let response = self
            .http
            .get(&self.config.userinfo_url)
            .bearer_auth(access_token)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(GoogleError::Api(response.status().to_string()));
        }
        Ok(response.json().await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client() -> GoogleClient {
        let config: GoogleConfig = serde_json::from_value(serde_json::json!({
            "client_id": "id",
            "client_secret": "secret",
        }))
        .unwrap();
        GoogleClient::new(config, "http://localhost:8080/")
    }

    #[test]
    fn builds_the_consent_url() {
        let scopes = vec!["openid".to_string(), "email".to_string()];
        let url = client().authorize_url("abc", &scopes, Some("me@example.com"));
        assert_eq!(url.host_str(), Some("accounts.google.com"));
        let query: Vec<(String, String)> = url.query_pairs().into_owned().collect();
        let get = |key: &str| {
            query
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(get("client_id"), Some("id"));
        assert_eq!(
            get("redirect_uri"),
            Some("http://localhost:8080/api/oauth/google/callback")
        );
        assert_eq!(get("scope"), Some("openid email"));
        assert_eq!(get("access_type"), Some("offline"));
        assert_eq!(get("state"), Some("abc"));
        assert_eq!(get("login_hint"), Some("me@example.com"));
    }
}
