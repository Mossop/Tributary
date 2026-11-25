//! Lists incomplete tasks from one or all of a Google account's task lists.

use std::sync::Arc;

use anyhow::{Context as _, anyhow};
use async_trait::async_trait;
use chrono::{DateTime, NaiveTime, Utc};
use serde::Deserialize;
use serde_json::Value;

use super::{
    Field, FieldKind, FieldOption, Source, SourceContext, SourceError, SourceItem, SourceType,
};
use crate::accounts::{Accounts, GOOGLE};

const ICON: &str = "https://www.gstatic.com/images/branding/product/1x/tasks_48dp.png";
const SCOPE: &str = "https://www.googleapis.com/auth/tasks.readonly";
/// The `list` value meaning every list.
pub const ALL_LISTS: &str = "*";
const TASKS_URL: &str = "https://tasks.google.com/";

#[derive(Debug, Clone, Deserialize)]
pub struct GoogleTasksConfig {
    pub account: String,
    pub list: String,
}

pub fn source_type() -> SourceType {
    SourceType::new(
        "google-tasks",
        "Google Tasks",
        vec![
            Field {
                key: "account",
                label: "Google account",
                kind: FieldKind::Account,
                required: true,
                placeholder: None,
                help: None,
                provider: Some(GOOGLE),
            },
            Field {
                key: "list",
                label: "Task list",
                kind: FieldKind::Select,
                required: true,
                placeholder: None,
                help: None,
                provider: None,
            },
        ],
        |config, context| {
            let config: GoogleTasksConfig = serde_json::from_value(config.clone())?;
            Ok(Arc::new(GoogleTasksSource::new(context, config)?))
        },
    )
    .with_scope(GOOGLE, SCOPE)
    .with_options(|_, config, context| {
        Box::pin(async move {
            let Some(account) = config.get("account").and_then(Value::as_str) else {
                return Ok(Vec::new());
            };
            let client = TasksClient::new(&context, account.to_string())?;
            let token = client.accounts.access_token(&client.account).await?;
            let mut options = vec![FieldOption {
                value: ALL_LISTS.to_string(),
                label: "All lists".to_string(),
            }];
            options.extend(
                client
                    .lists(&token)
                    .await?
                    .into_iter()
                    .map(|list| FieldOption {
                        value: list.id,
                        label: list.title,
                    }),
            );
            Ok(options)
        })
    })
}

#[derive(Deserialize)]
struct Page<T> {
    #[serde(default = "Vec::new")]
    items: Vec<T>,
    #[serde(rename = "nextPageToken")]
    next_page_token: Option<String>,
}

#[derive(Deserialize)]
struct TaskList {
    id: String,
    title: String,
}

#[derive(Deserialize)]
struct Task {
    id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    notes: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    due: Option<DateTime<Utc>>,
    updated: DateTime<Utc>,
    #[serde(rename = "webViewLink", default)]
    web_view_link: Option<String>,
    #[serde(default)]
    deleted: bool,
}

impl Task {
    fn into_item(self, list: Option<&str>) -> SourceItem {
        let notes = self.notes.filter(|n| !n.trim().is_empty());
        let description = match (list, notes) {
            (Some(list), Some(notes)) => Some(format!("{list} · {notes}")),
            (Some(list), None) => Some(list.to_string()),
            (None, notes) => notes,
        };
        let title = if self.title.trim().is_empty() {
            "(No title)".to_string()
        } else {
            self.title
        };
        SourceItem {
            id: self.id,
            title,
            url: self.web_view_link.unwrap_or_else(|| TASKS_URL.to_string()),
            icon: None,
            modified: self.updated,
            description,
            // Google only keeps the date of a due time. Midday UTC is that
            // date in practically every time zone.
            due: self.due.map(|due| {
                due.date_naive()
                    .and_time(NaiveTime::from_hms_opt(12, 0, 0).unwrap())
                    .and_utc()
            }),
        }
    }
}

/// Calls the Tasks API as an account.
struct TasksClient {
    accounts: Arc<Accounts>,
    account: String,
    http: reqwest::Client,
    api_url: String,
}

impl TasksClient {
    fn new(context: &SourceContext, account: String) -> anyhow::Result<Self> {
        let google = context
            .accounts
            .google()
            .ok_or_else(|| anyhow!("Google isn't configured on the server"))?;
        Ok(Self {
            accounts: context.accounts.clone(),
            account,
            http: google.http().clone(),
            api_url: google
                .config
                .tasks_api_url
                .trim_end_matches('/')
                .to_string(),
        })
    }

