//! A client for Phabricator's Conduit API.
//!
//! Calls are form POSTs to `<api>/<method>` with the parameters as JSON in
//! `params`, the token inside them under `__conduit__`, as Mozilla's
//! moz-phab does. Responses are `{result, error_code, error_info}`.

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

/// Mozilla's Phabricator.
pub const DEFAULT_API_URL: &str = "https://phabricator.services.mozilla.com/api";

/// Search endpoints return at most this many results per page.
const PAGE_SIZE: u64 = 100;
/// Stops runaway paging; 1000 results is far more than any list here needs.
const MAX_PAGES: usize = 10;

#[derive(Debug, thiserror::Error)]
pub enum ConduitError {
    /// The token is invalid, expired or revoked.
    #[error("Phabricator didn't accept the token")]
    Rejected,
    #[error("Phabricator returned {code}: {info}")]
    Api { code: String, info: String },
    #[error("unexpected response from Phabricator: {0}")]
    Unexpected(String),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
}

#[derive(Deserialize)]
struct Envelope {
    #[serde(default)]
    result: Value,
    error_code: Option<String>,
    error_info: Option<String>,
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

pub struct Conduit {
    http: reqwest::Client,
    api_url: String,
    token: String,
}

impl Conduit {
    pub fn new(http: reqwest::Client, api_url: &str, token: &str) -> Self {
        Self {
            http,
            api_url: api_url.trim_end_matches('/').to_string(),
            token: token.to_string(),
        }
    }

    /// Calls a method, returning its `result`.
    pub async fn call(&self, method: &str, params: Value) -> Result<Value, ConduitError> {
        let mut params = match params {
            Value::Object(params) => params,
            _ => serde_json::Map::new(),
        };
        params.insert("__conduit__".to_string(), json!({ "token": self.token }));
        let response = self
            .http
            .post(format!("{}/{method}", self.api_url))
            .form(&[
                ("params", Value::Object(params).to_string()),
                ("output", "json".to_string()),
                ("__conduit__", "true".to_string()),
            ])
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            return Err(ConduitError::Api {
                code: status.to_string(),
                info: String::new(),
            });
        }
        let envelope: Envelope = response
            .json()
            .await
            .map_err(|error| ConduitError::Unexpected(error.to_string()))?;
        match envelope.error_code {
            None => Ok(envelope.result),
            Some(code) if code == "ERR-INVALID-AUTH" => Err(ConduitError::Rejected),
            Some(code) => Err(ConduitError::Api {
                code,
                info: envelope.error_info.unwrap_or_default(),
            }),
        }
    }

    /// Calls a `*.search` method, following pages, and decodes each result.
    pub async fn search<T: DeserializeOwned>(
        &self,
        method: &str,
        mut params: Value,
    ) -> Result<Vec<T>, ConduitError> {
        params["limit"] = json!(PAGE_SIZE);
        let mut results = Vec::new();
        for _ in 0..MAX_PAGES {
            let result = self.call(method, params.clone()).await?;
            let data = result["data"]
                .as_array()
                .ok_or_else(|| ConduitError::Unexpected(format!("{method} returned no data")))?;
            for entry in data {
                results.push(
                    serde_json::from_value(entry.clone())
                        .map_err(|error| ConduitError::Unexpected(error.to_string()))?,
                );
            }
            match result["cursor"]["after"].as_str() {
                Some(after) => params["after"] = json!(after),
                None => break,
            }
        }
        Ok(results)
    }

    /// A revision's transactions (its history).
    pub async fn transactions(&self, revision_id: u64) -> Result<Vec<Transaction>, ConduitError> {
        self.search(
            "transaction.search",
            json!({ "objectIdentifier": format!("D{revision_id}") }),
        )
        .await
    }

    pub async fn whoami(&self) -> Result<WhoAmI, ConduitError> {
        let result = self.call("user.whoami", json!({})).await?;
        serde_json::from_value(result).map_err(|error| ConduitError::Unexpected(error.to_string()))
    }
}

/// The user a token belongs to.
#[derive(Debug, Deserialize)]
pub struct WhoAmI {
    pub phid: String,
    #[serde(rename = "userName")]
    pub user_name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Revision {
    pub id: u64,
    pub fields: RevisionFields,
    #[serde(default)]
    pub attachments: RevisionAttachments,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionFields {
    pub title: String,
    pub uri: String,
    #[serde(rename = "authorPHID")]
    pub author_phid: String,
    pub status: RevisionStatus,
    /// Epoch seconds.
    pub date_modified: i64,
    /// Epoch seconds.
    #[serde(default)]
    pub date_created: i64,
    /// Mozilla's field for the revision's bug, a string or null.
    #[serde(rename = "bugzilla.bug-id", default)]
    pub bug_id: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RevisionStatus {
    pub value: String,
    pub name: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RevisionAttachments {
    #[serde(default)]
    pub reviewers: Reviewers,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Reviewers {
    #[serde(default)]
    pub reviewers: Vec<Reviewer>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Reviewer {
    #[serde(rename = "reviewerPHID")]
    pub phid: String,
    pub status: String,
}

/// One change to a revision, e.g. `reviewers` (with `fields.operations`),
/// `accept`, `request-review`, `update` (a new diff) or `status` (with
/// `fields.old` and `fields.new`).
#[derive(Debug, Clone, Deserialize)]
pub struct Transaction {
    #[serde(rename = "type", default)]
    pub kind: Option<String>,
    /// Epoch seconds.
    #[serde(rename = "dateCreated")]
    pub date_created: i64,
    #[serde(default)]
    pub fields: Value,
}

/// A project (e.g. a review group) with its members.
#[derive(Debug, Deserialize)]
pub struct Project {
    pub phid: String,
    pub fields: ProjectFields,
    #[serde(default)]
    pub attachments: ProjectAttachments,
}

#[derive(Debug, Deserialize)]
pub struct ProjectFields {
    pub name: String,
    #[serde(default)]
    pub slug: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct ProjectAttachments {
    #[serde(default)]
    pub members: Members,
}

#[derive(Debug, Default, Deserialize)]
pub struct Members {
    #[serde(default)]
    pub members: Vec<Member>,
}

#[derive(Debug, Deserialize)]
pub struct Member {
    pub phid: String,
}

#[derive(Debug, Deserialize)]
pub struct User {
    pub phid: String,
    pub fields: UserFields,
}

#[derive(Debug, Deserialize)]
pub struct UserFields {
    pub username: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_the_api_url() {
        assert_eq!(api_url(None), DEFAULT_API_URL);
        assert_eq!(
            api_url(Some(" https://phab.example.com/api/ ")),
            "https://phab.example.com/api"
        );
    }

    #[test]
    fn decodes_revisions() {
        let revision: Revision = serde_json::from_value(json!({
            "id": 123,
            "phid": "PHID-DREV-1",
            "fields": {
                "title": "Bug 1 - Fix it",
                "uri": "https://phabricator.services.mozilla.com/D123",
                "authorPHID": "PHID-USER-a",
                "status": { "value": "needs-review", "name": "Needs Review", "closed": false },
                "dateModified": 1700000000,
                "isDraft": false,
            },
            "attachments": {
                "reviewers": {
                    "reviewers": [
                        { "reviewerPHID": "PHID-USER-b", "status": "added", "isBlocking": false, "actorPHID": null },
                    ],
                },
            },
        }))
        .unwrap();
        assert_eq!(revision.id, 123);
        assert_eq!(revision.fields.status.value, "needs-review");
        assert_eq!(
            revision.attachments.reviewers.reviewers[0].phid,
            "PHID-USER-b"
        );
    }
}
