//! Lists GitHub issues and pull requests matching a search query.

use std::sync::Arc;

use anyhow::{Context as _, bail};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::StatusCode;
use reqwest::header::{ACCEPT, HeaderMap, HeaderValue, USER_AGENT};
use serde::Deserialize;

use super::{
    Field, FieldKind, ItemKind, Source, SourceContext, SourceError, SourceItem, SourceType,
};
use crate::accounts::{Accounts, GITHUB};

const ICON: &str = "https://github.com/favicon.ico";

// Item icons are GitHub's Octicons (MIT licensed, see `octicons/LICENSE`) in
// the colours GitHub uses for each state.
const OPEN: &str = "#1a7f37";
const DONE: &str = "#8250df";
const CLOSED: &str = "#d1242f";
const NEUTRAL: &str = "#59636e";

const PR_OPEN: &str = include_str!("octicons/git-pull-request.svg");
const PR_DRAFT: &str = include_str!("octicons/git-pull-request-draft.svg");
const PR_CLOSED: &str = include_str!("octicons/git-pull-request-closed.svg");
const PR_MERGED: &str = include_str!("octicons/git-merge.svg");
const ISSUE_OPEN: &str = include_str!("octicons/issue-opened.svg");
const ISSUE_CLOSED: &str = include_str!("octicons/issue-closed.svg");
const ISSUE_NOT_PLANNED: &str = include_str!("octicons/skip.svg");
pub const DEFAULT_API_URL: &str = "https://api.github.com";

#[derive(Debug, Clone, Deserialize)]
pub struct GithubConfig {
    /// A GitHub issue search query, e.g. `is:open is:pr author:@me`.
    pub query: String,
    /// The GitHub account to search as.
    pub account: String,
}

pub fn source_type() -> SourceType {
    SourceType::new(
        "github",
        "GitHub search",
        vec![
            Field {
                key: "account",
                label: "GitHub account",
                kind: FieldKind::Account,
                required: true,
                placeholder: None,
                help: None,
                provider: Some(GITHUB),
            },
            Field {
                key: "query",
                label: "Search query",
                kind: FieldKind::Text,
                required: true,
                placeholder: Some("is:open is:pr author:@me"),
                help: Some("A GitHub issue and pull request search."),
                provider: None,
            },
        ],
        |config, context| {
            let config: GithubConfig = serde_json::from_value(config.clone())?;
            Ok(Arc::new(GithubSource::new(context, config)?))
        },
    )
}

/// The API URL to use, given an optional configured one.
pub fn api_url(configured: Option<&str>) -> String {
    configured
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .unwrap_or(DEFAULT_API_URL)
        .trim_end_matches('/')
        .to_string()
}

/// An HTTP client with the headers GitHub's API expects. Requests add the
/// token themselves.
pub fn client() -> reqwest::Result<reqwest::Client> {
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
    reqwest::Client::builder().default_headers(headers).build()
}

/// The user a token belongs to.
#[derive(Debug, Deserialize)]
pub struct GithubUser {
    pub id: u64,
    pub login: String,
}

#[derive(Debug, thiserror::Error)]
pub enum TokenError {
    /// GitHub didn't accept the token.
    #[error("GitHub didn't accept the token")]
    Rejected,
    #[error("GitHub returned {0}")]
    Api(String),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
}

/// Looks up the user a token belongs to, checking the token works.
pub async fn current_user(
    client: &reqwest::Client,
    api_url: &str,
    token: &str,
) -> Result<GithubUser, TokenError> {
    let response = client
        .get(format!("{api_url}/user"))
        .bearer_auth(token)
        .send()
        .await?;
    match response.status() {
        status if status.is_success() => Ok(response.json().await?),
        StatusCode::UNAUTHORIZED => Err(TokenError::Rejected),
        status => Err(TokenError::Api(error_message(status, response).await)),
    }
}

async fn error_message(status: StatusCode, response: reqwest::Response) -> String {
    let message = response
        .json::<ErrorResponse>()
        .await
        .map(|e| e.message)
        .unwrap_or_default();
    format!("{status}: {message}")
}

pub struct GithubSource {
    client: reqwest::Client,
    accounts: Arc<Accounts>,
    account: String,
    query: String,
}

impl GithubSource {
    pub fn new(context: &SourceContext, config: GithubConfig) -> anyhow::Result<Self> {
        Ok(Self {
            client: client()?,
            accounts: context.accounts.clone(),
            account: config.account,
            query: config.query,
        })
    }

