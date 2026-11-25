//! A client for Bugzilla's REST API, authenticated with an API key.

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use reqwest::StatusCode;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// Mozilla's Bugzilla.
pub const DEFAULT_URL: &str = "https://bugzilla.mozilla.org";

/// Results per page of a search.
const PAGE_SIZE: usize = 500;
/// Stops runaway paging.
const MAX_PAGES: usize = 5;
/// The error code for an invalid or revoked API key.
const INVALID_KEY: i64 = 306;

#[derive(Debug, thiserror::Error)]
pub enum BugzillaError {
    /// The API key is invalid or revoked.
    #[error("Bugzilla didn't accept the API key")]
    Rejected,
    #[error("Bugzilla returned {0}")]
    Api(String),
    #[error("unexpected response from Bugzilla: {0}")]
    Unexpected(String),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
}

/// The base URL to use, given an optional configured one.
pub fn base_url(configured: Option<&str>) -> String {
    configured
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .unwrap_or(DEFAULT_URL)
        .trim_end_matches('/')
        .to_string()
}

pub struct Bugzilla {
    http: reqwest::Client,
    base_url: String,
    key: String,
}

impl Bugzilla {
    pub fn new(http: reqwest::Client, base_url: &str, key: &str) -> Self {
        Self {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
            key: key.to_string(),
        }
    }

    pub fn bug_url(&self, id: u64) -> String {
        format!("{}/show_bug.cgi?id={id}", self.base_url)
    }

    /// GETs `/rest/<path>`.
    async fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<T, BugzillaError> {
        let response = self
            .http
            .get(format!("{}/rest/{path}", self.base_url))
            .header("X-BUGZILLA-API-KEY", &self.key)
            .query(query)
            .send()
            .await?;
        let status = response.status();
        let body: Value = response
            .json()
            .await
            .map_err(|error| BugzillaError::Unexpected(error.to_string()))?;
        // `error` is `true` or `1`.
        let failed = body["error"] == true || body["error"] == 1;
        if failed || !status.is_success() {
            if body["code"] == INVALID_KEY || status == StatusCode::UNAUTHORIZED {
                return Err(BugzillaError::Rejected);
            }
            let message = body["message"].as_str().unwrap_or_default();
            return Err(BugzillaError::Api(format!("{status}: {message}")));
        }
        serde_json::from_value(body).map_err(|error| BugzillaError::Unexpected(error.to_string()))
    }

    pub async fn whoami(&self) -> Result<WhoAmI, BugzillaError> {
        self.get("whoami", &[]).await
    }

    /// Searches bugs, following pages.
    pub async fn search(&self, query: &[(&str, String)]) -> Result<Vec<Bug>, BugzillaError> {
        let mut bugs = Vec::new();
        for page in 0..MAX_PAGES {
            let mut query = query.to_vec();
            query.push(("order", "bug_id".to_string()));
            query.push(("limit", PAGE_SIZE.to_string()));
            query.push(("offset", (page * PAGE_SIZE).to_string()));
            let result: Bugs = self.get("bug", &query).await?;
            let done = result.bugs.len() < PAGE_SIZE;
            bugs.extend(result.bugs);
            if done {
                break;
            }
        }
        Ok(bugs)
    }

    /// The flags on a bug's attachments that aren't obsolete.
    pub async fn attachment_flags(&self, bug: u64) -> Result<Vec<Flag>, BugzillaError> {
        let result: Value = self
            .get(
                &format!("bug/{bug}/attachment"),
                &[("include_fields", "id,is_obsolete,flags".to_string())],
            )
            .await?;
        let attachments: Vec<Attachment> =
            serde_json::from_value(result["bugs"][bug.to_string()].clone()).unwrap_or_default();
        Ok(attachments
            .into_iter()
            .filter(|a| !a.is_obsolete)
            .flat_map(|a| a.flags)
            .collect())
    }

    /// The names of requestable flags recently requested of `login`.
    pub async fn requested_flag_names(&self, login: &str) -> Result<Vec<String>, BugzillaError> {
        let events: Vec<FlagActivity> = self
            .get(
                &format!("review/flag_activity/requestee/{}", path_segment(login)),
                &[("limit", "500".to_string())],
            )
            .await?;
        let names: BTreeSet<String> = events
            .into_iter()
            .filter(|e| e.r#type.is_requesteeble)
            .map(|e| e.r#type.name)
            .collect();
        Ok(names.into_iter().collect())
    }
}

/// Percent-encodes one URL path segment.
fn path_segment(segment: &str) -> String {
    let mut url = reqwest::Url::parse("http://x/").unwrap();
    url.path_segments_mut().unwrap().push(segment);
    url.path()[1..].to_string()
}

/// The user an API key belongs to.
#[derive(Debug, Deserialize)]
pub struct WhoAmI {
    pub id: Value,
    /// The login, usually the email address.
    pub name: String,
}

#[derive(Deserialize)]
struct Bugs {
    bugs: Vec<Bug>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Bug {
    pub id: u64,
    pub summary: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub product: String,
    #[serde(default)]
    pub component: String,
    pub last_change_time: DateTime<Utc>,
    #[serde(default)]
    pub flags: Vec<Flag>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Flag {
    pub name: String,
    pub status: String,
    #[serde(default)]
    pub setter: Option<String>,
    #[serde(default)]
    pub requestee: Option<String>,
    pub modification_date: DateTime<Utc>,
}

#[derive(Deserialize)]
struct Attachment {
    #[serde(default, deserialize_with = "truthy")]
    is_obsolete: bool,
    #[serde(default)]
    flags: Vec<Flag>,
}

#[derive(Deserialize)]
struct FlagActivity {
    r#type: FlagType,
}

#[derive(Deserialize)]
struct FlagType {
    name: String,
    #[serde(default, deserialize_with = "truthy")]
    is_requesteeble: bool,
}

/// Bugzilla sends booleans as `true`/`false` or `1`/`0`.
fn truthy<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
    let value = Value::deserialize(deserializer)?;
    Ok(value == true || value == 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_the_base_url() {
        assert_eq!(base_url(None), DEFAULT_URL);
        assert_eq!(
            base_url(Some(" https://bugzilla.example.com/ ")),
            "https://bugzilla.example.com"
        );
    }

    #[test]
    fn encodes_logins_in_paths() {
        assert_eq!(path_segment("me@example.com"), "me@example.com");
        assert_eq!(path_segment("a/b c"), "a%2Fb%20c");
    }
}
