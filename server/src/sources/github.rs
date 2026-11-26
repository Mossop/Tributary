//! Lists GitHub issues and pull requests matching a search query.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context as _, bail};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures::{StreamExt as _, TryStreamExt as _, stream};
use reqwest::StatusCode;
use reqwest::header::{ACCEPT, HeaderMap, HeaderValue, USER_AGENT};
use serde::Deserialize;

use super::{
    Field, FieldKind, ITEM_REQUESTS, ItemKind, Source, SourceContext, SourceError, SourceItem,
    SourceType, references,
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
    .with_icon(ICON)
    .with_requested("Review requested (pull requests)")
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
    /// Work out when reviews of pull requests were requested, which costs a
    /// call per pull request.
    due_from: bool,
}

impl GithubSource {
    pub fn new(context: &SourceContext, config: GithubConfig) -> anyhow::Result<Self> {
        Ok(Self {
            client: client()?,
            accounts: context.accounts.clone(),
            account: config.account,
            query: config.query,
            due_from: context.wants_due_from,
        })
    }

    async fn search(&self) -> anyhow::Result<Vec<SourceItem>> {
        let (account, token) = self.accounts.token(&self.account, GITHUB, "GitHub").await?;
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

        let (api_url, token, account) = (&api_url, &token, &account);
        stream::iter(body.items)
            .map(|issue| async move {
                let timeline = match (&issue.pull_request, self.due_from) {
                    (Some(_), true) => Some(
                        self.timeline(api_url, token, &issue.repository_url, issue.number)
                            .await?,
                    ),
                    _ => None,
                };
                let mut item = issue.into_item();
                if let Some(events) = timeline {
                    item.due_from = review_requested_at(&events, &account.name);
                }
                anyhow::Ok(item)
            })
            .buffered(ITEM_REQUESTS)
            .try_collect()
            .await
    }

    /// The events on an issue or pull request, oldest first. `repository_url`
    /// is the API URL of its repository.
    async fn timeline(
        &self,
        api_url: &str,
        token: &str,
        repository_url: &str,
        number: u64,
    ) -> anyhow::Result<Vec<TimelineEvent>> {
        let repo = repository_url
            .split_once("/repos/")
            .map(|(_, repo)| repo)
            .unwrap_or(repository_url);
        let mut events = Vec::new();
        for page in 1..=TIMELINE_PAGES {
            let response = self
                .client
                .get(format!("{api_url}/repos/{repo}/issues/{number}/timeline"))
                .bearer_auth(token)
                .query(&[("per_page", "100".to_string()), ("page", page.to_string())])
                .send()
                .await?;
            let status = response.status();
            if !status.is_success() {
                bail!("GitHub returned {}", error_message(status, response).await);
            }
            let page: Vec<TimelineEvent> = response
                .json()
                .await
                .context("unexpected timeline from GitHub")?;
            let done = page.len() < 100;
            events.extend(page);
            if done {
                break;
            }
        }
        Ok(events)
    }
}

/// Stops runaway paging of very long timelines.
const TIMELINE_PAGES: usize = 5;

#[derive(Debug, Clone, Deserialize)]
struct TimelineEvent {
    #[serde(default)]
    event: String,
    #[serde(default)]
    created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    requested_reviewer: Option<User>,
    #[serde(default)]
    requested_team: Option<Team>,
}

#[derive(Debug, Clone, Deserialize)]
struct Team {
    slug: String,
}