    /// Fetches every page of a list endpoint.
    async fn get_all<T: serde::de::DeserializeOwned>(
        &self,
        token: &str,
        path: &str,
        query: &[(&str, &str)],
    ) -> anyhow::Result<Vec<T>> {
        let mut items = Vec::new();
        let mut page_token: Option<String> = None;
        loop {
            let mut request = self
                .http
                .get(format!("{}{path}", self.api_url))
                .bearer_auth(token)
                .query(query)
                .query(&[("maxResults", "100")]);
            if let Some(page_token) = &page_token {
                request = request.query(&[("pageToken", page_token)]);
            }
            let response = request.send().await?;
            let status = response.status();
            if !status.is_success() {
                let body: Value = response.json().await.unwrap_or_default();
                let message = body["error"]["message"].as_str().unwrap_or_default();
                return Err(anyhow!("Google Tasks returned {status}: {message}"));
            }
            let page: Page<T> = response
                .json()
                .await
                .context("unexpected response from Google Tasks")?;
            items.extend(page.items);
            match page.next_page_token {
                Some(next) => page_token = Some(next),
                None => return Ok(items),
            }
        }
    }

    async fn lists(&self, token: &str) -> anyhow::Result<Vec<TaskList>> {
        self.get_all(token, "/users/@me/lists", &[]).await
    }

    async fn tasks(&self, token: &str, list: &str) -> anyhow::Result<Vec<Task>> {
        let path = format!("/lists/{}/tasks", urlencode(list));
        let tasks: Vec<Task> = self
            .get_all(
                token,
                &path,
                &[("showCompleted", "false"), ("showHidden", "false")],
            )
            .await?;
        Ok(tasks
            .into_iter()
            .filter(|t| !t.deleted && t.status.as_deref() != Some("completed"))
            .collect())
    }
}

fn urlencode(segment: &str) -> String {
    let mut url = reqwest::Url::parse("http://x/").unwrap();
    url.path_segments_mut().unwrap().push(segment);
    url.path()[1..].to_string()
}

pub struct GoogleTasksSource {
    client: TasksClient,
    list: String,
}

impl GoogleTasksSource {
    pub fn new(context: &SourceContext, config: GoogleTasksConfig) -> anyhow::Result<Self> {
        Ok(Self {
            client: TasksClient::new(context, config.account)?,
            list: config.list,
        })
    }

    async fn items(&self) -> anyhow::Result<Vec<SourceItem>> {
        let client = &self.client;
        let token = client.accounts.access_token(&client.account).await?;
        if self.list != ALL_LISTS {
            let tasks = client.tasks(&token, &self.list).await?;
            return Ok(tasks.into_iter().map(|t| t.into_item(None)).collect());
        }
        let mut items = Vec::new();
        for list in client.lists(&token).await? {
            let tasks = client.tasks(&token, &list.id).await?;
            items.extend(tasks.into_iter().map(|t| t.into_item(Some(&list.title))));
        }
        Ok(items)
    }
}

#[async_trait]
impl Source for GoogleTasksSource {
    fn icon(&self) -> String {
        ICON.to_string()
    }

    async fn fetch(&self) -> Result<Vec<SourceItem>, SourceError> {
        self.items()
            .await
            .map_err(|error| SourceError::Message(format!("{error:#}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(json: Value) -> Task {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn maps_tasks() {
        let item = task(serde_json::json!({
            "id": "t1",
            "title": "Buy milk",
            "notes": "Semi-skimmed",
            "status": "needsAction",
            "due": "2026-12-01T00:00:00.000Z",
            "updated": "2026-11-20T10:00:00.000Z",
            "webViewLink": "https://tasks.google.com/task/t1",
        }))
        .into_item(None);
        assert_eq!(item.id, "t1");
        assert_eq!(item.title, "Buy milk");
        assert_eq!(item.url, "https://tasks.google.com/task/t1");
        assert_eq!(item.description.as_deref(), Some("Semi-skimmed"));
        assert_eq!(item.modified.to_rfc3339(), "2026-11-20T10:00:00+00:00");
        assert_eq!(item.due.unwrap().to_rfc3339(), "2026-12-01T12:00:00+00:00");
        assert_eq!(item.icon, None);
    }

    #[test]
    fn prefixes_the_list_when_showing_all_lists() {
        let json = serde_json::json!({
            "id": "t1",
            "title": "",
            "updated": "2026-11-20T10:00:00Z",
        });
        let item = task(json.clone()).into_item(Some("Groceries"));
        assert_eq!(item.title, "(No title)");
        assert_eq!(item.description.as_deref(), Some("Groceries"));
        assert_eq!(item.url, TASKS_URL);
        assert_eq!(item.due, None);

        let mut with_notes = json;
        with_notes["notes"] = "Some notes".into();
        let item = task(with_notes).into_item(Some("Groceries"));
        assert_eq!(item.description.as_deref(), Some("Groceries · Some notes"));
    }

    #[test]
    fn encodes_list_ids() {
        assert_eq!(urlencode("abc"), "abc");
        assert_eq!(urlencode("a/b c"), "a%2Fb%20c");
    }
}