    async fn search(&self) -> anyhow::Result<Vec<SourceItem>> {
        let (account, token) = self.accounts.github(&self.account).await?;
        let api_url = api_url(account.api_url.as_deref());
        let response = self
            .client
            .get(format!("{api_url}/search/issues"))
            .bearer_auth(&token)
            .query(&[
                ("q", self.query.as_str()),
                ("per_page", "100"),
                ("sort", "updated"),
                ("order", "desc"),
            ])
            .send()
            .await?;

        let status = response.status();
        if status == StatusCode::UNAUTHORIZED {
            self.accounts.mark_revoked(&self.account);
            bail!(
                "GitHub account {} needs reconnecting in settings",
                account.name
            );
        }
        if !status.is_success() {
            bail!("GitHub returned {}", error_message(status, response).await);
        }
        let body: SearchResponse = response
            .json()
            .await
            .context("unexpected response from GitHub")?;
        Ok(body.items.into_iter().map(Issue::into_item).collect())
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
    state: String,
    state_reason: Option<String>,
    #[serde(default)]
    draft: bool,
    pull_request: Option<PullRequest>,
}

#[derive(Deserialize)]
struct PullRequest {
    merged_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
struct User {
    login: String,
}

#[derive(Deserialize)]
struct ErrorResponse {
    message: String,
}

/// Makes a `data:` URL for an Octicon SVG drawn in `color`.
fn octicon(svg: &str, color: &str) -> String {
    let svg = svg
        .trim()
        .replacen("<svg ", &format!("<svg fill=\"{color}\" "), 1);
    let mut url = String::from("data:image/svg+xml,");
    for c in svg.chars() {
        match c {
            '%' => url.push_str("%25"),
            '#' => url.push_str("%23"),
            '<' => url.push_str("%3C"),
            '>' => url.push_str("%3E"),
            '"' => url.push('\''),
            c => url.push(c),
        }
    }
    url
}

impl Issue {
    fn icon(&self) -> String {
        let closed = self.state == "closed";
        match &self.pull_request {
            Some(pr) if pr.merged_at.is_some() => octicon(PR_MERGED, DONE),
            Some(_) if closed => octicon(PR_CLOSED, CLOSED),
            Some(_) if self.draft => octicon(PR_DRAFT, NEUTRAL),
            Some(_) => octicon(PR_OPEN, OPEN),
            None if !closed => octicon(ISSUE_OPEN, OPEN),
            None => match self.state_reason.as_deref() {
                Some("not_planned" | "duplicate") => octicon(ISSUE_NOT_PLANNED, NEUTRAL),
                _ => octicon(ISSUE_CLOSED, DONE),
            },
        }
    }

    fn into_item(self) -> SourceItem {
        let icon = self.icon();
        let repo = self
            .repository_url
            .split_once("/repos/")
            .map(|(_, repo)| repo.to_string())
            .unwrap_or(self.repository_url);
        let (kind, item_kind) = if self.pull_request.is_some() {
            ("PR", ItemKind::Patch)
        } else {
            ("Issue", ItemKind::Issue)
        };
        let description = match self.user {
            Some(user) => format!("{repo} #{} · {kind} by {}", self.number, user.login),
            None => format!("{repo} #{} · {kind}", self.number),
        };

        SourceItem {
            id: format!("{repo}#{}", self.number),
            kind: item_kind,
            title: self.title,
            url: self.html_url,
            icon: Some(icon),
            modified: self.updated_at,
            description: Some(description),
            due: None,
            actions: Vec::new(),
        }
    }
}

#[async_trait]
impl Source for GithubSource {
    fn icon(&self) -> String {
        ICON.to_string()
    }

    async fn fetch(&self) -> Result<Vec<SourceItem>, SourceError> {
        self.search()
            .await
            .map_err(|error| SourceError::Message(format!("{error:#}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue(state: &str, state_reason: Option<&str>) -> Issue {
        Issue {
            number: 1,
            title: "Title".into(),
            html_url: "https://github.com/owner/repo/issues/1".into(),
            repository_url: "https://api.github.com/repos/owner/repo".into(),
            updated_at: DateTime::UNIX_EPOCH,
            user: None,
            state: state.into(),
            state_reason: state_reason.map(Into::into),
            draft: false,
            pull_request: None,
        }
    }

    fn pr(state: &str, draft: bool, merged: bool) -> Issue {
        Issue {
            draft,
            pull_request: Some(PullRequest {
                merged_at: merged.then_some(DateTime::UNIX_EPOCH),
            }),
            ..issue(state, None)
        }
    }

    #[test]
    fn defaults_the_api_url() {
        assert_eq!(api_url(None), DEFAULT_API_URL);
        assert_eq!(api_url(Some(" ")), DEFAULT_API_URL);
        assert_eq!(
            api_url(Some("https://ghe.example.com/api/v3/")),
            "https://ghe.example.com/api/v3"
        );
    }

    #[test]
    fn issue_icons_reflect_state() {
        assert_eq!(issue("open", None).icon(), octicon(ISSUE_OPEN, OPEN));
        assert_eq!(
            issue("open", Some("reopened")).icon(),
            octicon(ISSUE_OPEN, OPEN)
        );
        assert_eq!(
            issue("closed", Some("completed")).icon(),
            octicon(ISSUE_CLOSED, DONE)
        );
        assert_eq!(issue("closed", None).icon(), octicon(ISSUE_CLOSED, DONE));
        assert_eq!(
            issue("closed", Some("not_planned")).icon(),
            octicon(ISSUE_NOT_PLANNED, NEUTRAL)
        );
        assert_eq!(
            issue("closed", Some("duplicate")).icon(),
            octicon(ISSUE_NOT_PLANNED, NEUTRAL)
        );
    }

    #[test]
    fn pull_request_icons_reflect_state() {
        assert_eq!(pr("open", false, false).icon(), octicon(PR_OPEN, OPEN));
        assert_eq!(pr("open", true, false).icon(), octicon(PR_DRAFT, NEUTRAL));
        assert_eq!(
            pr("closed", false, false).icon(),
            octicon(PR_CLOSED, CLOSED)
        );
        assert_eq!(pr("closed", true, false).icon(), octicon(PR_CLOSED, CLOSED));
        assert_eq!(pr("closed", false, true).icon(), octicon(PR_MERGED, DONE));
    }

    #[test]
    fn octicons_are_data_urls() {
        let url = octicon(r#"<svg a="b"><path d="M1 1Z"/></svg>"#, "#123456");
        assert_eq!(
            url,
            "data:image/svg+xml,%3Csvg fill='%23123456' a='b'%3E%3Cpath d='M1 1Z'/%3E%3C/svg%3E"
        );
    }
}