/// When review of a pull request was last asked of `login`, from its
/// timeline: the latest request of them that wasn't removed since, else the
/// latest outstanding request of a team.
fn review_requested_at(events: &[TimelineEvent], login: &str) -> Option<DateTime<Utc>> {
    let mut mine = None;
    let mut teams: HashMap<&str, DateTime<Utc>> = HashMap::new();
    for event in events {
        let requested = match event.event.as_str() {
            "review_requested" => true,
            "review_request_removed" => false,
            _ => continue,
        };
        if let Some(reviewer) = &event.requested_reviewer {
            if reviewer.login.eq_ignore_ascii_case(login) {
                mine = if requested { event.created_at } else { None };
            }
        } else if let Some(team) = &event.requested_team {
            match (requested, event.created_at) {
                (true, Some(at)) => {
                    teams.insert(team.slug.as_str(), at);
                }
                _ => {
                    teams.remove(team.slug.as_str());
                }
            }
        }
    }
    mine.or_else(|| teams.into_values().max())
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
    #[serde(default)]
    created_at: Option<DateTime<Utc>>,
    user: Option<User>,
    state: String,
    state_reason: Option<String>,
    #[serde(default)]
    draft: bool,
    pull_request: Option<PullRequest>,
    #[serde(default)]
    body: Option<String>,
}

#[derive(Deserialize)]
struct PullRequest {
    merged_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Deserialize)]
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
        let mut refs = vec![references::github(&repo, self.number)];
        if self.pull_request.is_some() {
            let body = self.body.as_deref().unwrap_or_default();
            for reference in references::github_closing(body, &repo) {
                references::push(&mut refs, reference);
            }
        }

        SourceItem {
            id: format!("{repo}#{}", self.number),
            kind: item_kind,
            title: self.title,
            url: self.html_url,
            icon: Some(icon),
            modified: self.updated_at,
            created: self.created_at,
            description: Some(description),
            due: None,
            actions: Vec::new(),
            due_from: None,
            references: refs,
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
            created_at: None,
            user: None,
            state: state.into(),
            state_reason: state_reason.map(Into::into),
            draft: false,
            pull_request: None,
            body: None,
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
    fn references_the_issue_and_what_a_pr_closes() {
        assert_eq!(
            issue("open", None).into_item().references,
            ["github:owner/repo:1"]
        );
        let closing = Issue {
            body: Some("Fixes #3 and closes other/thing#4. See #5.".into()),
            ..pr("open", false, false)
        };
        assert_eq!(
            closing.into_item().references,
            [
                "github:owner/repo:1",
                "github:owner/repo:3",
                "github:other/thing:4"
            ]
        );
        // Issues' bodies don't close anything.
        let mentioning = Issue {
            body: Some("Fixes #3".into()),
            ..issue("open", None)
        };
        assert_eq!(mentioning.into_item().references, ["github:owner/repo:1"]);
    }

    fn event(kind: &str, at: u32, reviewer: Option<&str>, team: Option<&str>) -> TimelineEvent {
        TimelineEvent {
            event: kind.into(),
            created_at: Some(DateTime::UNIX_EPOCH + chrono::Duration::days(at.into())),
            requested_reviewer: reviewer.map(|login| User {
                login: login.into(),
            }),
            requested_team: team.map(|slug| Team { slug: slug.into() }),
        }
    }

    fn day(at: u32) -> Option<DateTime<Utc>> {
        Some(DateTime::UNIX_EPOCH + chrono::Duration::days(at.into()))
    }

    #[test]
    fn finds_when_review_was_requested() {
        let events = [
            event("committed", 1, None, None),
            event("review_requested", 2, Some("Me"), None),
            event("review_requested", 3, Some("other"), None),
            event("reviewed", 4, None, None),
            event("review_requested", 5, Some("me"), None),
        ];
        // The latest request of me, ignoring case.
        assert_eq!(review_requested_at(&events, "me"), day(5));

        let removed = [
            event("review_requested", 2, Some("me"), None),
            event("review_request_removed", 3, Some("me"), None),
        ];
        assert_eq!(review_requested_at(&removed, "me"), None);

        // Teams are the fallback.
        let teams = [
            event("review_requested", 2, None, Some("a")),
            event("review_requested", 4, None, Some("b")),
            event("review_request_removed", 5, None, Some("b")),
        ];
        assert_eq!(review_requested_at(&teams, "me"), day(2));
        let mut both = teams.to_vec();
        both.push(event("review_requested", 1, Some("me"), None));
        assert_eq!(review_requested_at(&both, "me"), day(1));
        assert_eq!(review_requested_at(&[], "me"), None);
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
