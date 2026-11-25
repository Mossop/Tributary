//! Lists GitHub issues and pull requests matching a search query.

use anyhow::Context as _;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::header::{ACCEPT, AUTHORIZATION, HeaderMap, HeaderValue, USER_AGENT};
use serde::Deserialize;

use super::{Field, FieldKind, Source, SourceError, SourceItem, SourceType};

const ICON: &str = "https://github.com/favicon.ico";
const DEFAULT_API_URL: &str = "https://api.github.com";

#[derive(Debug, Clone, Deserialize)]
pub struct GithubConfig {
    /// A GitHub issue search query, e.g. `is:open is:pr author:@me`.
    pub query: String,
    pub token: String,
    /// API base URL; empty or absent means public GitHub.
    #[serde(default)]
    pub api_url: Option<String>,
}

pub fn source_type() -> SourceType {
    SourceType::new(
        "github",
        "GitHub search",
        vec![
            Field {
                key: "query",
                label: "Search query",
                kind: FieldKind::Text,
                required: true,
                placeholder: Some("is:open is:pr author:@me"),
                help: Some("A GitHub issue and pull request search."),
            },
            Field {
                key: "token",
                label: "Access token",
                kind: FieldKind::Secret,
                required: true,
                placeholder: None,
                help: Some("A personal access token with read access."),
            },
            Field {
                key: "api_url",
                label: "API URL",
                kind: FieldKind::Url,
                required: false,
                placeholder: Some(DEFAULT_API_URL),
                help: Some("Only needed for GitHub Enterprise."),
            },
        ],
        |config| {
            let config: GithubConfig = serde_json::from_value(config.clone())?;
            Ok(std::sync::Arc::new(GithubSource::from_config(&config)?))
        },
    )
}

pub struct GithubSource {
    client: reqwest::Client,
    api_url: String,
    query: String,
}

impl GithubSource {
    pub fn from_config(config: &GithubConfig) -> anyhow::Result<Self> {
        let api_url = config
            .api_url
            .as_deref()
            .map(str::trim)
            .filter(|url| !url.is_empty())
            .unwrap_or(DEFAULT_API_URL);
        Self::new(api_url, &config.query, &config.token)
    }

    pub fn new(api_url: &str, query: &str, token: &str) -> anyhow::Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/vnd.github+json"),
        );
        headers.insert(USER_AGENT, HeaderValue::from_static("tributary"));
        headers.insert(
            "X-GitHub-Api-Version",
            HeaderValue::from_static("2022-11-28"),
        );
        let mut auth =
            HeaderValue::from_str(&format!("Bearer {token}")).context("invalid GitHub token")?;
        auth.set_sensitive(true);
        headers.insert(AUTHORIZATION, auth);

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()?;

        Ok(Self {
            client,
            api_url: api_url.trim_end_matches('/').to_string(),
            query: query.to_string(),
        })
    }
}

#[derive(Deserialize)]
struct SearchResponse {
    items: Vec<Issue>,
}

#[derive(Deserialize)]
struct Issue {
    number: u64,
    title: String,
    html_url: String,
    repository_url: String,
    updated_at: DateTime<Utc>,
    user: Option<User>,
    pull_request: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct User {
    login: String,
}

#[derive(Deserialize)]
struct ErrorResponse {
    message: String,
}

impl Issue {
    fn into_item(self) -> SourceItem {
        let repo = self
            .repository_url
            .split_once("/repos/")
            .map(|(_, repo)| repo.to_string())
            .unwrap_or(self.repository_url);
        let kind = if self.pull_request.is_some() {
            "PR"
        } else {
            "Issue"
        };
        let description = match self.user {
            Some(user) => format!("{repo} #{} · {kind} by {}", self.number, user.login),
            None => format!("{repo} #{} · {kind}", self.number),
        };

        SourceItem {
            id: format!("{repo}#{}", self.number),
            title: self.title,
            url: self.html_url,
            icon: None,
            modified: self.updated_at,
            description: Some(description),
            due: None,
        }
    }
}

#[async_trait]
impl Source for GithubSource {
    fn icon(&self) -> String {
        ICON.to_string()
    }

    async fn fetch(&self) -> Result<Vec<SourceItem>, SourceError> {
        let response = self
            .client
            .get(format!("{}/search/issues", self.api_url))
            .query(&[
                ("q", self.query.as_str()),
                ("per_page", "100"),
                ("sort", "updated"),
                ("order", "desc"),
            ])
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let message = response
                .json::<ErrorResponse>()
                .await
                .map(|e| e.message)
                .unwrap_or_default();
            return Err(SourceError::Message(format!(
                "GitHub returned {status}: {message}"
            )));
        }

        let body: SearchResponse = response.json().await?;
        Ok(body.items.into_iter().map(Issue::into_item).collect())
    }
}
